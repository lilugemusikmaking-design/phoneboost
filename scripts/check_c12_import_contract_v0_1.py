#!/usr/bin/env python3
"""Deterministic consistency checks for the proposed C12 import contract."""

from __future__ import annotations

import copy
import json
import re
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
CONTRACT = ROOT / "docs/agentic-integration/contracts/phoneboost-c12-import-v1.proposed.json"
MIB = 1024 * 1024


def require_exact_keys(value: dict[str, Any], expected: set[str], label: str) -> None:
    if set(value) != expected:
        raise ValueError(f"{label}: key set differs")


def validate(candidate: dict[str, Any]) -> None:
    require_exact_keys(
        candidate,
        {"contract", "status", "transport", "source", "limits", "handle", "lifecycle", "storage", "authority"},
        "root",
    )
    if candidate["contract"] != "phoneboost-c12-import-v1":
        raise ValueError("contract identifier")
    if candidate["status"] != "proposed_explicit_approval_required":
        raise ValueError("proposal must remain explicitly unapproved")

    transport = candidate["transport"]
    if transport != {
        "control": "private_same_uid_af_unix",
        "descriptor_transfer": "scm_rights",
        "descriptors_per_request": 1,
        "caller_path_allowed": False,
    }:
        raise ValueError("transport boundary")

    source = candidate["source"]
    require_exact_keys(
        source,
        {"allowed_types", "required_memfd_seals", "copy_from_offset", "copy_chunk_bytes"},
        "source",
    )
    if source["allowed_types"] != ["owned_regular_file", "sealed_memfd"]:
        raise ValueError("source allowlist")
    if set(source["required_memfd_seals"]) != {"seal", "shrink", "grow", "write"}:
        raise ValueError("memfd seals")
    if source["copy_from_offset"] != 0 or source["copy_chunk_bytes"] != 65_536:
        raise ValueError("bounded copy")

    limits = candidate["limits"]
    expected_limits = {
        "maximum_object_bytes": 128 * MIB,
        "maximum_objects_per_requester": 4,
        "maximum_objects_per_uid": 8,
        "maximum_aggregate_bytes_per_uid": 256 * MIB,
        "maximum_receiving_per_requester": 1,
        "maximum_receiving_per_uid": 2,
        "receiving_timeout_ms": 60_000,
        "ready_ttl_ms": 300_000,
        "cancellation_ack_deadline_ms": 5_000,
        "terminal_tombstone_ttl_ms": 300_000,
        "maximum_tombstones_per_uid": 256,
    }
    if limits != expected_limits:
        raise ValueError("resource limits")
    if limits["maximum_aggregate_bytes_per_uid"] < limits["maximum_object_bytes"]:
        raise ValueError("aggregate below object maximum")
    if limits["maximum_receiving_per_uid"] > limits["maximum_objects_per_uid"]:
        raise ValueError("receiver count exceeds object count")

    handle = candidate["handle"]
    if handle != {
        "entropy_bits": 256,
        "encoding": "lowercase_hex",
        "bound_to": ["authenticated_local_connection", "peer_uid", "daemon_incarnation"],
        "logged": False,
        "reused": False,
    }:
        raise ValueError("handle boundary")

    lifecycle = candidate["lifecycle"]
    require_exact_keys(
        lifecycle,
        {
            "states",
            "ready_expiry_extended_by_use",
            "disconnect_receiving",
            "disconnect_ready",
            "daemon_restart",
            "terminal_resurrection_allowed",
            "cancellation_success_requires_cleanup",
            "ambiguous_cleanup_releases_quota",
        },
        "lifecycle",
    )
    if lifecycle["states"] != ["receiving", "ready", "consumed", "cancelled", "expired"]:
        raise ValueError("lifecycle states")
    if any(
        lifecycle[field]
        for field in (
            "ready_expiry_extended_by_use",
            "terminal_resurrection_allowed",
            "ambiguous_cleanup_releases_quota",
        )
    ):
        raise ValueError("lifecycle fail-closed flags")
    if not lifecycle["cancellation_success_requires_cleanup"]:
        raise ValueError("cancellation truth")
    if lifecycle["disconnect_receiving"] != "cancel_and_delete" or lifecycle["disconnect_ready"] != "cancel_and_delete":
        raise ValueError("disconnect cleanup")
    if lifecycle["daemon_restart"] != "delete_all_receiving_and_ready":
        raise ValueError("restart cleanup")

    storage = candidate["storage"]
    if storage != {
        "class": "daemon_owned_disk_backed_volatile_import",
        "root": "xdg_cache_home_phoneboost_import_v1",
        "directory_mode_octal": "0700",
        "object_mode_octal": "0600",
        "symlinks_allowed": False,
        "payload_logged": False,
        "payload_digest_logged": False,
    }:
        raise ValueError("storage boundary")

    authority = candidate["authority"]
    if authority != {
        "is_remote_buffer": False,
        "grants_remote_admission": False,
        "bypasses_c11": False,
        "bypasses_resource_guard": False,
        "adapter_is_requester_only": True,
    }:
        raise ValueError("authority boundary")


def repository_ceiling() -> int:
    source = (ROOT / "crates/pb-pbmux/src/compute.rs").read_text(encoding="utf-8")
    match = re.search(r"MAX_COMPUTE_INPUT_BYTES:\s*u64\s*=\s*(\d+)\s*\*\s*1024\s*\*\s*1024", source)
    if match is None:
        raise ValueError("canonical compute ceiling not found")
    return int(match.group(1)) * MIB


def negative_oracles(candidate: dict[str, Any]) -> None:
    mutations = [
        ("caller path", lambda value: value["transport"].__setitem__("caller_path_allowed", True)),
        ("extra fd", lambda value: value["transport"].__setitem__("descriptors_per_request", 2)),
        ("special source", lambda value: value["source"]["allowed_types"].append("fifo")),
        ("unsealed memfd", lambda value: value["source"].__setitem__("required_memfd_seals", ["write"])),
        ("object oversize", lambda value: value["limits"].__setitem__("maximum_object_bytes", 128 * MIB + 1)),
        ("ttl extension", lambda value: value["lifecycle"].__setitem__("ready_expiry_extended_by_use", True)),
        ("cross-connection handle", lambda value: value["handle"].__setitem__("bound_to", ["peer_uid"])),
        ("logged handle", lambda value: value["handle"].__setitem__("logged", True)),
        ("symlink", lambda value: value["storage"].__setitem__("symlinks_allowed", True)),
        ("authority bypass", lambda value: value["authority"].__setitem__("bypasses_c11", True)),
        ("unknown field", lambda value: value.__setitem__("unexpected", True)),
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
    if candidate["limits"]["maximum_object_bytes"] != repository_ceiling():
        raise ValueError("proposed object maximum differs from canonical compute ceiling")
    negative_oracles(candidate)
    print("C12-IMPORT proposed contract PASS")
    print("RESOURCE limits and repository ceiling PASS")
    print("AUTHORITY/privacy invariants PASS")
    print("NEGATIVE oracles 11/11 PASS")
    print("STATUS explicit approval still required PASS")


if __name__ == "__main__":
    main()
