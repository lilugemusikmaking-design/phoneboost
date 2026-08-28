#!/usr/bin/env python3
"""Independent C08/C09 V0.1 golden-vector generator and checker."""

from __future__ import annotations

import argparse
import hashlib
import struct
import sys
from dataclasses import dataclass
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parents[1]
VECTOR_DIR = REPOSITORY / "docs" / "protocol" / "c08_c09_wire_v0_1_vectors_001"
README = VECTOR_DIR / "README.md"
MANIFEST = VECTOR_DIR / "MANIFEST.sha256"

PASS = "PASS"
REJECT = "REJECT"
REQUEST_ID_CONFLICT = "REQUEST_ID_CONFLICT"
HOST_TO_WORKER = "host->worker"
WORKER_TO_HOST = "worker->host"

HEADER_LEN = 40
MAX_PAYLOAD = 61_440
MAX_LOGICAL = 4 * 1024 * 1024
RESOURCE_CHANNEL = 1
REMOTE_BUFFER_CHANNEL = 2
RESOURCE_REQUEST_ID = 0x1112_1314_1516_1718
BUFFER_REQUEST_ID = 0x2122_2324_2526_2728

LEASE_ID = bytes.fromhex("00112233445566778899aabbccddeeff")
INCARNATION_ID = bytes.fromhex("102132435465768798a9bacbdcedfe0f")
RESERVATION_ID = bytes.fromhex("202122232425262728292a2b2c2d2e2f")
BUFFER_ID = bytes.fromhex("303132333435363738393a3b3c3d3e3f")
ZERO_16 = bytes(16)
RESOURCE_BYTES = 128 * 1024 * 1024
RESERVATION_TTL_MS = 30_000
BUFFER_TTL_MS = 300_000

RESOURCE_RESULT_SIZE = 72
EXPIRE_NOTIFY_SIZE = 64
BUFFER_RESULT_PREFIX = 100
PUT_PREFIX = 64


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


def frame(
    channel: int,
    message_type: int,
    flags: int,
    request_id: int,
    payload: bytes,
    *,
    sequence: int = 0,
    fragment_index: int = 0,
    logical_len: int | None = None,
) -> bytes:
    if logical_len is None:
        logical_len = len(payload)
    return struct.pack(
        ">4sBBHHHQQIII",
        b"PBM1",
        1,
        channel,
        flags,
        message_type,
        HEADER_LEN,
        request_id,
        sequence,
        fragment_index,
        len(payload),
        logical_len,
    ) + payload


def reserve_request(amount: int = RESOURCE_BYTES) -> bytes:
    return struct.pack(
        ">16s16sB3sQI", LEASE_ID, INCARNATION_ID, 1, bytes(3), amount, RESERVATION_TTL_MS
    )


def reservation_request() -> bytes:
    return struct.pack(">16s16s16s", LEASE_ID, INCARNATION_ID, RESERVATION_ID)


def resource_result(
    *,
    result_state: int,
    reason: int,
    reservation_present: int,
    reservation_state: int = 0,
    granted: int = 0,
    ttl: int = 0,
) -> bytes:
    reservation = RESERVATION_ID if reservation_present == 1 else ZERO_16
    resource_class = 1 if reservation_present == 1 else 0
    payload = struct.pack(
        ">BBH16s16s16sBB2sQI4s",
        result_state,
        reservation_present,
        reason,
        LEASE_ID,
        INCARNATION_ID,
        reservation,
        reservation_state,
        resource_class,
        bytes(2),
        granted,
        ttl,
        bytes(4),
    )
    if len(payload) != RESOURCE_RESULT_SIZE:
        raise CheckFailure("resource result generator size mismatch")
    return payload


def expire_notify() -> bytes:
    payload = struct.pack(
        ">16s16s16sBBHQ4s",
        LEASE_ID,
        INCARNATION_ID,
        RESERVATION_ID,
        1,
        5,
        6,
        RESOURCE_BYTES,
        bytes(4),
    )
    if len(payload) != EXPIRE_NOTIFY_SIZE:
        raise CheckFailure("expire notification generator size mismatch")
    return payload


def alloc_request() -> bytes:
    return struct.pack(
        ">16s16s16sQII", LEASE_ID, INCARNATION_ID, RESERVATION_ID, RESOURCE_BYTES, 1, 0
    )


def range_request(buffer_id: bytes, offset: int, length: int) -> bytes:
    return struct.pack(">16s16s16sQII", LEASE_ID, INCARNATION_ID, buffer_id, offset, length, 0)


