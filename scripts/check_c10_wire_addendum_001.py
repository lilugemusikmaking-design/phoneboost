#!/usr/bin/env python3
"""Independent C10 COMPUTE V0.1 golden-vector generator and checker."""

from __future__ import annotations

import argparse
import hashlib
import struct
import sys
from dataclasses import dataclass
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parents[1]
VECTOR_DIR = REPOSITORY / "docs" / "protocol" / "c10_wire_v0_1_vectors_001"
README = VECTOR_DIR / "README.md"
MANIFEST = VECTOR_DIR / "MANIFEST.sha256"

PASS = "PASS"
REJECT = "REJECT"
UNSUPPORTED_PROVIDER = "UNSUPPORTED_PROVIDER"
REQUEST_ID_CONFLICT = "REQUEST_ID_CONFLICT"
REPLAY = "REPLAY"
RESERVATION_INVALID = "RESERVATION_INVALID"
NON_RESURRECTED = "NON_RESURRECTED"
HOST_TO_WORKER = "host->worker"
WORKER_TO_HOST = "worker->host"

HEADER_LEN = 40
COMPUTE_CHANNEL = 3
SUBMIT_SIZE = 84
JOB_REQUEST_SIZE = 48
JOB_STATUS_SIZE = 56
RESULT_SIZE = 88
MAX_INPUT = 128 * 1024 * 1024
NATIVE_SCRATCH_BYTES = 8 * 1024 * 1024
REQUEST_ID = 0x3132_3334_3536_3738

ZERO_16 = bytes(16)
ZERO_32 = bytes(32)
LEASE_ID = bytes.fromhex("00112233445566778899aabbccddeeff")
INCARNATION_ID = bytes.fromhex("102132435465768798a9bacbdcedfe0f")
RESERVATION_ID = bytes.fromhex("404142434445464748494a4b4c4d4e4f")
BUFFER_ID = bytes.fromhex("303132333435363738393a3b3c3d3e3f")
JOB_ID = bytes.fromhex("505152535455565758595a5b5c5d5e5f")
ABC_DIGEST = bytes.fromhex(
    "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
)

NONE = 0
STALE_CONTROLLER_LEASE = 1
WRONG_WORKER_INCARNATION = 2
UNSUPPORTED_PROVIDER_REASON = 3
INVALID_INPUT = 4
BUFFER_NOT_FOUND = 5
BUFFER_NOT_OWNED = 6
BUFFER_WRONG_INCARNATION = 7
BUFFER_INVALID_STATE = 8
BUFFER_LOST = 9
BUFFER_FREED = 10
BUFFER_EVICTED = 11
INPUT_TOO_LARGE = 12
RESERVATION_INVALID_REASON = 13
RESOURCE_EXHAUSTED = 14
REQUEST_ID_CONFLICT_REASON = 15
IDEMPOTENCE_TABLE_FULL = 16
JOB_NOT_FOUND = 17
JOB_NOT_OWNED = 18
JOB_NOT_CANCELLABLE = 19
PROVIDER_TIMEOUT = 20
PROVIDER_FAILED = 21
SESSION_LOST = 22
UNSUPPORTED_MESSAGE = 23
INTERNAL_ERROR = 24

INVALID = 0
ACCEPTED = 1
RUNNING = 2
COMPLETED = 3
FAILED = 4
CANCELLED = 5


class CheckFailure(RuntimeError):
    """A deterministic wire or semantic invariant failed."""


@dataclass(frozen=True)
class Vector:
    name: str
    data: bytes
    direction: str
    verdict: str
    note: str


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def frame(message_type: int, flags: int, request_id: int, payload: bytes) -> bytes:
    return struct.pack(
        ">4sBBHHHQQIII",
        b"PBM1",
        1,
        COMPUTE_CHANNEL,
        flags,
        message_type,
        HEADER_LEN,
        request_id,
        0,
        0,
        len(payload),
        len(payload),
    ) + payload


