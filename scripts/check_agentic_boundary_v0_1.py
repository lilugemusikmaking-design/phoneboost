#!/usr/bin/env python3
"""Deterministic checker for the approved pre-C13/C12 boundary artifacts."""

from __future__ import annotations

import copy
import json
import re
import sys
from pathlib import Path
from typing import Any


REPOSITORY = Path(__file__).resolve().parents[1]
ROOT = REPOSITORY / "docs" / "agentic-integration"
CONTRACT = ROOT / "contracts" / "phoneboost-boundary-v1.json"
SCHEMA = ROOT / "schemas" / "phoneboost-perf-profile-v1.schema.json"
FIXTURE = ROOT / "fixtures" / "perf-profile-v1-valid.json"
APPROVAL = ROOT / "PHONEBOOST_PRE_C13_C12_BOUNDARY_DECISION_APPROVAL_V0_1.md"

LINK_CLASSES = [
    "wifi_lan",
    "usb_network",
    "ethernet_ip",
    "local_ip_unknown",
]
TRANSPORT_SIGNATURES = [f"pb-link-v1:{value}" for value in LINK_CLASSES]
PROFILE_TTL_MS = 86_400_000
MAX_PROFILE_BYTES = 65_536


class CheckFailure(RuntimeError):
    """A contract, schema, fixture, or negative invariant failed."""


def reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise CheckFailure(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(), object_pairs_hook=reject_duplicate_keys)
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise CheckFailure(f"cannot load {path.relative_to(REPOSITORY)}: {error}") from error


def check_exact_keys(value: dict[str, Any], expected: set[str], where: str) -> None:
    actual = set(value)
    if actual != expected:
        raise CheckFailure(
            f"{where} key mismatch: missing={sorted(expected - actual)} "
            f"extra={sorted(actual - expected)}"
        )


def resolve_ref(schema_root: dict[str, Any], reference: str) -> dict[str, Any]:
    if not reference.startswith("#/"):
        raise CheckFailure(f"external schema reference forbidden: {reference}")
    current: Any = schema_root
    for component in reference[2:].split("/"):
        if not isinstance(current, dict) or component not in current:
            raise CheckFailure(f"unresolved schema reference: {reference}")
        current = current[component]
    if not isinstance(current, dict):
        raise CheckFailure(f"schema reference is not an object: {reference}")
    return current


def validate_schema_value(
    value: Any,
    rule: dict[str, Any],
    schema_root: dict[str, Any],
    where: str = "$",
) -> None:
    if "$ref" in rule:
        validate_schema_value(value, resolve_ref(schema_root, rule["$ref"]), schema_root, where)
        return
    if "const" in rule and value != rule["const"]:
        raise CheckFailure(f"{where}: expected constant {rule['const']!r}")
    if "enum" in rule and value not in rule["enum"]:
        raise CheckFailure(f"{where}: value outside enum")

    kind = rule.get("type")
    if kind == "object":
        if not isinstance(value, dict):
            raise CheckFailure(f"{where}: expected object")
        properties = rule.get("properties", {})
        required = set(rule.get("required", []))
        missing = required - set(value)
        if missing:
            raise CheckFailure(f"{where}: missing {sorted(missing)}")
        if rule.get("additionalProperties") is False:
            extra = set(value) - set(properties)
            if extra:
                raise CheckFailure(f"{where}: unexpected {sorted(extra)}")
        for key, child in value.items():
            if key in properties:
                validate_schema_value(child, properties[key], schema_root, f"{where}.{key}")
    elif kind == "string":
        if not isinstance(value, str):
            raise CheckFailure(f"{where}: expected string")
        if len(value) < rule.get("minLength", 0):
            raise CheckFailure(f"{where}: string too short")
        if len(value) > rule.get("maxLength", sys.maxsize):
            raise CheckFailure(f"{where}: string too long")
        pattern = rule.get("pattern")
        if pattern is not None and re.fullmatch(pattern, value) is None:
            raise CheckFailure(f"{where}: pattern mismatch")
    elif kind == "integer":
        if isinstance(value, bool) or not isinstance(value, int):
            raise CheckFailure(f"{where}: expected integer")
        if value < rule.get("minimum", -sys.maxsize - 1):
            raise CheckFailure(f"{where}: integer below minimum")
        if value > rule.get("maximum", sys.maxsize):
            raise CheckFailure(f"{where}: integer above maximum")
    elif kind == "boolean" and not isinstance(value, bool):
        raise CheckFailure(f"{where}: expected boolean")


