#!/usr/bin/env python3
"""Independent C07 V0.1 documentation fixture generator and checker."""

from __future__ import annotations

import argparse
import hashlib
import math
import struct
import sys
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parents[1]
VECTOR_DIR = REPOSITORY / "docs" / "protocol" / "c07_wire_v0_1_vectors_002"
MANIFEST = VECTOR_DIR / "MANIFEST.sha256"
README = VECTOR_DIR / "README.md"

PASS = "PASS"
REJECT = "REJECT"
UNSUPPORTED = "UNSUPPORTED_MESSAGE"

HEADER_LEN = 40
COMMAND_SIZE = 46
ACK_SIZE = 98
HEARTBEAT_SIZE = 110
REQUEST_ID = 0x0102030405060708

ZERO_16 = bytes(16)
ZERO_32 = bytes(32)
LEASE_ID = bytes.fromhex("00112233445566778899aabbccddeeff")
INCARNATION_ID = bytes.fromhex("102132435465768798a9bacbdcedfe0f")
TRACE_ID = bytes.fromhex("1112131415161718191a1b1c1d1e1f20")
PEER_ID = bytes(range(32))


class CheckFailure(RuntimeError):
    """A deterministic fixture or checker invariant failed."""


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def frame(channel: int, message_type: int, flags: int, request_id: int, payload: bytes) -> bytes:
    return struct.pack(
        ">4sBBHHHQQIII",
        b"PBM1",
        1,
        channel,
        flags,
        message_type,
        HEADER_LEN,
        request_id,
        0,  # fixture-only PBMUX sequence
        0,  # fragment_index
        len(payload),
        len(payload),
    ) + payload


def command_payload(
    command_type: int,
    lease_present: int,
    lease_id: bytes,
    command_seq: int,
) -> bytes:
    payload = struct.pack(
        ">BB16sQ16sBBH",
        command_type,
        lease_present,
        lease_id,
        command_seq,
        TRACE_ID,
        0,
        0,
        0,
    )
    if len(payload) != COMMAND_SIZE:
        raise CheckFailure("generator COMMAND size mismatch")
    return payload


def ack_payload(
    ack_state: int,
    reason_code: int,
    command_seq: int,
    expected_present: int,
    expected_next: int,
    result_present: int,
    lease_id: bytes,
    incarnation_id: bytes,
    ttl_remaining_ms: int,
    next_command_seq: int,
) -> bytes:
    payload = struct.pack(
        ">BHQBQB16s16sIQB32s",
        ack_state,
        reason_code,
        command_seq,
        expected_present,
        expected_next,
        result_present,
        lease_id,
        incarnation_id,
        ttl_remaining_ms,
        next_command_seq,
        0,
        ZERO_32,
    )
    if len(payload) != ACK_SIZE:
        raise CheckFailure("generator COMMAND_ACK size mismatch")
    return payload


def heartbeat_payload(
    *,
    lease_present: int,
    lease_id: bytes,
    thermal_status: int = 2,
    headroom_present: int = 1,
    headroom_bits: int = 0x3F000000,
    provider_count: int = 0,
    transport_count: int = 0,
) -> bytes:
    payload = bytearray(HEARTBEAT_SIZE)
    payload[0:32] = PEER_ID
    payload[32:48] = INCARNATION_ID
    payload[48] = lease_present
    payload[49:65] = lease_id
    payload[65:73] = (1_000_000).to_bytes(8, "big")
    payload[73] = thermal_status
    payload[74] = headroom_present
    payload[75:79] = headroom_bits.to_bytes(4, "big")
    payload[79] = 80
    payload[80] = 1
    payload[81] = 0
    payload[82:90] = (1_073_741_824).to_bytes(8, "big")
    payload[90:98] = (134_217_728).to_bytes(8, "big")
    payload[98:106] = (0).to_bytes(8, "big")
    payload[106:108] = (3).to_bytes(2, "big")
    payload[108] = provider_count
    payload[109] = transport_count
    return bytes(payload)