def submit_payload(
    *,
    request_length: int = 3,
    provider_id: int = 1,
    provider_version: int = 1,
    input_kind: int = 1,
    reserved: int = 0,
    reservation_id: bytes = RESERVATION_ID,
) -> bytes:
    payload = struct.pack(
        ">16s16s16sBBBB16sQQ",
        LEASE_ID,
        INCARNATION_ID,
        reservation_id,
        provider_id,
        provider_version,
        input_kind,
        reserved,
        BUFFER_ID,
        0,
        request_length,
    )
    if len(payload) != SUBMIT_SIZE:
        raise CheckFailure("SUBMIT generator size mismatch")
    return payload


def job_request(job_id: bytes = JOB_ID) -> bytes:
    payload = struct.pack(">16s16s16s", LEASE_ID, INCARNATION_ID, job_id)
    if len(payload) != JOB_REQUEST_SIZE:
        raise CheckFailure("job request generator size mismatch")
    return payload


def job_status(
    *,
    state: int,
    present: int,
    reason: int,
    job_id: bytes = JOB_ID,
    provider_id: int = 1,
    provider_version: int = 1,
    reserved: bytes = bytes(2),
) -> bytes:
    if present == 0:
        job_id = ZERO_16
        provider_id = 0
        provider_version = 0
    payload = struct.pack(
        ">BBH16s16s16sBB2s",
        state,
        present,
        reason,
        LEASE_ID,
        INCARNATION_ID,
        job_id,
        provider_id,
        provider_version,
        reserved,
    )
    if len(payload) != JOB_STATUS_SIZE:
        raise CheckFailure("job status generator size mismatch")
    return payload


def result_payload(
    *,
    state: int,
    present: int,
    reason: int,
    digest_present: int,
    digest: bytes,
    job_id: bytes = JOB_ID,
    provider_id: int = 1,
    provider_version: int = 1,
    reserved: int = 0,
) -> bytes:
    if present == 0:
        job_id = ZERO_16
        provider_id = 0
        provider_version = 0
    payload = struct.pack(
        ">BBHBBBB16s16s16s32s",
        state,
        present,
        reason,
        digest_present,
        provider_id,
        provider_version,
        reserved,
        LEASE_ID,
        INCARNATION_ID,
        job_id,
        digest,
    )
    if len(payload) != RESULT_SIZE:
        raise CheckFailure("RESULT generator size mismatch")
    return payload