def validate_profile(profile: Any, schema: dict[str, Any]) -> None:
    validate_schema_value(profile, schema, schema)
    created = profile["created_wall_ms"]
    expires = profile["expires_wall_ms"]
    if expires - created != PROFILE_TTL_MS:
        raise CheckFailure("profile expiry is not exactly 24 hours")
    key = profile["key"]
    provenance = profile["provenance"]
    expected_path_marker = {
        "native_c12": "native-c12",
        "pb_cli": "pb-cli",
        "mcp_adapter": "mcp-adapter",
    }[provenance["requester_path"]]
    normalized_config = provenance["benchmark_config_id"].replace("_", "-")
    if expected_path_marker not in normalized_config:
        raise CheckFailure("requester path is absent from benchmark_config_id provenance")
    if any(forbidden in json.dumps(profile).lower() for forbidden in [
        '"path"',
        '"payload"',
        '"ssid"',
        '"bssid"',
        '"serial"',
        '"capability"',
        '"lease_id"',
        '"noise_key"',
    ]):
        raise CheckFailure("profile contains a forbidden persistence field")
    if key["transport_signature"] not in TRANSPORT_SIGNATURES:
        raise CheckFailure("transport signature is not canonical")


def must_reject(profile: dict[str, Any], schema: dict[str, Any], label: str) -> None:
    try:
        validate_profile(profile, schema)
    except CheckFailure:
        return
    raise CheckFailure(f"negative profile accepted: {label}")


def check_contract(contract: dict[str, Any]) -> None:
    check_exact_keys(
        contract,
        {"contract_version", "transport_signature", "live_metrics", "performance_profile", "bulk_ingress", "mcp"},
        "contract",
    )
    if contract["contract_version"] != 1:
        raise CheckFailure("contract version drift")
    transport = contract["transport_signature"]
    if transport["prefix"] != "pb-link-v1:" or transport["classes"] != LINK_CLASSES:
        raise CheckFailure("transport signature contract drift")
    if len(set(TRANSPORT_SIGNATURES)) != len(TRANSPORT_SIGNATURES):
        raise CheckFailure("transport signatures alias")
    if transport["grants_trust"] or transport["creates_fixed_priority"]:
        raise CheckFailure("link class became trust or priority authority")
    required_forbidden = {
        "ip_address", "port", "interface_name", "interface_index", "mac",
        "ssid", "bssid", "usb_serial", "android_serial",
    }
    if set(transport["persisted_identifiers_forbidden"]) != required_forbidden:
        raise CheckFailure("forbidden persisted identifier set drift")

    metrics = contract["live_metrics"]
    expected_metrics = {
        "contract": "pb-link-metrics-v1",
        "freshness_max_age_ms_exclusive": 6000,
        "rtt_source": "authenticated_request_response",
        "rtt_minimum_samples": 3,
        "goodput_source": "completed_authenticated_payload",
        "goodput_minimum_active_window_ms": 1000,
        "clock": "monotonic",
        "missing_speed_behavior": "fail_closed",
    }
    for key, expected in expected_metrics.items():
        if metrics.get(key) != expected:
            raise CheckFailure(f"live metric contract drift: {key}")

    persistence = contract["performance_profile"]
    if persistence["ttl_ms"] != PROFILE_TTL_MS:
        raise CheckFailure("profile TTL drift")
    if persistence["maximum_file_bytes"] != MAX_PROFILE_BYTES:
        raise CheckFailure("maximum profile file size drift")
    if persistence["parent_mode_octal"] != "0700" or persistence["file_mode_octal"] != "0600":
        raise CheckFailure("private persistence mode drift")
    if not persistence["atomic_replace_required"] or not persistence["unknown_fields_rejected"]:
        raise CheckFailure("persistence fail-closed rule drift")

    ingress = contract["bulk_ingress"]
    if ingress["approved_direction"] != "scm_rights_then_private_import":
        raise CheckFailure("bulk ingress direction drift")
    if ingress["implementation_status"] != "deferred_pending_exact_resource_contract":
        raise CheckFailure("bulk ingress implemented before exact resource contract")
    if any(ingress[name] for name in [
        "caller_path_allowed", "shell_allowed", "generic_rpc_allowed",
        "arbitrary_code_allowed", "remote_buffer_authority",
    ]):
        raise CheckFailure("forbidden bulk ingress authority enabled")

    mcp = contract["mcp"]
    if mcp["implementation_status"] != "blocked_until_d1_d2_c13_and_c12_contract_green":
        raise CheckFailure("MCP stop condition drift")
    if any(mcp[name] for name in [
        "placement_authority", "pairing_authority", "lease_authority", "admission_authority",
    ]):
        raise CheckFailure("MCP acquired PhoneBoost authority")