def build_vectors() -> list[tuple[str, bytes, str]]:
    command = lambda payload: frame(0, 5, 0x0007, REQUEST_ID, payload)
    ack = lambda payload: frame(0, 6, 0x0003, REQUEST_ID, payload)
    heartbeat = lambda payload: frame(5, 1, 0x0003, 0, payload)

    vectors = [
        (
            "GV-C07-01-acquire-valid.bin",
            command(command_payload(1, 0, ZERO_16, 0)),
            PASS,
        ),
        (
            "GV-C07-02-renew-valid.bin",
            command(command_payload(2, 1, LEASE_ID, 0)),
            PASS,
        ),
        (
            "GV-C07-03-release-valid.bin",
            command(command_payload(3, 1, LEASE_ID, 1)),
            PASS,
        ),
        (
            "GV-C07-04-acquire-invalid-lease.bin",
            command(command_payload(1, 1, LEASE_ID, 0)),
            REJECT,
        ),
        (
            "GV-C07-05-renew-invalid-no-lease.bin",
            command(command_payload(2, 0, ZERO_16, 0)),
            REJECT,
        ),
        (
            "GV-C07-06-ack-acquire-completed.bin",
            ack(ack_payload(2, 0, 0, 0, 0, 1, LEASE_ID, INCARNATION_ID, 60_000, 0)),
            PASS,
        ),
        (
            "GV-C07-07-ack-renew-completed.bin",
            ack(ack_payload(2, 0, 0, 0, 0, 1, LEASE_ID, INCARNATION_ID, 60_000, 1)),
            PASS,
        ),
        (
            "GV-C07-08-ack-release-completed.bin",
            ack(ack_payload(2, 0, 1, 0, 0, 0, ZERO_16, ZERO_16, 0, 0)),
            PASS,
        ),
        (
            "GV-C07-09-ack-out-of-order.bin",
            ack(ack_payload(3, 3, 7, 1, 1, 0, ZERO_16, ZERO_16, 0, 0)),
            PASS,
        ),
        (
            "GV-C07-10-ack-invalid-missing-expected.bin",
            ack(ack_payload(3, 3, 7, 0, 0, 0, ZERO_16, ZERO_16, 0, 0)),
            REJECT,
        ),
        (
            "GV-HB-01-no-lease.bin",
            heartbeat(heartbeat_payload(lease_present=0, lease_id=ZERO_16)),
            PASS,
        ),
        (
            "GV-HB-02-active-lease.bin",
            heartbeat(heartbeat_payload(lease_present=1, lease_id=LEASE_ID)),
            PASS,
        ),
        (
            "GV-HB-03-headroom-absent.bin",
            heartbeat(
                heartbeat_payload(
                    lease_present=0,
                    lease_id=ZERO_16,
                    headroom_present=0,
                    headroom_bits=0,
                )
            ),
            PASS,
        ),
        (
            "GV-HB-04-headroom-present.bin",
            heartbeat(
                heartbeat_payload(
                    lease_present=1,
                    lease_id=LEASE_ID,
                    headroom_present=1,
                    headroom_bits=0x3E800000,
                )
            ),
            PASS,
        ),
        (
            "GV-HB-05-invalid-provider-count.bin",
            heartbeat(
                heartbeat_payload(
                    lease_present=0,
                    lease_id=ZERO_16,
                    provider_count=1,
                )
            ),
            REJECT,
        ),
        (
            "GV-HB-06-invalid-transport-count.bin",
            heartbeat(
                heartbeat_payload(
                    lease_present=0,
                    lease_id=ZERO_16,
                    transport_count=1,
                )
            ),
            REJECT,
        ),
        (
            "GV-HB-07-invalid-thermal-status.bin",
            heartbeat(
                heartbeat_payload(
                    lease_present=0,
                    lease_id=ZERO_16,
                    thermal_status=7,
                )
            ),
            REJECT,
        ),
        (
            "GV-HB-08-invalid-negative-zero-headroom.bin",
            heartbeat(
                heartbeat_payload(
                    lease_present=0,
                    lease_id=ZERO_16,
                    headroom_present=1,
                    headroom_bits=0x80000000,
                )
            ),
            REJECT,
        ),
    ]
    return vectors


def unsigned(data: bytes) -> int:
    return int.from_bytes(data, "big")


def parse_command(payload: bytes) -> str:
    if len(payload) != COMMAND_SIZE:
        return REJECT
    command_type = payload[0]
    lease_present = payload[1]
    lease_id = payload[2:18]
    command_seq = unsigned(payload[18:26])
    provider_present = payload[42]
    provider_id = payload[43]
    inner_payload_len = unsigned(payload[44:46])

    if lease_present not in (0, 1) or provider_present not in (0, 1):
        return REJECT
    if lease_present == 0 and lease_id != ZERO_16:
        return REJECT
    if provider_present != 0 or provider_id != 0 or inner_payload_len != 0:
        return REJECT
    if command_type not in (1, 2, 3):
        return UNSUPPORTED
    if command_type == 1 and (lease_present != 0 or command_seq != 0):
        return REJECT
    if command_type in (2, 3) and lease_present != 1:
        return REJECT
    return PASS