def build_vectors() -> list[Vector]:
    submit = lambda payload, request_id=REQUEST_ID: frame(1, 0x0007, request_id, payload)
    request = lambda message_type, request_id: frame(
        message_type, 0x0007, request_id, job_request()
    )
    response = lambda message_type, request_id, payload: frame(
        message_type, 0x0003, request_id, payload
    )

    invalid_digest = result_payload(
        state=FAILED,
        present=1,
        reason=PROVIDER_FAILED,
        digest_present=1,
        digest=ABC_DIGEST,
    )
    return [
        Vector(
            "GV-C10-01-submit-blake3-remote-buffer.bin",
            submit(submit_payload()),
            HOST_TO_WORKER,
            PASS,
            "SUBMIT provider 1/1, class-2 reservation, RemoteBuffer `abc`",
        ),
        Vector(
            "GV-C10-02-status-request.bin",
            request(2, REQUEST_ID + 1),
            HOST_TO_WORKER,
            PASS,
            "STATUS request",
        ),
        Vector(
            "GV-C10-03-status-running-response.bin",
            response(
                2,
                REQUEST_ID + 1,
                job_status(state=RUNNING, present=1, reason=NONE),
            ),
            WORKER_TO_HOST,
            PASS,
            "STATUS RUNNING",
        ),
        Vector(
            "GV-C10-04-result-completed.bin",
            response(
                3,
                REQUEST_ID,
                result_payload(
                    state=COMPLETED,
                    present=1,
                    reason=NONE,
                    digest_present=1,
                    digest=ABC_DIGEST,
                ),
            ),
            WORKER_TO_HOST,
            PASS,
            "RESULT COMPLETED with exact BLAKE3(`abc`)",
        ),
        Vector(
            "GV-C10-05-result-failed.bin",
            response(
                3,
                REQUEST_ID + 2,
                result_payload(
                    state=FAILED,
                    present=1,
                    reason=PROVIDER_TIMEOUT,
                    digest_present=0,
                    digest=ZERO_32,
                ),
            ),
            WORKER_TO_HOST,
            PASS,
            "RESULT FAILED PROVIDER_TIMEOUT",
        ),
        Vector(
            "GV-C10-06-cancel-request.bin",
            request(4, REQUEST_ID + 3),
            HOST_TO_WORKER,
            PASS,
            "CANCEL request",
        ),
        Vector(
            "GV-C10-07-cancel-response.bin",
            response(
                4,
                REQUEST_ID + 3,
                job_status(state=CANCELLED, present=1, reason=NONE),
            ),
            WORKER_TO_HOST,
            PASS,
            "CANCEL terminal response",
        ),
        Vector(
            "GV-C10-08-unsupported-provider.bin",
            submit(submit_payload(provider_id=2), REQUEST_ID + 4),
            HOST_TO_WORKER,
            UNSUPPORTED_PROVIDER,
            "unassigned provider pair 2/1",
        ),
        Vector(
            "GV-C10-09-stale-lease.bin",
            response(
                3,
                REQUEST_ID + 5,
                result_payload(
                    state=INVALID,
                    present=0,
                    reason=STALE_CONTROLLER_LEASE,
                    digest_present=0,
                    digest=ZERO_32,
                ),
            ),
            WORKER_TO_HOST,
            PASS,
            "absent-job STALE_CONTROLLER_LEASE",
        ),
        Vector(
            "GV-C10-10-buffer-lost.bin",
            response(
                3,
                REQUEST_ID + 6,
                result_payload(
                    state=INVALID,
                    present=0,
                    reason=BUFFER_LOST,
                    digest_present=0,
                    digest=ZERO_32,
                ),
            ),
            WORKER_TO_HOST,
            PASS,
            "absent-job BUFFER_LOST",
        ),
        Vector(
            "GV-C10-11-invalid-digest-presence.bin",
            response(3, REQUEST_ID + 7, invalid_digest),
            WORKER_TO_HOST,
            REJECT,
            "FAILED illegally carries digest",
        ),
        Vector(
            "GV-C10-12-malformed-reserved-zero.bin",
            submit(submit_payload(reserved=1), REQUEST_ID + 8),
            HOST_TO_WORKER,
            REJECT,
            "SUBMIT reserved byte nonzero",
        ),
        Vector(
            "GV-C10-13-wrong-direction.bin",
            submit(submit_payload(), REQUEST_ID + 9),
            WORKER_TO_HOST,
            REJECT,
            "SUBMIT in worker-to-host direction",
        ),
        Vector(
            "GV-C10-14-request-id-conflict.bin",
            submit(submit_payload(request_length=4)),
            HOST_TO_WORKER,
            REQUEST_ID_CONFLICT,
            "same lease/request_id as vector 01, different SUBMIT payload",
        ),
        Vector(
            "GV-C10-15-duplicate-submit-replay.bin",
            submit(submit_payload()),
            HOST_TO_WORKER,
            REPLAY,
            "byte-identical replay of vector 01",
        ),
        Vector(
            "GV-C10-16-consumed-reservation-reuse.bin",
            submit(submit_payload(), REQUEST_ID + 10),
            HOST_TO_WORKER,
            RESERVATION_INVALID,
            "new request_id attempts reuse of consumed class-2 reservation",
        ),
        Vector(
            "GV-C10-17-session-loss-non-resurrection.bin",
            request(2, REQUEST_ID + 11),
            HOST_TO_WORKER,
            NON_RESURRECTED,
            "fresh-session query cannot resurrect old nonterminal job",
        ),
    ]


def unsigned(data: bytes) -> int:
    return int.from_bytes(data, "big")


def parse_frame(blob: bytes, direction: str) -> tuple[int, int, bytes]:
    if len(blob) < HEADER_LEN:
        raise CheckFailure("truncated PBMUX header")
    header = struct.unpack(">4sBBHHHQQIII", blob[:HEADER_LEN])
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
    ) = header
    payload = blob[HEADER_LEN:]
    expected_flags = 0x0007 if direction == HOST_TO_WORKER else 0x0003
    if magic != b"PBM1" or version != 1 or channel != COMPUTE_CHANNEL:
        raise CheckFailure("invalid fixed PBMUX identity")
    if header_len != HEADER_LEN or flags != expected_flags or request_id == 0:
        raise CheckFailure("invalid C10 header/flag/request profile")
    if sequence != 0 or fragment_index != 0:
        raise CheckFailure("fixture sequence/fragment profile mismatch")
    if payload_len != len(payload) or logical_len != len(payload):
        raise CheckFailure("C10 length or fragmentation profile mismatch")
    if message_type not in (1, 2, 3, 4):
        raise CheckFailure("unknown COMPUTE type")
    if direction == HOST_TO_WORKER and message_type not in (1, 2, 4):
        raise CheckFailure("worker-only COMPUTE type from host")
    if direction == WORKER_TO_HOST and message_type not in (2, 3, 4):
        raise CheckFailure("host-only COMPUTE type from worker")
    return message_type, request_id, payload