def handle_request(buffer_id: bytes = BUFFER_ID) -> bytes:
    return struct.pack(">16s16s16s", LEASE_ID, INCARNATION_ID, buffer_id)


def buffer_result(
    *,
    result_state: int,
    reason: int,
    buffer_present: int,
    buffer_state: int = 0,
    reservation_present: int = 0,
    flags: int = 0,
    size: int = 0,
    offset: int = 0,
    data_len: int = 0,
    ttl: int = 0,
    body: bytes = b"",
) -> bytes:
    buffer_id = BUFFER_ID if buffer_present == 1 else ZERO_16
    reservation = RESERVATION_ID if reservation_present == 1 else ZERO_16
    prefix = struct.pack(
        ">BBH16s16s16sB16sB2sIQQII",
        result_state,
        buffer_present,
        reason,
        LEASE_ID,
        INCARNATION_ID,
        buffer_id,
        reservation_present,
        reservation,
        buffer_state,
        bytes(2),
        flags,
        size,
        offset,
        data_len,
        ttl,
    )
    if len(prefix) != BUFFER_RESULT_PREFIX:
        raise CheckFailure("buffer result generator prefix size mismatch")
    return prefix + body


def deterministic_body(length: int) -> bytes:
    output = bytearray(length)
    for start in range(0, length, 32):
        block = hashlib.sha256((start // 32).to_bytes(8, "big")).digest()
        take = min(32, length - start)
        output[start : start + take] = block[:take]
    return bytes(output)


def fragmented_put() -> bytes:
    body = deterministic_body(MAX_LOGICAL - PUT_PREFIX)
    logical = range_request(BUFFER_ID, 0, len(body)) + body
    if len(logical) != MAX_LOGICAL:
        raise CheckFailure("4 MiB logical fixture size mismatch")
    chunks = [logical[index : index + MAX_PAYLOAD] for index in range(0, len(logical), MAX_PAYLOAD)]
    if len(chunks) != 69 or len(chunks[-1]) != 16_384:
        raise CheckFailure("4 MiB fragmentation profile mismatch")
    frames = []
    for index, chunk in enumerate(chunks):
        if index == 0:
            flags = 0x0005
            logical_len = len(logical)
        elif index == len(chunks) - 1:
            flags = 0x0006
            logical_len = 0
        else:
            flags = 0x0004
            logical_len = 0
        frames.append(
            frame(
                REMOTE_BUFFER_CHANNEL,
                3,
                flags,
                BUFFER_REQUEST_ID,
                chunk,
                sequence=index,
                fragment_index=index,
                logical_len=logical_len,
            )
        )
    return b"".join(frames)


def build_vectors() -> list[Vector]:
    reserve = lambda payload: frame(RESOURCE_CHANNEL, 1, 0x0007, RESOURCE_REQUEST_ID, payload)
    reserve_ack = lambda payload: frame(RESOURCE_CHANNEL, 2, 0x0003, RESOURCE_REQUEST_ID, payload)
    commit = lambda payload, flags=0x0007: frame(RESOURCE_CHANNEL, 3, flags, RESOURCE_REQUEST_ID + 1, payload)
    release = lambda payload, flags=0x0007: frame(RESOURCE_CHANNEL, 4, flags, RESOURCE_REQUEST_ID + 2, payload)
    request = lambda message_type, payload, request_id=BUFFER_REQUEST_ID: frame(
        REMOTE_BUFFER_CHANNEL, message_type, 0x0007, request_id, payload
    )
    response = lambda message_type, payload, request_id=BUFFER_REQUEST_ID: frame(
        REMOTE_BUFFER_CHANNEL, message_type, 0x0003, request_id, payload
    )

    small = b"phoneboost-c09"
    get_data = b"PB09"
    malformed_presence = bytearray(
        resource_result(
            result_state=2,
            reason=0,
            reservation_present=1,
            reservation_state=1,
            granted=RESOURCE_BYTES,
            ttl=RESERVATION_TTL_MS,
        )
    )
    malformed_presence[1] = 2
    malformed_put = range_request(BUFFER_ID, 0, len(small) + 1) + small

    return [
        Vector("GV-C08-01-reserve-request.bin", reserve(reserve_request()), HOST_TO_WORKER, PASS, "RESERVE"),
        Vector(
            "GV-C08-02-reserve-success.bin",
            reserve_ack(
                resource_result(
                    result_state=2,
                    reason=0,
                    reservation_present=1,
                    reservation_state=1,
                    granted=RESOURCE_BYTES,
                    ttl=RESERVATION_TTL_MS,
                )
            ),
            WORKER_TO_HOST,
            PASS,
            "RESERVE_ACK RESERVED",
        ),
        Vector(
            "GV-C08-03-reserve-refused-stale.bin",
            reserve_ack(resource_result(result_state=3, reason=2, reservation_present=0)),
            WORKER_TO_HOST,
            PASS,
            "RESERVE_ACK REFUSED_STALE_STATE",
        ),
        Vector("GV-C08-04-commit-request.bin", commit(reservation_request()), HOST_TO_WORKER, PASS, "COMMIT"),
        Vector(
            "GV-C08-05-commit-result.bin",
            commit(
                resource_result(
                    result_state=2,
                    reason=0,
                    reservation_present=1,
                    reservation_state=2,
                    granted=RESOURCE_BYTES,
                ),
                0x0003,
            ),
            WORKER_TO_HOST,
            PASS,
            "COMMIT COMPLETED",
        ),
        Vector("GV-C08-06-release-request.bin", release(reservation_request()), HOST_TO_WORKER, PASS, "RELEASE"),
        Vector(
            "GV-C08-07-release-result.bin",
            release(
                resource_result(
                    result_state=2,
                    reason=0,
                    reservation_present=1,
                    reservation_state=4,
                    granted=RESOURCE_BYTES,
                ),
                0x0003,
            ),
            WORKER_TO_HOST,
            PASS,
            "RELEASE COMPLETED",
        ),
        Vector(
            "GV-C08-08-expire-notify.bin",
            frame(RESOURCE_CHANNEL, 5, 0x0003, 0, expire_notify()),
            WORKER_TO_HOST,
            PASS,
            "EXPIRE_NOTIFY",
        ),
        Vector(
            "GV-C08-09-malformed-presence.bin",
            reserve_ack(bytes(malformed_presence)),
            WORKER_TO_HOST,
            REJECT,
            "reservation_present=2",
        ),
        Vector(
            "GV-C08-10-request-id-conflict.bin",
            reserve(reserve_request(RESOURCE_BYTES // 2)),
            HOST_TO_WORKER,
            REQUEST_ID_CONFLICT,
            "same lease/request_id, different amount after GV-C08-01",
        ),
        Vector("GV-C09-01-alloc-request.bin", request(1, alloc_request()), HOST_TO_WORKER, PASS, "ALLOC"),
        Vector(
            "GV-C09-02-alloc-ack.bin",
            response(
                2,
                buffer_result(
                    result_state=2,
                    reason=0,
                    buffer_present=1,
                    buffer_state=1,
                    reservation_present=1,
                    flags=1,
                    size=RESOURCE_BYTES,
                    ttl=BUFFER_TTL_MS,
                ),
            ),
            WORKER_TO_HOST,
            PASS,
            "ALLOC_ACK ALLOCATED",
        ),
        Vector(
            "GV-C09-03-put-small-request.bin",
            request(3, range_request(BUFFER_ID, 0, len(small)) + small, BUFFER_REQUEST_ID + 1),
            HOST_TO_WORKER,
            PASS,
            "PUT small staged body",
        ),
        Vector(
            "GV-C09-04-put-result.bin",
            response(
                3,
                buffer_result(
                    result_state=2,
                    reason=0,
                    buffer_present=1,
                    buffer_state=1,
                    flags=1,
                    size=RESOURCE_BYTES,
                    offset=0,
                    data_len=len(small),
                    ttl=BUFFER_TTL_MS,
                ),
                BUFFER_REQUEST_ID + 1,
            ),
            WORKER_TO_HOST,
            PASS,
            "PUT COMPLETED",
        ),
        Vector(
            "GV-C09-05-get-request.bin",
            request(4, range_request(BUFFER_ID, 0, len(get_data)), BUFFER_REQUEST_ID + 2),
            HOST_TO_WORKER,
            PASS,
            "GET",
        ),
        Vector(
            "GV-C09-06-data-response.bin",
            response(
                5,
                buffer_result(
                    result_state=2,
                    reason=0,
                    buffer_present=1,
                    buffer_state=2,
                    flags=1,
                    size=RESOURCE_BYTES,
                    offset=0,
                    data_len=len(get_data),
                    ttl=BUFFER_TTL_MS,
                    body=get_data,
                ),
                BUFFER_REQUEST_ID + 2,
            ),
            WORKER_TO_HOST,
            PASS,
            "DATA exact body",
        ),
        Vector("GV-C09-07-stat-request.bin", request(7, handle_request(), BUFFER_REQUEST_ID + 3), HOST_TO_WORKER, PASS, "STAT"),
        Vector(
            "GV-C09-08-stat-result.bin",
            response(
                7,
                buffer_result(
                    result_state=2,
                    reason=0,
                    buffer_present=1,
                    buffer_state=2,
                    reservation_present=1,
                    flags=1,
                    size=RESOURCE_BYTES,
                    ttl=BUFFER_TTL_MS,
                ),
                BUFFER_REQUEST_ID + 3,
            ),
            WORKER_TO_HOST,
            PASS,
            "STAT metadata only",
        ),
        Vector("GV-C09-09-touch-request.bin", request(8, handle_request(), BUFFER_REQUEST_ID + 4), HOST_TO_WORKER, PASS, "TOUCH"),
        Vector(
            "GV-C09-10-touch-result.bin",
            response(
                8,
                buffer_result(
                    result_state=2,
                    reason=0,
                    buffer_present=1,
                    buffer_state=2,
                    flags=1,
                    size=RESOURCE_BYTES,
                    ttl=BUFFER_TTL_MS,
                ),
                BUFFER_REQUEST_ID + 4,
            ),
            WORKER_TO_HOST,
            PASS,
            "TOUCH bounded TTL",
        ),
        Vector("GV-C09-11-free-request.bin", request(6, handle_request(), BUFFER_REQUEST_ID + 5), HOST_TO_WORKER, PASS, "FREE"),
        Vector(
            "GV-C09-12-free-result.bin",
            response(
                6,
                buffer_result(
                    result_state=2,
                    reason=0,
                    buffer_present=1,
                    buffer_state=6,
                    flags=1,
                    size=RESOURCE_BYTES,
                ),
                BUFFER_REQUEST_ID + 5,
            ),
            WORKER_TO_HOST,
            PASS,
            "FREE terminal",
        ),
        Vector(
            "GV-C09-13-lost-result.bin",
            response(
                7,
                buffer_result(
                    result_state=3,
                    reason=6,
                    buffer_present=1,
                    buffer_state=5,
                    flags=1,
                    size=RESOURCE_BYTES,
                ),
                BUFFER_REQUEST_ID + 6,
            ),
            WORKER_TO_HOST,
            PASS,
            "BUFFER_LOST tombstone",
        ),
        Vector(
            "GV-C09-14-stale-lease-result.bin",
            response(
                7,
                buffer_result(result_state=3, reason=1, buffer_present=0),
                BUFFER_REQUEST_ID + 7,
            ),
            WORKER_TO_HOST,
            PASS,
            "STALE_CONTROLLER_LEASE no handle leak",
        ),
        Vector(
            "GV-C09-15-malformed-put-length.bin",
            request(3, malformed_put, BUFFER_REQUEST_ID + 8),
            HOST_TO_WORKER,
            REJECT,
            "PUT data_len/body mismatch",
        ),
        Vector(
            "GV-C09-16-put-4mib-fragmented.bin",
            fragmented_put(),
            HOST_TO_WORKER,
            PASS,
            "69 concatenated PBMUX frames, exact 4 MiB logical PUT",
        ),
    ]


def unsigned(data: bytes) -> int:
    return int.from_bytes(data, "big")


def split_frames(blob: bytes) -> list[tuple[tuple[int, ...], bytes]]:
    frames: list[tuple[tuple[int, ...], bytes]] = []
    cursor = 0
    while cursor < len(blob):
        if len(blob) - cursor < HEADER_LEN:
            raise CheckFailure("truncated PBMUX header")
        raw = blob[cursor : cursor + HEADER_LEN]
        try:
            unpacked = struct.unpack(">4sBBHHHQQIII", raw)
        except struct.error as error:
            raise CheckFailure("invalid PBMUX header") from error
        payload_len = unpacked[9]
        end = cursor + HEADER_LEN + payload_len
        if payload_len > MAX_PAYLOAD or end > len(blob):
            raise CheckFailure("invalid PBMUX payload framing")
        payload = blob[cursor + HEADER_LEN : end]
        frames.append((unpacked, payload))
        cursor = end
    return frames


def reassemble(blob: bytes, direction: str) -> tuple[int, int, int, bytes]:
    frames = split_frames(blob)
    if not frames:
        raise CheckFailure("empty vector")
    first, first_payload = frames[0]
    magic, version, channel, flags, message_type, header_len, request_id, sequence, fragment, payload_len, logical_len = first
    if magic != b"PBM1" or version != 1 or header_len != HEADER_LEN:
        raise CheckFailure("invalid fixed PBMUX header")
    if sequence != 0 or fragment != 0 or payload_len != len(first_payload):
        raise CheckFailure("invalid first fragment profile")
    if len(frames) == 1:
        expected_flags = 0x0007 if direction == HOST_TO_WORKER else 0x0003
        if flags != expected_flags or logical_len != len(first_payload):
            raise CheckFailure("invalid unfragmented profile")
        return channel, message_type, request_id, first_payload

    if (channel, message_type, direction) not in (
        (REMOTE_BUFFER_CHANNEL, 3, HOST_TO_WORKER),
        (REMOTE_BUFFER_CHANNEL, 5, WORKER_TO_HOST),
    ):
        raise CheckFailure("message type may not fragment")
    expected_start = 0x0005 if direction == HOST_TO_WORKER else 0x0001
    expected_middle = 0x0004 if direction == HOST_TO_WORKER else 0x0000
    expected_end = 0x0006 if direction == HOST_TO_WORKER else 0x0002
    if flags != expected_start or logical_len == 0 or logical_len > MAX_LOGICAL:
        raise CheckFailure("invalid START fragment")
    logical = bytearray(first_payload)
    for index, (header, payload) in enumerate(frames[1:], 1):
        (
            next_magic,
            next_version,
            next_channel,
            next_flags,
            next_type,
            next_header_len,
            next_request,
            next_sequence,
            next_fragment,
            next_payload_len,
            next_logical_len,
        ) = header
        expected_flags = expected_end if index == len(frames) - 1 else expected_middle
        if (
            next_magic != b"PBM1"
            or next_version != 1
            or next_header_len != HEADER_LEN
            or next_channel != channel
            or next_type != message_type
            or next_request != request_id
            or next_sequence != index
            or next_fragment != index
            or next_flags != expected_flags
            or next_payload_len != len(payload)
            or next_logical_len != 0
        ):
            raise CheckFailure("fragment continuity mismatch")
        logical.extend(payload)
    if len(logical) != logical_len:
        raise CheckFailure("reassembled logical length mismatch")
    return channel, message_type, request_id, bytes(logical)


def parse_resource_result(payload: bytes, message_type: int) -> str:
    if len(payload) != RESOURCE_RESULT_SIZE:
        return REJECT
    result_state = payload[0]
    present = payload[1]
    reason = unsigned(payload[2:4])
    lease = payload[4:20]
    incarnation = payload[20:36]
    reservation = payload[36:52]
    state = payload[52]
    resource_class = payload[53]
    reserved = payload[54:56] + payload[68:72]
    granted = unsigned(payload[56:64])
    ttl = unsigned(payload[64:68])
    if result_state not in (2, 3) or present not in (0, 1) or reason > 12:
        return REJECT
    if lease == ZERO_16 or incarnation == ZERO_16 or reserved != bytes(6):
        return REJECT
    if result_state == 2 and reason != 0 or result_state == 3 and reason == 0:
        return REJECT
    if present == 0:
        if reservation != ZERO_16 or state != 0 or resource_class != 0 or granted != 0 or ttl != 0:
            return REJECT
    else:
        if reservation == ZERO_16 or state not in range(1, 8) or resource_class != 1 or granted == 0:
            return REJECT
        if state == 1:
            if not 1 <= ttl <= RESERVATION_TTL_MS:
                return REJECT
        elif ttl != 0:
            return REJECT
    if result_state == 2:
        expected_state = {2: 1, 3: 2, 4: 4}.get(message_type)
        if present != 1 or state != expected_state:
            return REJECT
    if result_state == 3 and reason in (1, 2, 3, 4, 9, 10, 11, 12) and present != 0:
        return REJECT
    return PASS


def parse_c08(message_type: int, direction: str, request_id: int, payload: bytes) -> str:
    if direction == HOST_TO_WORKER:
        if request_id == 0 or message_type not in (1, 3, 4):
            return REJECT
        if message_type == 1:
            if len(payload) != 48:
                return REJECT
            if payload[0:16] == ZERO_16 or payload[16:32] == ZERO_16:
                return REJECT
            if payload[32] != 1 or payload[33:36] != bytes(3):
                return REJECT
            amount = unsigned(payload[36:44])
            ttl = unsigned(payload[44:48])
            return PASS if 0 < amount <= RESOURCE_BYTES and ttl == RESERVATION_TTL_MS else REJECT
        if len(payload) != 48:
            return REJECT
        return PASS if payload[0:16] != ZERO_16 and payload[16:32] != ZERO_16 and payload[32:48] != ZERO_16 else REJECT

    if message_type in (2, 3, 4):
        if request_id == 0:
            return REJECT
        return parse_resource_result(payload, message_type)
    if message_type == 5:
        if request_id != 0 or len(payload) != EXPIRE_NOTIFY_SIZE:
            return REJECT
        if payload[0:16] == ZERO_16 or payload[16:32] == ZERO_16 or payload[32:48] == ZERO_16:
            return REJECT
        if payload[48] != 1 or payload[49] != 5 or unsigned(payload[50:52]) != 6:
            return REJECT
        if unsigned(payload[52:60]) == 0 or payload[60:64] != bytes(4):
            return REJECT
        return PASS
    return REJECT


def parse_buffer_result(payload: bytes, message_type: int) -> str:
    if len(payload) < BUFFER_RESULT_PREFIX:
        return REJECT
    result_state = payload[0]
    buffer_present = payload[1]
    reason = unsigned(payload[2:4])
    lease = payload[4:20]
    incarnation = payload[20:36]
    buffer_id = payload[36:52]
    reservation_present = payload[52]
    reservation = payload[53:69]
    state = payload[69]
    reserved = payload[70:72]
    flags = unsigned(payload[72:76])
    size = unsigned(payload[76:84])
    offset = unsigned(payload[84:92])
    data_len = unsigned(payload[92:96])
    ttl = unsigned(payload[96:100])
    body = payload[100:]
    if result_state not in (2, 3) or buffer_present not in (0, 1) or reservation_present not in (0, 1):
        return REJECT
    if reason > 15 or lease == ZERO_16 or incarnation == ZERO_16 or reserved != bytes(2):
        return REJECT
    if flags & 0xFFFF_FFF8 or ttl > 1_800_000:
        return REJECT
    if result_state == 2 and reason != 0 or result_state == 3 and reason == 0:
        return REJECT
    if buffer_present == 0:
        if buffer_id != ZERO_16 or state != 0 or flags != 0 or size != 0 or offset != 0 or data_len != 0 or ttl != 0 or body:
            return REJECT
    else:
        if buffer_id == ZERO_16 or state not in range(1, 7) or size == 0:
            return REJECT
        if state in (4, 5, 6) and ttl != 0:
            return REJECT
    if reservation_present == 0:
        if reservation != ZERO_16:
            return REJECT
    elif reservation == ZERO_16:
        return REJECT
    if result_state == 3:
        if body or offset != 0 or data_len != 0 or reservation_present != 0:
            return REJECT
        if reason in (1, 2, 3, 4, 13, 14, 15) and buffer_present != 0:
            return REJECT
        return PASS
    if message_type == 2:
        valid = buffer_present == 1 and reservation_present == 1 and state == 1 and offset == 0 and data_len == 0 and not body and 0 < ttl <= BUFFER_TTL_MS
    elif message_type == 3:
        valid = buffer_present == 1 and reservation_present == 0 and state in (1, 2) and data_len > 0 and not body
    elif message_type == 5:
        valid = buffer_present == 1 and reservation_present == 0 and state == 2 and data_len == len(body) and data_len > 0
    elif message_type == 6:
        valid = buffer_present == 1 and reservation_present == 0 and state == 6 and offset == 0 and data_len == 0 and ttl == 0 and not body
    elif message_type == 7:
        valid = buffer_present == 1 and reservation_present == 1 and offset == 0 and data_len == 0 and not body
    elif message_type == 8:
        valid = buffer_present == 1 and reservation_present == 0 and state in (1, 2) and offset == 0 and data_len == 0 and not body and ttl > 0
    else:
        valid = False
    return PASS if valid else REJECT


def parse_c09(message_type: int, direction: str, request_id: int, payload: bytes) -> str:
    if request_id == 0:
        return REJECT
    if direction == HOST_TO_WORKER:
        if message_type == 1:
            if len(payload) != 64:
                return REJECT
            if payload[0:16] == ZERO_16 or payload[16:32] == ZERO_16 or payload[32:48] == ZERO_16:
                return REJECT
            size = unsigned(payload[48:56])
            flags = unsigned(payload[56:60])
            return PASS if 0 < size <= RESOURCE_BYTES and flags & 0xFFFF_FFF8 == 0 and payload[60:64] == bytes(4) else REJECT
        if message_type in (3, 4):
            if len(payload) < 64 or payload[0:16] == ZERO_16 or payload[16:32] == ZERO_16 or payload[32:48] == ZERO_16:
                return REJECT
            length = unsigned(payload[56:60])
            if payload[60:64] != bytes(4):
                return REJECT
            if message_type == 3:
                return PASS if length == len(payload) - 64 and len(payload) <= MAX_LOGICAL else REJECT
            return PASS if len(payload) == 64 and length <= MAX_LOGICAL - BUFFER_RESULT_PREFIX else REJECT
        if message_type in (6, 7, 8):
            return PASS if len(payload) == 48 and payload[0:16] != ZERO_16 and payload[16:32] != ZERO_16 and payload[32:48] != ZERO_16 else REJECT
        return REJECT
    if message_type not in (2, 3, 5, 6, 7, 8):
        return REJECT
    return parse_buffer_result(payload, message_type)


def parse_vector(vector: Vector) -> str:
    try:
        channel, message_type, request_id, payload = reassemble(vector.data, vector.direction)
    except CheckFailure:
        return REJECT
    if channel == RESOURCE_CHANNEL:
        return parse_c08(message_type, vector.direction, request_id, payload)
    if channel == REMOTE_BUFFER_CHANNEL:
        return parse_c09(message_type, vector.direction, request_id, payload)
    return REJECT


def readme_bytes(vectors: list[Vector]) -> bytes:
    lines = [
        "# C08/C09 Wire V0.1 Locked 001 — Golden Vectors",
        "",
        "Status: TEST-ONLY DOCUMENTATION ORACLES. NO NOISE CIPHERTEXT OR PRODUCTION SECRETS.",
        "",
        "Each ordinary `.bin` contains one complete PBMUX plaintext frame. The 4 MiB fixture",
        "contains 69 complete PBMUX plaintext frames concatenated in sequence; each frame remains",
        "self-delimiting through its 40-byte header and payload length.",
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
            "Deterministic constants and all payload layouts are fixed by",
            "`../PHONEBOOST_C08_C09_WIRE_ADDENDUM_V0_1_LOCKED_001.md`.",
            "The independent checker regenerates every byte without importing PhoneBoost crates.",
            "",
            "Expected meanings:",
            "",
            "- `PASS`: exact V0.1 frame or fragmented logical message accepted.",
            "- `REJECT`: malformed/noncanonical data rejected before domain mutation.",
            "- `REQUEST_ID_CONFLICT`: structurally valid RESERVE that conflicts with the prior",
            "  same-lease/same-request-ID fixture in the stateful C08 oracle.",
            "",
        ]
    )
    return "\n".join(lines).encode("utf-8")


def manifest_bytes(paths: list[Path]) -> bytes:
    return ("\n".join(f"{sha256(path.read_bytes())}  {path.name}" for path in sorted(paths)) + "\n").encode("ascii")


def generate() -> None:
    VECTOR_DIR.mkdir(parents=True, exist_ok=True)
    vectors = build_vectors()
    for vector in vectors:
        (VECTOR_DIR / vector.name).write_bytes(vector.data)
    README.write_bytes(readme_bytes(vectors))
    MANIFEST.write_bytes(manifest_bytes([README] + [VECTOR_DIR / vector.name for vector in vectors]))


def verify_semantic_oracles(vectors: list[Vector]) -> None:
    original = next(vector for vector in vectors if vector.name == "GV-C08-01-reserve-request.bin")
    conflict = next(vector for vector in vectors if vector.name == "GV-C08-10-request-id-conflict.bin")
    if parse_vector(original) != PASS or parse_vector(conflict) != PASS:
        raise CheckFailure("request conflict inputs are not structurally valid")
    original_frame = split_frames(original.data)[0]
    conflict_frame = split_frames(conflict.data)[0]
    if original_frame[0][6] != conflict_frame[0][6] or original_frame[1] == conflict_frame[1]:
        raise CheckFailure("request conflict fixture does not reuse ID with changed parameters")

    class Oracle:
        def __init__(self) -> None:
            self.reservation = "NONE"
            self.buffer = "NONE"
            self.held = 0
            self.releases = 0

        def reserve(self, amount: int) -> None:
            if self.reservation != "NONE" or amount != RESOURCE_BYTES:
                raise CheckFailure("semantic reserve precondition")
            self.reservation = "RESERVED"
            self.held += amount

        def commit(self) -> None:
            if self.reservation != "RESERVED":
                raise CheckFailure("semantic commit precondition")
            self.reservation = "COMMITTED"

        def alloc(self, amount: int) -> bool:
            if self.reservation != "COMMITTED" or self.buffer != "NONE" or amount != RESOURCE_BYTES:
                return False
            before = self.held
            self.reservation = "CONSUMED"
            self.buffer = "ALLOCATED"
            if self.held != before:
                raise CheckFailure("ALLOC changed held-byte total")
            return True

        def lose(self) -> None:
            if self.buffer in ("LOST", "FREED", "EVICTED"):
                return
            if self.buffer == "NONE":
                raise CheckFailure("loss without buffer")
            self.buffer = "LOST"
            self.reservation = "CONSUMED_RELEASED"
            self.held -= RESOURCE_BYTES
            self.releases += 1

    oracle = Oracle()
    oracle.reserve(RESOURCE_BYTES)
    oracle.commit()
    if not oracle.alloc(RESOURCE_BYTES):
        raise CheckFailure("first allocation unexpectedly refused")
    if oracle.alloc(RESOURCE_BYTES):
        raise CheckFailure("second allocation reused consumed reservation")
    oracle.lose()
    oracle.lose()
    if oracle.held != 0 or oracle.releases != 1 or oracle.buffer != "LOST":
        raise CheckFailure("LOST budget release is not exactly once")
    if oracle.alloc(RESOURCE_BYTES):
        raise CheckFailure("LOST buffer or consumed reservation resurrected")
    print("SEMANTIC request-id-conflict PASS")
    print("SEMANTIC reservation-single-consumption PASS")
    print("SEMANTIC lost-no-resurrection PASS")
    print("SEMANTIC budget-release-exactly-once PASS")


def verify_mutation_rejections(vectors: list[Vector]) -> None:
    reserve_success = next(vector for vector in vectors if vector.name == "GV-C08-02-reserve-success.bin")
    mutated = bytearray(reserve_success.data)
    mutated[HEADER_LEN + 54] = 1
    if parse_vector(Vector("mutation", bytes(mutated), WORKER_TO_HOST, REJECT, "")) != REJECT:
        raise CheckFailure("nonzero reserved result byte accepted")

    alloc = next(vector for vector in vectors if vector.name == "GV-C09-02-alloc-ack.bin")
    mutated = bytearray(alloc.data)
    mutated[HEADER_LEN] = 3
    mutated[HEADER_LEN + 2 : HEADER_LEN + 4] = (0).to_bytes(2, "big")
    if parse_vector(Vector("mutation", bytes(mutated), WORKER_TO_HOST, REJECT, "")) != REJECT:
        raise CheckFailure("FAILED/zero-reason result accepted")

    if parse_vector(Vector("direction", alloc.data, HOST_TO_WORKER, REJECT, "")) != REJECT:
        raise CheckFailure("direction-invalid ALLOC_ACK accepted")
    print("NEGATIVE reserved-zero PASS")
    print("NEGATIVE reason-state-profile PASS")
    print("NEGATIVE direction-profile PASS")


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
    for vector in vectors:
        actual = (VECTOR_DIR / vector.name).read_bytes()
        if actual != vector.data:
            raise CheckFailure(f"{vector.name}: bytes differ from deterministic oracle")
        verdict = parse_vector(vector)
        expected = PASS if vector.verdict == REQUEST_ID_CONFLICT else vector.verdict
        if verdict != expected:
            raise CheckFailure(f"{vector.name}: expected {expected}, got {verdict}")
        print(f"VECTOR {vector.name} {vector.verdict}")
    expected_paths = [README] + [VECTOR_DIR / vector.name for vector in vectors]
    if MANIFEST.read_bytes() != manifest_bytes(expected_paths):
        raise CheckFailure("SHA256 manifest mismatch")
    fragmented = next(vector for vector in vectors if vector.name == "GV-C09-16-put-4mib-fragmented.bin")
    frames = split_frames(fragmented.data)
    if len(frames) != 69 or sum(len(payload) for _, payload in frames) != MAX_LOGICAL:
        raise CheckFailure("exact 4 MiB fragmentation oracle mismatch")
    print("FRAGMENTATION exact-4MiB 69-frames PASS")
    print("MANIFEST SHA256 PASS")
    verify_semantic_oracles(vectors)
    verify_mutation_rejections(vectors)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", action="store_true", help="regenerate deterministic artifacts")
    args = parser.parse_args()
    try:
        if args.write:
            generate()
        verify()
    except (CheckFailure, OSError) as error:
        print(f"C08_C09_WIRE_CHECK FAIL: {error}", file=sys.stderr)
        return 1
    print("C08_C09_WIRE_CHECK PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