def parse_ack(payload: bytes) -> str:
    if len(payload) != ACK_SIZE:
        return REJECT
    ack_state = payload[0]
    reason_code = unsigned(payload[1:3])
    expected_present = payload[11]
    expected_next = unsigned(payload[12:20])
    result_present = payload[20]
    result_ref = payload[21:65]
    digest_present = payload[65]
    digest = payload[66:98]

    if expected_present not in (0, 1) or result_present not in (0, 1):
        return REJECT
    if digest_present not in (0, 1):
        return REJECT
    if expected_present == 0 and expected_next != 0:
        return REJECT
    if result_present == 0 and result_ref != bytes(44):
        return REJECT
    if digest_present != 0 or digest != ZERO_32:
        return REJECT
    if ack_state not in (1, 2, 3):
        return REJECT
    if reason_code > 5:
        return UNSUPPORTED

    if ack_state == 1:
        if reason_code != 0 or expected_present != 0 or result_present != 0:
            return REJECT
        return PASS

    if ack_state == 2:
        if reason_code != 0 or expected_present != 0:
            return REJECT
        return PASS

    if reason_code == 0 or result_present != 0:
        return REJECT
    if reason_code == 3:
        if expected_present != 1:
            return REJECT
    elif expected_present != 0:
        return REJECT
    return PASS


def parse_heartbeat(payload: bytes) -> str:
    if len(payload) != HEARTBEAT_SIZE:
        return REJECT
    lease_present = payload[48]
    lease_id = payload[49:65]
    thermal_status = payload[73]
    headroom_present = payload[74]
    headroom_bits = unsigned(payload[75:79])
    battery_percent = payload[79]
    charging = payload[80]
    power_save = payload[81]
    provider_count = payload[108]
    transport_count = payload[109]

    if lease_present not in (0, 1) or headroom_present not in (0, 1):
        return REJECT
    if lease_present == 0 and lease_id != ZERO_16:
        return REJECT
    if thermal_status > 6:
        return REJECT
    if headroom_present == 0:
        if headroom_bits != 0:
            return REJECT
    else:
        if headroom_bits == 0x80000000:
            return REJECT
        value = struct.unpack(">f", payload[75:79])[0]
        if not math.isfinite(value) or value < 0.0:
            return REJECT
    if battery_percent > 100 or charging not in (0, 1) or power_save not in (0, 1):
        return REJECT
    if provider_count != 0 or transport_count != 0:
        return REJECT
    return PASS


def parse_frame(data: bytes) -> str:
    if len(data) < HEADER_LEN:
        return REJECT
    try:
        (
            magic,
            version,
            channel,
            flags,
            message_type,
            header_len,
            request_id,
            sequence,
            fragment_index,
            payload_len,
            logical_len,
        ) = struct.unpack(">4sBBHHHQQIII", data[:HEADER_LEN])
    except struct.error:
        return REJECT

    if magic != b"PBM1" or version != 1 or header_len != HEADER_LEN:
        return REJECT
    if sequence != 0 or fragment_index != 0:
        return REJECT
    if payload_len != len(data) - HEADER_LEN or logical_len != payload_len:
        return REJECT
    payload = data[HEADER_LEN:]

    if (channel, message_type) == (0, 5):
        if flags != 0x0007 or request_id != REQUEST_ID or payload_len != COMMAND_SIZE:
            return REJECT
        return parse_command(payload)
    if (channel, message_type) == (0, 6):
        if flags != 0x0003 or request_id != REQUEST_ID or payload_len != ACK_SIZE:
            return REJECT
        return parse_ack(payload)
    if (channel, message_type) == (5, 1):
        if flags != 0x0003 or request_id != 0 or payload_len != HEARTBEAT_SIZE:
            return REJECT
        return parse_heartbeat(payload)
    return REJECT


def readme_bytes(vectors: list[tuple[str, bytes, str]]) -> bytes:
    lines = [
        "# C07 Wire V0.1 Lock Candidate 002 — Golden Vectors",
        "",
        "Status: TEST-ONLY DOCUMENTATION ORACLES. NO NOISE CIPHERTEXT. NO RUNTIME AUTHORITY.",
        "",
        "Each `.bin` is a complete PBMUX plaintext frame: 40-byte header plus payload.",
        "Fixture request IDs and PBMUX sequence values follow the lock-candidate test profile.",
        "",
        "| Vector | Expected | Raw payload hex | Full PBMUX plaintext frame hex |",
        "|---|---|---|---|",
    ]
    for name, data, verdict in vectors:
        lines.append(f"| `{name}` | `{verdict}` | `{data[HEADER_LEN:].hex()}` | `{data.hex()}` |")
    lines.extend(
        [
            "",
            "Expected meanings:",
            "",
            "- `PASS`: exact V0.1 payload accepted by the independent oracle.",
            "- `UNSUPPORTED_MESSAGE`: structurally valid envelope with unsupported V0.1 operation/schema value.",
            "- `REJECT`: malformed or noncanonical logical payload.",
            "",
        ]
    )
    return ("\n".join(lines)).encode("utf-8")


def manifest_bytes(paths: list[Path]) -> bytes:
    lines = [f"{sha256(path.read_bytes())}  {path.name}" for path in sorted(paths)]
    return ("\n".join(lines) + "\n").encode("ascii")