def parse_submit(payload: bytes) -> str:
    if len(payload) != SUBMIT_SIZE:
        return REJECT
    lease = payload[0:16]
    incarnation = payload[16:32]
    reservation = payload[32:48]
    provider_id = payload[48]
    provider_version = payload[49]
    input_kind = payload[50]
    reserved = payload[51]
    buffer_id = payload[52:68]
    offset = unsigned(payload[68:76])
    length = unsigned(payload[76:84])
    if lease == ZERO_16 or incarnation == ZERO_16 or reservation == ZERO_16:
        return REJECT
    if reserved != 0 or buffer_id == ZERO_16:
        return REJECT
    if (provider_id, provider_version) != (1, 1):
        return UNSUPPORTED_PROVIDER
    if input_kind != 1:
        return INVALID_INPUT
    if length > MAX_INPUT or offset + length > (1 << 64) - 1:
        return INPUT_TOO_LARGE
    return PASS


def parse_job_request(payload: bytes) -> str:
    if len(payload) != JOB_REQUEST_SIZE:
        return REJECT
    return PASS if all(payload[index : index + 16] != ZERO_16 for index in (0, 16, 32)) else REJECT


def parse_job_status(payload: bytes, message_type: int) -> str:
    if len(payload) != JOB_STATUS_SIZE:
        return REJECT
    state = payload[0]
    present = payload[1]
    reason = unsigned(payload[2:4])
    lease = payload[4:20]
    incarnation = payload[20:36]
    job_id = payload[36:52]
    provider = (payload[52], payload[53])
    reserved = payload[54:56]
    if present not in (0, 1) or reason > INTERNAL_ERROR:
        return REJECT
    if lease == ZERO_16 or incarnation == ZERO_16 or reserved != bytes(2):
        return REJECT
    absent_reasons = {
        STALE_CONTROLLER_LEASE,
        WRONG_WORKER_INCARNATION,
        JOB_NOT_FOUND,
        JOB_NOT_OWNED,
        UNSUPPORTED_MESSAGE,
        INTERNAL_ERROR,
    }
    if present == 0:
        valid = state == INVALID and reason in absent_reasons and job_id == ZERO_16 and provider == (0, 0)
        return PASS if valid else REJECT
    if job_id == ZERO_16 or provider != (1, 1):
        return REJECT
    if message_type == 2:
        valid = state in (ACCEPTED, RUNNING) and reason == NONE
    elif message_type == 4:
        valid = (state == CANCELLED and reason == NONE) or (
            state in (COMPLETED, FAILED, CANCELLED) and reason == JOB_NOT_CANCELLABLE
        )
    else:
        valid = False
    return PASS if valid else REJECT


def parse_result(payload: bytes) -> str:
    if len(payload) != RESULT_SIZE:
        return REJECT
    state = payload[0]
    present = payload[1]
    reason = unsigned(payload[2:4])
    digest_present = payload[4]
    provider = (payload[5], payload[6])
    reserved = payload[7]
    lease = payload[8:24]
    incarnation = payload[24:40]
    job_id = payload[40:56]
    digest = payload[56:88]
    if present not in (0, 1) or digest_present not in (0, 1) or reason > INTERNAL_ERROR:
        return REJECT
    if reserved != 0 or lease == ZERO_16 or incarnation == ZERO_16:
        return REJECT
    if present == 0:
        admission_reasons = set(range(1, 17)) | {UNSUPPORTED_MESSAGE, INTERNAL_ERROR}
        valid = (
            state == INVALID
            and reason in admission_reasons
            and provider == (0, 0)
            and job_id == ZERO_16
            and digest_present == 0
            and digest == ZERO_32
        )
        return PASS if valid else REJECT
    if provider != (1, 1) or job_id == ZERO_16:
        return REJECT
    if state == COMPLETED:
        valid = reason == NONE and digest_present == 1
    elif state == FAILED:
        valid = (
            reason
            in {
                BUFFER_INVALID_STATE,
                BUFFER_LOST,
                BUFFER_FREED,
                BUFFER_EVICTED,
                RESOURCE_EXHAUSTED,
                PROVIDER_TIMEOUT,
                PROVIDER_FAILED,
                SESSION_LOST,
                INTERNAL_ERROR,
            }
            and digest_present == 0
            and digest == ZERO_32
        )
    elif state == CANCELLED:
        valid = reason in (NONE, SESSION_LOST) and digest_present == 0 and digest == ZERO_32
    else:
        valid = False
    return PASS if valid else REJECT