def check_negative_profiles(profile: dict[str, Any], schema: dict[str, Any]) -> None:
    mutations: list[tuple[str, tuple[str, ...], Any]] = [
        ("unknown root field", ("extra",), True),
        ("uppercase peer id", ("key", "peer_id_sha256_hex"), profile["key"]["peer_id_sha256_hex"].upper()),
        ("raw endpoint signature", ("key", "transport_signature"), "pb-link-v1:192.0.2.1:4242"),
        ("unversioned wifi", ("key", "transport_signature"), "wifi_lan"),
        ("path-like workload", ("key", "workload_signature"), "pb-workload-v1:/home/user/source"),
        ("boolean version", ("key", "core_protocol_version"), True),
        ("zero RTT", ("provenance", "median_rtt_ms"), 0),
        ("unknown requester", ("provenance", "requester_path"), "ide"),
        ("wrong benchmark contract", ("provenance", "benchmark_contract"), "pb-bench-v2"),
        ("wrong expiry", ("expires_wall_ms",), profile["expires_wall_ms"] - 1),
    ]
    for label, path, replacement in mutations:
        candidate = copy.deepcopy(profile)
        target = candidate
        for component in path[:-1]:
            target = target[component]
        target[path[-1]] = replacement
        must_reject(candidate, schema, label)

    candidate = copy.deepcopy(profile)
    del candidate["evidence"]["measured_remote_runs"]
    must_reject(candidate, schema, "missing required evidence")


def main() -> int:
    for path in [CONTRACT, SCHEMA, FIXTURE, APPROVAL]:
        if not path.is_file():
            raise CheckFailure(f"missing artifact: {path.relative_to(REPOSITORY)}")
    if FIXTURE.stat().st_size > MAX_PROFILE_BYTES:
        raise CheckFailure("valid fixture exceeds maximum profile file size")

    contract = load_json(CONTRACT)
    schema = load_json(SCHEMA)
    profile = load_json(FIXTURE)
    if not isinstance(contract, dict) or not isinstance(schema, dict) or not isinstance(profile, dict):
        raise CheckFailure("root JSON artifacts must be objects")

    check_contract(contract)
    validate_profile(profile, schema)
    check_negative_profiles(profile, schema)

    approval = APPROVAL.read_text()
    for marker in ["**D1:**", "**D2:**", "**D3:**", "**D4:**", "no MCP implementation"]:
        if marker not in approval:
            raise CheckFailure(f"approval marker missing: {marker}")

    print("AGENTIC-BOUNDARY contract PASS")
    print(f"TRANSPORT signatures {len(TRANSPORT_SIGNATURES)}/4 unique PASS")
    print("LIVE-METRICS freshness/provenance PASS")
    print("C13 schema valid fixture + 11 negative profiles PASS")
    print("C12/MCP stop conditions PASS")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except CheckFailure as error:
        print(f"AGENTIC-BOUNDARY FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