def generate() -> None:
    VECTOR_DIR.mkdir(parents=True, exist_ok=True)
    vectors = build_vectors()
    for name, data, _ in vectors:
        (VECTOR_DIR / name).write_bytes(data)
    README.write_bytes(readme_bytes(vectors))
    artifact_paths = [README] + [VECTOR_DIR / name for name, _, _ in vectors]
    MANIFEST.write_bytes(manifest_bytes(artifact_paths))


def verify_manifest(expected_paths: list[Path]) -> None:
    expected = manifest_bytes(expected_paths)
    actual = MANIFEST.read_bytes()
    if actual != expected:
        raise CheckFailure("SHA256 manifest mismatch")


def verify_vectors() -> dict[str, bytes]:
    vectors = build_vectors()
    expected_names = {name for name, _, _ in vectors}
    actual_names = {path.name for path in VECTOR_DIR.glob("*.bin")}
    if actual_names != expected_names:
        raise CheckFailure(
            f"vector inventory mismatch missing={sorted(expected_names - actual_names)} "
            f"extra={sorted(actual_names - expected_names)}"
        )

    if README.read_bytes() != readme_bytes(vectors):
        raise CheckFailure("README does not match deterministic vector documentation")

    checked: dict[str, bytes] = {}
    for name, expected_data, expected_verdict in vectors:
        path = VECTOR_DIR / name
        actual_data = path.read_bytes()
        if actual_data != expected_data:
            raise CheckFailure(f"{name}: bytes differ from deterministic fixture")
        actual_verdict = parse_frame(actual_data)
        if actual_verdict != expected_verdict:
            raise CheckFailure(
                f"{name}: expected {expected_verdict}, checker returned {actual_verdict}"
            )
        checked[name] = actual_data
        print(f"VECTOR {name} {actual_verdict}")

    expected_paths = [README] + [VECTOR_DIR / name for name in sorted(expected_names)]
    verify_manifest(expected_paths)
    print("MANIFEST PASS")
    return checked


def run_oracles(vectors: dict[str, bytes]) -> None:
    structural = bytearray(vectors["GV-C07-01-acquire-valid.bin"])
    structural[32:36] = (COMMAND_SIZE - 1).to_bytes(4, "big")
    structural_verdict = parse_frame(bytes(structural))
    if structural_verdict != REJECT:
        raise CheckFailure("structural false-pass oracle accepted mutated payload_len")
    print("ORACLE_STRUCTURAL PASS checker_verdict=REJECT mutation=payload_len")

    unknown_command = bytearray(vectors["GV-C07-01-acquire-valid.bin"])
    unknown_command[HEADER_LEN] = 9
    unknown_verdict = parse_frame(bytes(unknown_command))
    if unknown_verdict != UNSUPPORTED:
        raise CheckFailure("unknown command_type oracle did not return UNSUPPORTED_MESSAGE")
    print("ORACLE_UNKNOWN_COMMAND PASS checker_verdict=UNSUPPORTED_MESSAGE")

    missing_expected = bytearray(vectors["GV-C07-09-ack-out-of-order.bin"])
    missing_expected[HEADER_LEN + 11] = 0
    missing_expected[HEADER_LEN + 12 : HEADER_LEN + 20] = bytes(8)
    missing_verdict = parse_frame(bytes(missing_expected))
    if missing_verdict != REJECT:
        raise CheckFailure("OUT_OF_ORDER missing expected sequence false-pass")
    print("ORACLE_OUT_OF_ORDER PASS checker_verdict=REJECT")

    thermal = bytearray(vectors["GV-HB-01-no-lease.bin"])
    thermal[HEADER_LEN + 73] = 7
    thermal_verdict = parse_frame(bytes(thermal))
    if thermal_verdict != REJECT:
        raise CheckFailure("thermal status 7 false-pass")
    print("ORACLE_THERMAL_STATUS PASS checker_verdict=REJECT")

    negative_zero = bytearray(vectors["GV-HB-04-headroom-present.bin"])
    negative_zero[HEADER_LEN + 75 : HEADER_LEN + 79] = bytes.fromhex("80000000")
    negative_zero_verdict = parse_frame(bytes(negative_zero))
    if negative_zero_verdict != REJECT:
        raise CheckFailure("negative-zero headroom false-pass")
    print("ORACLE_NEGATIVE_ZERO PASS checker_verdict=REJECT")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--generate",
        action="store_true",
        help="write the deterministic documentation fixtures before checking",
    )
    args = parser.parse_args()
    try:
        if args.generate:
            generate()
        vectors = verify_vectors()
        run_oracles(vectors)
    except (CheckFailure, OSError, ValueError) as error:
        print(f"C07_WIRE_CHECK FAIL: {error}", file=sys.stderr)
        return 1
    print("C07_WIRE_CHECK PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