def parse_vector(vector: Vector) -> str:
    try:
        message_type, _, payload = parse_frame(vector.data, vector.direction)
    except CheckFailure:
        return REJECT
    if vector.direction == HOST_TO_WORKER:
        if message_type == 1:
            return parse_submit(payload)
        return parse_job_request(payload)
    if message_type in (2, 4):
        return parse_job_status(payload, message_type)
    return parse_result(payload)


def readme_bytes(vectors: list[Vector]) -> bytes:
    lines = [
        "# C10 COMPUTE Wire V0.1 Locked 001 — Golden Vectors",
        "",
        "Status: TEST-ONLY DOCUMENTATION ORACLES. NO NOISE CIPHERTEXT OR PRODUCTION SECRETS.",
        "",
        "Each `.bin` contains one complete fixed-size PBMUX plaintext frame. Direction and",
        "stateful expected outcomes are metadata in this README and the independent checker.",
        "",
        "| Vector | Direction | Expected | Bytes | SHA-256 | Meaning |",
        "|---|---|---|---:|---|---|",
    ]
    for vector in vectors:
        lines.append(
            f"| `{vector.name}` | `{vector.direction}` | `{vector.verdict}` | {len(vector.data)} "
            f"| `{sha256(vector.data)}` | {vector.note} |"
        )
    lines.extend(
        [
            "",
            "Deterministic constants and layouts are fixed by",
            "`../PHONEBOOST_C10_WIRE_ADDENDUM_V0_1_LOCKED_001.md`.",
            "The checker manually regenerates and parses every byte using Python stdlib only.",
            "",
            "Expected meanings:",
            "",
            "- `PASS`: canonical frame accepted.",
            "- `REJECT`: malformed or direction-invalid logical message.",
            "- `UNSUPPORTED_PROVIDER`: structurally valid typed provider refusal.",
            "- `REQUEST_ID_CONFLICT`: same lease/request ID with different SUBMIT bytes.",
            "- `REPLAY`: identical SUBMIT returns the existing job/outcome without new work.",
            "- `RESERVATION_INVALID`: a consumed class-2 reservation cannot back another job.",
            "- `NON_RESURRECTED`: session loss terminalizes old work and fresh session state",
            "  cannot resume or report it successful.",
            "",
        ]
    )
    return "\n".join(lines).encode("utf-8")


def manifest_bytes(paths: list[Path]) -> bytes:
    return (
        "\n".join(f"{sha256(path.read_bytes())}  {path.name}" for path in sorted(paths)) + "\n"
    ).encode("ascii")


def generate() -> None:
    VECTOR_DIR.mkdir(parents=True, exist_ok=True)
    vectors = build_vectors()
    for vector in vectors:
        (VECTOR_DIR / vector.name).write_bytes(vector.data)
    README.write_bytes(readme_bytes(vectors))
    MANIFEST.write_bytes(
        manifest_bytes([README] + [VECTOR_DIR / vector.name for vector in vectors])
    )


