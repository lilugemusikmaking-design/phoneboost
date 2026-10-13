#!/usr/bin/env python3
"""Validate the proposed first typed C12 import consumer and stop conditions."""

from __future__ import annotations

import copy
import json
from pathlib import Path
from typing import Any, Callable


ROOT = Path(__file__).resolve().parents[1]
CONTRACT = ROOT / "docs/agentic-integration/contracts/phoneboost-c12-imported-blake3-consumer-v1.json"
ASYNC_CONTRACT = ROOT / "docs/agentic-integration/contracts/phoneboost-c12-import-async-wire-v1.json"


def exact_keys(value: dict[str, Any], expected: set[str], label: str) -> None:
    if set(value) != expected:
        raise ValueError(f"{label}: key set differs")


def validate(candidate: dict[str, Any]) -> None:
    exact_keys(
        candidate,
        {"contract", "status", "api", "method", "lifecycle", "execution", "errors", "authority", "change_control"},
        "root",
    )
    if candidate["contract"] != "phoneboost-c12-imported-blake3-consumer-v1":
        raise ValueError("contract identifier")
    if candidate["status"] != "proposal_product_owner_approval_required" or candidate["api"] != 1:
        raise ValueError("proposal status or API")

    method = candidate["method"]
    exact_keys(
        method,
        {"name", "class", "params", "descriptors", "result_fields", "result_constants", "echoes_import_handle", "c12_job_identifier"},
        "method",
    )
    if method != {
        "name": "compute.submit_import",
        "class": "mutating",
        "params": {"op_id": "pb.native.blake3/1", "import_handle": "lowercase_hex_64"},
        "descriptors": {"exact_count": 0},
        "result_fields": [
            "state",
            "op_id",
            "input_kind",
            "input_bytes",
            "digest_blake3_hex",
            "execution_source",
            "auto_use_reason",
        ],
        "result_constants": {
            "state": "COMPLETED",
            "op_id": "pb.native.blake3/1",
            "input_kind": "PRIVATE_IMPORT_V1",
        },
        "echoes_import_handle": False,
        "c12_job_identifier": False,
    }:
        raise ValueError("exact typed method")

    lifecycle = candidate["lifecycle"]
    if lifecycle != {
        "required_state": "READY",
        "minimum_input_bytes": 1,
        "maximum_input_bytes": 134217728,
        "consumption": "atomic_single_use_before_execution",
        "receiving_is_not_consumed": True,
        "empty_ready_is_not_consumed": True,
        "terminal_or_cross_authority_is_not_found": True,
        "replay_after_consume": "IMPORT_NOT_FOUND",
        "resurrection_allowed": False,
    }:
        raise ValueError("lifecycle boundary")

    execution = candidate["execution"]
    if execution != {
        "operation": "BLAKE3",
        "whole_payload_allocation": False,
        "local_digest_chunk_bytes": 65536,
        "remote_upload_chunk": "existing_MAX_PUT_BODY",
        "passes_c11": True,
        "passes_c07_c08_c09_c10": True,
        "passes_resource_guard": True,
        "execution_source_strings_unchanged": True,
        "auto_use_reason_strings_unchanged": True,
        "synchronous_terminal_c12_response": True,
        "c12_retry_or_replay": False,
    }:
        raise ValueError("execution boundary")

    if candidate["errors"] != [
        "LOCAL_BAD_REQUEST",
        "LOCAL_UNSUPPORTED_METHOD",
        "IMPORT_NOT_FOUND",
        "IMPORT_NOT_READY",
        "IMPORT_INPUT_REJECTED",
        "IMPORT_UNAVAILABLE",
    ]:
        raise ValueError("closed errors")
    if candidate["authority"] != {
        "handle_binding": ["authenticated_local_connection", "peer_uid", "daemon_incarnation"],
        "caller_path_allowed": False,
        "public_read_or_download_method": False,
        "generic_execution_method": False,
        "mcp_present": False,
    }:
        raise ValueError("authority boundary")
    if candidate["change_control"] != {
        "implementation_authorized": False,
        "product_owner_approval_required": True,
    }:
        raise ValueError("stop condition")


def negative_oracles(candidate: dict[str, Any]) -> None:
    mutations: list[tuple[str, Callable[[dict[str, Any]], None]]] = [
        ("reuse compute.submit", lambda item: item["method"].update(name="compute.submit")),
        ("generic op", lambda item: item["method"]["params"].update(op_id="caller_selected")),
        ("path", lambda item: item["method"]["params"].update(path="/tmp/input")),
        ("descriptor", lambda item: item["method"]["descriptors"].update(exact_count=1)),
        ("echo handle", lambda item: item["method"].update(echoes_import_handle=True)),
        ("C12 job", lambda item: item["method"].update(c12_job_identifier=True)),
        ("empty consume", lambda item: item["lifecycle"].update(empty_ready_is_not_consumed=False)),
        ("two consumers", lambda item: item["lifecycle"].update(consumption="replayable")),
        ("resurrection", lambda item: item["lifecycle"].update(resurrection_allowed=True)),
        ("whole payload", lambda item: item["execution"].update(whole_payload_allocation=True)),
        ("C11 bypass", lambda item: item["execution"].update(passes_c11=False)),
        ("ResourceGuard bypass", lambda item: item["execution"].update(passes_resource_guard=False)),
        ("generic execution", lambda item: item["authority"].update(generic_execution_method=True)),
        ("public read", lambda item: item["authority"].update(public_read_or_download_method=True)),
        ("MCP", lambda item: item["authority"].update(mcp_present=True)),
        ("implementation", lambda item: item["change_control"].update(implementation_authorized=True)),
    ]
    for label, mutate in mutations:
        changed = copy.deepcopy(candidate)
        mutate(changed)
        try:
            validate(changed)
        except ValueError:
            continue
        raise AssertionError(f"negative oracle accepted: {label}")


def validate_existing_boundary() -> None:
    async_contract = json.loads(ASYNC_CONTRACT.read_text(encoding="utf-8"))
    if list(async_contract["methods"]) != ["imports.create", "imports.get", "imports.cancel"]:
        raise ValueError("locked async method set changed")
    authority = async_contract["authority"]
    if authority["public_read_or_download_method"] or authority["generic_execution_method"] or authority["mcp_present"]:
        raise ValueError("locked async stop condition changed")


def main() -> None:
    candidate = json.loads(CONTRACT.read_text(encoding="utf-8"))
    validate(candidate)
    negative_oracles(candidate)
    validate_existing_boundary()
    print("C12 IMPORTED BLAKE3 consumer proposal PASS")
    print("METHOD exact typed candidate 1/1 PASS")
    print("LIFECYCLE/streaming/authority boundary PASS")
    print("NEGATIVE oracles 16/16 PASS")
    print("IMPLEMENTATION unauthorized and MCP absent PASS")


if __name__ == "__main__":
    main()
