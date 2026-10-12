#!/usr/bin/env python3
"""Deterministic checks for the locked asynchronous C12 import wire contract."""

from __future__ import annotations

import copy
import json
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
CONTRACT = ROOT / "docs/agentic-integration/contracts/phoneboost-c12-import-async-wire-v1.json"


def exact_keys(value: dict[str, Any], expected: set[str], label: str) -> None:
    if set(value) != expected:
        raise ValueError(f"{label}: key set differs")


def validate(candidate: dict[str, Any]) -> None:
    exact_keys(candidate, {"contract", "status", "api", "methods", "errors", "authority"}, "root")
    if candidate["contract"] != "phoneboost-c12-import-async-wire-v1":
        raise ValueError("contract identifier")
    if candidate["status"] != "locked_product_owner_approved" or candidate["api"] != 1:
        raise ValueError("approval or API version")

    methods = candidate["methods"]
    if list(methods) != ["imports.create", "imports.get", "imports.cancel"]:
        raise ValueError("exact ordered method set")

    create = methods["imports.create"]
    exact_keys(create, {"class", "params", "descriptors", "result"}, "create")
    if create != {
        "class": "mutating",
        "params": {},
        "descriptors": {"kind": "scm_rights", "exact_count": 1, "truncation_allowed": False},
        "result": {"state": "RECEIVING", "import_handle": "lowercase_hex_64"},
    }:
        raise ValueError("create shape")

    get = methods["imports.get"]
    exact_keys(
        get,
        {
            "class",
            "params",
            "descriptors",
            "result_states",
            "extends_deadline_or_ttl",
            "consumes_import",
        },
        "get",
    )
    if get["class"] != "read_only" or get["params"] != {"import_handle": "lowercase_hex_64"}:
        raise ValueError("get request")
    if get["descriptors"] != {"exact_count": 0}:
        raise ValueError("get descriptors")
    if get["result_states"] != {
        "RECEIVING": [],
        "READY": ["input_bytes"],
        "CANCELLED": [],
        "CONSUMED": [],
        "EXPIRED": [],
    }:
        raise ValueError("get states")
    if get["extends_deadline_or_ttl"] or get["consumes_import"]:
        raise ValueError("get must remain passive")

    cancel = methods["imports.cancel"]
    exact_keys(
        cancel,
        {
            "class",
            "params",
            "descriptors",
            "result_fields",
            "result_states",
            "success_requires_confirmed_cleanup",
            "acknowledgement_deadline_ms",
        },
        "cancel",
    )
    if cancel != {
        "class": "mutating",
        "params": {"import_handle": "lowercase_hex_64"},
        "descriptors": {"exact_count": 0},
        "result_fields": ["state", "state_changed"],
        "result_states": ["CANCELLED", "CONSUMED", "EXPIRED"],
        "success_requires_confirmed_cleanup": True,
        "acknowledgement_deadline_ms": 5000,
    }:
        raise ValueError("cancel shape")

    expected_errors = [
        "LOCAL_BAD_REQUEST",
        "IMPORT_NOT_FOUND",
        "IMPORT_LIMIT_REACHED",
        "IMPORT_SOURCE_REJECTED",
        "IMPORT_RECEIVING_TIMEOUT",
        "IMPORT_CLEANUP_UNCONFIRMED",
        "IMPORT_UNAVAILABLE",
    ]
    if candidate["errors"] != expected_errors:
        raise ValueError("closed public errors")

    authority = candidate["authority"]
    if authority != {
        "handle_binding": ["authenticated_local_connection", "peer_uid", "daemon_incarnation"],
        "cross_authority_indistinguishable_from_unknown": True,
        "public_read_or_download_method": False,
        "public_consume_method": False,
        "generic_execution_method": False,
        "bypasses_c11": False,
        "bypasses_resource_guard": False,
        "mcp_present": False,
    }:
        raise ValueError("authority boundary")


def negative_oracles(candidate: dict[str, Any]) -> None:
    mutations = [
        ("path param", lambda item: item["methods"]["imports.create"]["params"].update(path="/tmp/x")),
        ("missing fd", lambda item: item["methods"]["imports.create"]["descriptors"].update(exact_count=0)),
        ("two fds", lambda item: item["methods"]["imports.create"]["descriptors"].update(exact_count=2)),
        ("truncation", lambda item: item["methods"]["imports.create"]["descriptors"].update(truncation_allowed=True)),
        ("get descriptor", lambda item: item["methods"]["imports.get"]["descriptors"].update(exact_count=1)),
        ("get consumes", lambda item: item["methods"]["imports.get"].update(consumes_import=True)),
        ("cancel early success", lambda item: item["methods"]["imports.cancel"].update(success_requires_confirmed_cleanup=False)),
        ("cancel deadline", lambda item: item["methods"]["imports.cancel"].update(acknowledgement_deadline_ms=5001)),
        ("cross authority oracle", lambda item: item["authority"].update(cross_authority_indistinguishable_from_unknown=False)),
        ("public read", lambda item: item["authority"].update(public_read_or_download_method=True)),
        ("generic execution", lambda item: item["authority"].update(generic_execution_method=True)),
        ("mcp", lambda item: item["authority"].update(mcp_present=True)),
    ]
    for label, mutate in mutations:
        changed = copy.deepcopy(candidate)
        mutate(changed)
        try:
            validate(changed)
        except ValueError:
            continue
        raise AssertionError(f"negative oracle accepted: {label}")


def main() -> None:
    candidate = json.loads(CONTRACT.read_text(encoding="utf-8"))
    validate(candidate)
    negative_oracles(candidate)
    print("C12-IMPORT-ASYNC locked wire contract PASS")
    print("METHODS exact 3/3 PASS")
    print("AUTHORITY/cancellation boundary PASS")
    print("NEGATIVE oracles 12/12 PASS")
    print("MCP absent PASS")


if __name__ == "__main__":
    main()