def verify_offsets() -> None:
    submit = submit_payload()
    if submit[0:16] != LEASE_ID or submit[16:32] != INCARNATION_ID:
        raise CheckFailure("SUBMIT authority offsets mismatch")
    if submit[32:48] != RESERVATION_ID or submit[48:52] != bytes((1, 1, 1, 0)):
        raise CheckFailure("SUBMIT reservation/provider/input offsets mismatch")
    if submit[52:68] != BUFFER_ID or unsigned(submit[68:76]) != 0 or unsigned(submit[76:84]) != 3:
        raise CheckFailure("SUBMIT RemoteBuffer offsets mismatch")
    completed = result_payload(
        state=COMPLETED,
        present=1,
        reason=NONE,
        digest_present=1,
        digest=ABC_DIGEST,
    )
    if completed[40:56] != JOB_ID or completed[56:88] != ABC_DIGEST:
        raise CheckFailure("RESULT job/digest offsets mismatch")
    print("OFFSETS exact-layouts PASS")


def verify_semantic_oracles(vectors: list[Vector]) -> None:
    by_name = {vector.name: vector for vector in vectors}
    original = by_name["GV-C10-01-submit-blake3-remote-buffer.bin"]
    conflict = by_name["GV-C10-14-request-id-conflict.bin"]
    duplicate = by_name["GV-C10-15-duplicate-submit-replay.bin"]
    reuse = by_name["GV-C10-16-consumed-reservation-reuse.bin"]
    reconnect_query = by_name["GV-C10-17-session-loss-non-resurrection.bin"]
    if any(parse_vector(vector) != PASS for vector in (original, conflict, duplicate, reuse, reconnect_query)):
        raise CheckFailure("stateful oracle inputs are not structurally canonical")

    original_header = parse_frame(original.data, original.direction)
    conflict_header = parse_frame(conflict.data, conflict.direction)
    duplicate_header = parse_frame(duplicate.data, duplicate.direction)
    if original_header[1] != conflict_header[1] or original_header[2] == conflict_header[2]:
        raise CheckFailure("request-ID conflict does not reuse ID with changed bytes")
    if original.data != duplicate.data:
        raise CheckFailure("duplicate SUBMIT fixture is not byte-identical")

    class Oracle:
        def __init__(self) -> None:
            self.session = "ACTIVE"
            self.reservation = "COMMITTED"
            self.job = "NONE"
            self.jobs_created = 0
            self.held = NATIVE_SCRATCH_BYTES
            self.releases = 0
            self.cache: dict[tuple[bytes, int], bytes] = {}

        def submit(self, request_id: int, payload: bytes) -> str:
            key = (LEASE_ID, request_id)
            if key in self.cache:
                return REPLAY if self.cache[key] == payload else REQUEST_ID_CONFLICT
            if self.session != "ACTIVE" or self.reservation != "COMMITTED":
                return RESERVATION_INVALID
            self.cache[key] = payload
            self.reservation = "CONSUMED"
            self.job = "RUNNING"
            self.jobs_created += 1
            return PASS

        def lose_session(self) -> None:
            if self.session == "LOST":
                return
            self.session = "LOST"
            if self.job in ("ACCEPTED", "RUNNING"):
                self.job = "FAILED_SESSION_LOST"
            if self.reservation == "CONSUMED":
                self.reservation = "CONSUMED_RELEASED"
                self.held -= NATIVE_SCRATCH_BYTES
                self.releases += 1

        def publish_success(self) -> bool:
            if self.session != "ACTIVE" or self.job != "RUNNING":
                return False
            self.job = "COMPLETED"
            return True

        def reconnect_query(self) -> str:
            return NON_RESURRECTED if self.session == "LOST" else PASS

    oracle = Oracle()
    _, original_request, original_payload = original_header
    if oracle.submit(original_request, original_payload) != PASS:
        raise CheckFailure("first SUBMIT was not admitted")
    if oracle.submit(original_request, conflict_header[2]) != REQUEST_ID_CONFLICT:
        raise CheckFailure("changed replay did not conflict")
    if oracle.submit(original_request, duplicate_header[2]) != REPLAY:
        raise CheckFailure("identical replay created new work")
    _, reuse_request, reuse_payload = parse_frame(reuse.data, reuse.direction)
    if oracle.submit(reuse_request, reuse_payload) != RESERVATION_INVALID:
        raise CheckFailure("consumed reservation authorized a second job")
    if oracle.jobs_created != 1:
        raise CheckFailure("SUBMIT replay created multiple jobs")
    oracle.lose_session()
    oracle.lose_session()
    if oracle.publish_success():
        raise CheckFailure("session-lost job later published success")
    if oracle.reconnect_query() != NON_RESURRECTED:
        raise CheckFailure("fresh session resurrected old job")
    if oracle.held != 0 or oracle.releases != 1:
        raise CheckFailure("compute scratch was not released exactly once")
    print("SEMANTIC request-id-conflict PASS")
    print("SEMANTIC duplicate-submit-replay PASS")
    print("SEMANTIC reservation-single-consumption PASS")
    print("SEMANTIC compute-budget-release-exactly-once PASS")
    print("SEMANTIC session-loss-no-success PASS")
    print("SEMANTIC reconnect-non-resurrection PASS")


def verify_negative_matrix() -> None:
    completed_without_digest = result_payload(
        state=COMPLETED,
        present=1,
        reason=NONE,
        digest_present=0,
        digest=ZERO_32,
    )
    if parse_result(completed_without_digest) != REJECT:
        raise CheckFailure("COMPLETED without digest accepted")
    failed_nonzero_digest = result_payload(
        state=FAILED,
        present=1,
        reason=PROVIDER_FAILED,
        digest_present=0,
        digest=ABC_DIGEST,
    )
    if parse_result(failed_nonzero_digest) != REJECT:
        raise CheckFailure("absent digest with nonzero bytes accepted")
    unassigned_reason = result_payload(
        state=FAILED,
        present=1,
        reason=25,
        digest_present=0,
        digest=ZERO_32,
    )
    if parse_result(unassigned_reason) != REJECT:
        raise CheckFailure("unassigned result reason accepted")
    fragmented = bytearray(frame(1, 0x0007, REQUEST_ID, submit_payload()))
    fragmented[6:8] = (0x0005).to_bytes(2, "big")
    if parse_vector(Vector("fragment", bytes(fragmented), HOST_TO_WORKER, REJECT, "")) != REJECT:
        raise CheckFailure("fragmented SUBMIT accepted")
    print("NEGATIVE digest-presence-matrix PASS")
    print("NEGATIVE state-reason-matrix PASS")
    print("NEGATIVE no-fragmentation PASS")


def verify() -> None:
    vectors = build_vectors()
    expected_names = {vector.name for vector in vectors}
    actual_names = {path.name for path in VECTOR_DIR.glob("*.bin")}
    if actual_names != expected_names:
        raise CheckFailure(
            f"vector inventory mismatch missing={sorted(expected_names - actual_names)} "
            f"extra={sorted(actual_names - expected_names)}"
        )
    if README.read_bytes() != readme_bytes(vectors):
        raise CheckFailure("README differs from deterministic oracle")
    semantic_verdicts = {
        REQUEST_ID_CONFLICT,
        REPLAY,
        RESERVATION_INVALID,
        NON_RESURRECTED,
    }
    for vector in vectors:
        if (VECTOR_DIR / vector.name).read_bytes() != vector.data:
            raise CheckFailure(f"{vector.name}: bytes differ from deterministic oracle")
        verdict = parse_vector(vector)
        expected = PASS if vector.verdict in semantic_verdicts else vector.verdict
        if verdict != expected:
            raise CheckFailure(f"{vector.name}: expected {expected}, got {verdict}")
        print(f"VECTOR {vector.name} {vector.verdict}")
    expected_paths = [README] + [VECTOR_DIR / vector.name for vector in vectors]
    if MANIFEST.read_bytes() != manifest_bytes(expected_paths):
        raise CheckFailure("SHA256 manifest mismatch")
    print("REGISTRY COMPUTE 1=SUBMIT 2=STATUS 3=RESULT 4=CANCEL PASS")
    print("REGISTRY provider 1/1=pb.native.blake3/1 PASS")
    print("REGISTRY input 1=REMOTE_BUFFER PASS")
    print("MANIFEST SHA256 PASS")
    verify_offsets()
    verify_semantic_oracles(vectors)
    verify_negative_matrix()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", action="store_true", help="regenerate deterministic artifacts")
    args = parser.parse_args()
    try:
        if args.write:
            generate()
        verify()
    except (CheckFailure, OSError) as error:
        print(f"C10_WIRE_CHECK FAIL: {error}", file=sys.stderr)
        return 1
    print("C10_WIRE_CHECK PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
