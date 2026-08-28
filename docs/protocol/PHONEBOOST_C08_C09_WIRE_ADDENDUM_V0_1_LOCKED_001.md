# PhoneBoost C08/C09 Wire Addendum V0.1 — Locked 001

Status: **LOCKED FOR GENERAL POC IMPLEMENTATION**

## 1. Authority and scope

Authority remains:

1. SPEC V0.7
2. Contract Set V1.3
3. Tech Sheet V1.3
4. Pseudocode V1.1
5. Fixture Generation Spec V1.0

`PHONEBOOST_C08_C09_REGISTRY_RESOLUTION_V0_1_001.md` is the project-owner
erratum resolving the SPEC V0.7 TOUCH omission and authorizing the additive C08
native-operation scratch resource class. This addendum closes only the C08
RESOURCE and C09 REMOTE_BUFFER byte-level schemas. It changes no C05, C06, or
C07 layout and creates no production runtime authority by itself.

## 2. General wire rules

- Payloads are binary. JSON is forbidden on the wire.
- All multi-byte integers are unsigned big-endian.
- IDs are opaque byte strings. A 128-bit ID occupies 16 bytes in network order.
- `lease_id`, `worker_incarnation_id`, `reservation_id`, and `buffer_id` are
  nonzero whenever present or required.
- Presence flags are exactly `0` or `1`.
- Bytes allocated to an absent field are all zero.
- Reserved bytes and reserved flag bits are all zero.
- No peer ID or authenticated boolean is carried. Peer ownership is derived
  exclusively from the committed `VerifiedPeerSession` at the worker.
- Lengths, reserved bytes, presence flags, enums, and direction are validated
  before domain dispatch.
- PBMUX sequence remains per-direction SecureSession ordering. C08 does not use
  C07 `command_seq`.
- Schema identity is PBMUX version 1 plus channel, direction, message type, and
  the exact payload profile below. There is no inner version byte.

## 3. Registry and direction

### 3.1 RESOURCE channel 1

| Type | Name | Direction |
|---:|---|---|
| 1 | `RESERVE` | host -> worker request |
| 2 | `RESERVE_ACK` | worker -> host terminal response |
| 3 | `COMMIT` | direction-disambiguated request/terminal response |
| 4 | `RELEASE` | direction-disambiguated request/terminal response |
| 5 | `EXPIRE_NOTIFY` | worker -> host unsolicited advisory |

### 3.2 REMOTE_BUFFER channel 2

| Type | Name | Direction |
|---:|---|---|
| 1 | `ALLOC` | host -> worker request |
| 2 | `ALLOC_ACK` | worker -> host terminal response |
| 3 | `PUT` | direction-disambiguated request/terminal response |
| 4 | `GET` | host -> worker request |
| 5 | `DATA` | worker -> host terminal GET response |
| 6 | `FREE` | direction-disambiguated request/terminal response |
| 7 | `STAT` | direction-disambiguated request/terminal response |
| 8 | `TOUCH` | direction-disambiguated request/terminal response |

Receiving a known type in an invalid direction is a logical-message
`UNSUPPORTED_MESSAGE` failure and never reaches ResourceGuard or
RemoteBufferStore.

## 4. PBMUX profiles and correlation

Every request has a fresh nonzero u64 PBMUX `request_id`. Its terminal response
copies the exact request ID. `EXPIRE_NOTIFY` is unsolicited and has
`request_id = 0`.

C08 request IDs are unique within the active controller lease. The key
`(lease_id, request_id)` is C08 idempotence authority. An exact duplicate type
and payload replays the cached terminal result; reuse with a different type or
payload returns `REQUEST_ID_CONFLICT`. The canonical capacity is 1,024 entries
per lease with five-minute terminal retention or lease end, whichever occurs
first.

C09 request IDs are correlation-only. C09 authority is the authenticated peer,
active lease, current incarnation, provider state, and opaque handle. A second
ALLOC using a consumed reservation fails even with a different request ID.

Fixed requests use:

```text
flags = START|END|ACK_REQUIRED = 0x0007
fragment_index = 0
payload_len = logical_message_len = exact fixed size
```

Fixed terminal responses use:

```text
flags = START|END = 0x0003
fragment_index = 0
payload_len = logical_message_len = exact fixed size
```

## 5. C08 request layouts

### 5.1 RESOURCE/1 RESERVE — exactly 48 bytes

| Offset | Size | Field |
|---:|---:|---|
| 0 | 16 | `lease_id` |
| 16 | 16 | `worker_incarnation_id` |
| 32 | 1 | `resource_class` |
| 33 | 3 | reserved zero |
| 36 | 8 | `requested_bytes` u64 BE |
| 44 | 4 | `reservation_ttl_ms` u32 BE |

The resource-class registry is:

| Value | Name | Valid `requested_bytes` | Exclusive purpose |
|---:|---|---:|---|
| 1 | `REMOTE_BUFFER_BYTES` | `1..=128 MiB` | C09 RemoteBuffer backing storage |
| 2 | `NATIVE_OP_SCRATCH_BYTES` | `1..=8 MiB` | trusted native-operation/provider scratch |

Zero and values 3 through 255 are unsupported. Class 1 is never compute
scratch; class 2 is never RemoteBuffer backing. `reservation_ttl_ms` is exactly
30,000. No truncation or silent TTL defaulting occurs. A known class with zero
or over-profile bytes is an invalid V0.1 RESERVE payload and never mutates
ResourceGuard.

### 5.2 RESOURCE/3 COMMIT request — exactly 48 bytes

| Offset | Size | Field |
|---:|---:|---|
| 0 | 16 | `lease_id` |
| 16 | 16 | `worker_incarnation_id` |
| 32 | 16 | `reservation_id` |

### 5.3 RESOURCE/4 RELEASE request — exactly 48 bytes

The layout is identical to COMMIT request. RELEASE of RESERVED or COMMITTED
state terminalizes the reservation and releases its held budget. RELEASE of a
consumed reservation fails `RESERVATION_ALREADY_CONSUMED`; it never frees the
buffer indirectly. Repeated RELEASE of a known RELEASED reservation is a
successful idempotent RELEASED result.

## 6. C08 terminal result envelope — exactly 72 bytes

RESOURCE/2 RESERVE_ACK, worker-to-host RESOURCE/3 COMMIT, and worker-to-host
RESOURCE/4 RELEASE use the same envelope:

| Offset | Size | Field |
|---:|---:|---|
| 0 | 1 | `result_state` |
| 1 | 1 | `reservation_present` |
| 2 | 2 | `reason_code` u16 BE |
| 4 | 16 | `lease_id` |
| 20 | 16 | current `worker_incarnation_id` |
| 36 | 16 | `reservation_id` |
| 52 | 1 | `reservation_state` |
| 53 | 1 | `resource_class` |
| 54 | 2 | reserved zero |
| 56 | 8 | `granted_bytes` u64 BE |
| 64 | 4 | `ttl_remaining_ms` u32 BE |
| 68 | 4 | reserved zero |

`result_state` is:

| Value | State |
|---:|---|
| 2 | `COMPLETED` |
| 3 | `FAILED` |

No ACCEPTED/nonterminal response exists. COMPLETED requires `reason_code = 0`;
FAILED requires a nonzero assigned reason.

`reservation_state` is:

| Value | State | Holds budget |
|---:|---|---|
| 0 | `NONE` | no |
| 1 | `RESERVED` | yes |
| 2 | `COMMITTED` | yes |
| 3 | `CONSUMED` | budget transferred to the authorized class-specific consumer |
| 4 | `RELEASED` | no |
| 5 | `EXPIRED` | no |
| 6 | `REFUSED_SAFETY` | no |
| 7 | `CONSUMED_RELEASED` | no; permanently non-reusable |

If `reservation_present = 0`, reservation ID, state, class, granted bytes, and
TTL are zero. If it is 1, those fields describe a reservation authenticated as
owned by the requesting peer/lease/incarnation. RESERVED has relative TTL in
`1..=30000`; every other state has TTL zero.

Successful profiles are exact:

- RESERVE_ACK: present, state RESERVED, class echoes request (1 or 2), granted
  bytes equal requested.
- COMMIT: present, state COMMITTED, class is the reservation class, TTL zero.
- RELEASE: present, state RELEASED, class is the reservation class, TTL zero.

Known owned failures may carry their actual reservation state. Stale lease,
not-found, request-conflict, unsupported, and internal failures carry no
reservation reference.

### 6.1 C08 local reason registry

| Value | Reason |
|---:|---|
| 0 | `NONE` |
| 1 | `STALE_CONTROLLER_LEASE` |
| 2 | `REFUSED_STALE_STATE` |
| 3 | `RESOURCE_EXHAUSTED` |
| 4 | `RESERVATION_NOT_FOUND` |
| 5 | `RESERVATION_NOT_COMMITTED` |
| 6 | `RESERVATION_EXPIRED` |
| 7 | `RESERVATION_ALREADY_CONSUMED` |
| 8 | `COMMIT_REFUSED_SAFETY` |
| 9 | `REQUEST_ID_CONFLICT` |
| 10 | `IDEMPOTENCE_TABLE_FULL` |
| 11 | `UNSUPPORTED_MESSAGE` |
| 12 | `INTERNAL_ERROR` |
| 13..65535 | unassigned |

An unassigned response reason is an unsupported V0.1 logical payload.

## 7. RESOURCE/5 EXPIRE_NOTIFY — exactly 64 bytes

| Offset | Size | Field |
|---:|---:|---|
| 0 | 16 | `lease_id` |
| 16 | 16 | current `worker_incarnation_id` |
| 32 | 16 | `reservation_id` |
| 48 | 1 | `resource_class` = 1 or 2 |
| 49 | 1 | `reservation_state` = EXPIRED / 5 |
| 50 | 2 | `reason_code` = RESERVATION_EXPIRED / 6 |
| 52 | 8 | `granted_bytes` u64 BE |
| 60 | 4 | reserved zero |

The class and granted bytes obey the same per-class profile as RESERVE. It uses
START|END, request ID zero, and no ACK_REQUIRED. It is advisory: loss
of the notification does not alter worker authority. Only an uncommitted
RESERVED reservation expires under the 30-second reservation TTL.

## 8. C09 request layouts

Every C09 request begins with a nonzero active `lease_id` and the nonzero
worker incarnation the host believes current. The worker derives owner peer
from the authenticated session and validates all three authorities.

### 8.1 REMOTE_BUFFER/1 ALLOC — exactly 64 bytes

| Offset | Size | Field |
|---:|---:|---|
| 0 | 16 | `lease_id` |
| 16 | 16 | `worker_incarnation_id` |
| 32 | 16 | `reservation_id` |
| 48 | 8 | `size_bytes` u64 BE |
| 56 | 4 | `allocation_flags` u32 BE |
| 60 | 4 | reserved zero |

Flag bits are:

| Mask | Name |
|---:|---|
| `0x00000001` | `EVICTABLE` |
| `0x00000002` | `RECONSTRUCTIBLE` |
| `0x00000004` | `SENSITIVE_D3` |

Bits `0xfffffff8` are reserved zero. Zero flags is a legal explicit
non-evictable/non-reconstructible/non-D3 request; host APIs may choose
EVICTABLE as their default. Size is nonzero and no greater than 128 MiB.

ALLOC succeeds only when the reservation is COMMITTED, owned by the same
peer/lease/current incarnation, unconsumed, class 1, and has granted bytes
exactly equal to `size_bytes`.

### 8.2 REMOTE_BUFFER/3 PUT — 64-byte prefix plus data

| Offset | Size | Field |
|---:|---:|---|
| 0 | 16 | `lease_id` |
| 16 | 16 | `worker_incarnation_id` |
| 32 | 16 | `buffer_id` |
| 48 | 8 | `offset` u64 BE |
| 56 | 4 | `data_len` u32 BE |
| 60 | 4 | reserved zero |
| 64 | N | exact staged data body |

`data_len = N` and total logical length is `64 + N`. The maximum body is
4,194,240 bytes so the complete logical message remains at most 4 MiB. A
zero-length or out-of-bounds operation is a domain `BUFFER_RANGE_INVALID`
failure. A body-length mismatch is malformed and never dispatches.

### 8.3 REMOTE_BUFFER/4 GET — exactly 64 bytes

| Offset | Size | Field |
|---:|---:|---|
| 0 | 16 | `lease_id` |
| 16 | 16 | `worker_incarnation_id` |
| 32 | 16 | `buffer_id` |
| 48 | 8 | `offset` u64 BE |
| 56 | 4 | `length` u32 BE |
| 60 | 4 | reserved zero |

Length is nonzero and no greater than 4,194,204 bytes, the maximum DATA body
that fits after the 100-byte response prefix. GET requires READY.

### 8.4 REMOTE_BUFFER/6 FREE, /7 STAT, /8 TOUCH — exactly 48 bytes

| Offset | Size | Field |
|---:|---:|---|
| 0 | 16 | `lease_id` |
| 16 | 16 | `worker_incarnation_id` |
| 32 | 16 | `buffer_id` |

STAT is metadata-only. TOUCH sets expiry to
`min(now + 300000, created + 1800000)` and cannot resurrect terminal state.
FREE is terminal and idempotent while its tombstone remains locally known.
For a forgotten bounded tombstone it returns BUFFER_NOT_FOUND, never success
against reused storage.

## 9. C09 terminal response prefix — 100 bytes plus optional DATA

Worker-to-host ALLOC_ACK/2, PUT/3, DATA/5, FREE/6, STAT/7, and TOUCH/8 use:

| Offset | Size | Field |
|---:|---:|---|
| 0 | 1 | `result_state` |
| 1 | 1 | `buffer_present` |
| 2 | 2 | `reason_code` u16 BE |
| 4 | 16 | request `lease_id` |
| 20 | 16 | current `worker_incarnation_id` |
| 36 | 16 | `buffer_id` |
| 52 | 1 | `reservation_present` |
| 53 | 16 | `reservation_id` |
| 69 | 1 | `buffer_state` |
| 70 | 2 | reserved zero |
| 72 | 4 | `allocation_flags` u32 BE |
| 76 | 8 | `size_bytes` u64 BE |
| 84 | 8 | `offset` u64 BE |
| 92 | 4 | `data_len` u32 BE |
| 96 | 4 | `ttl_remaining_ms` u32 BE |
| 100 | N | DATA body, only for successful type 5 |

Result states are COMPLETED=2 and FAILED=3, with the same reason-zero rule as
C08. No nonterminal response exists.

Buffer states are:

| Value | State | Terminal |
|---:|---|---|
| 0 | `NONE` | n/a |
| 1 | `ALLOCATED` | no |
| 2 | `READY` | no |
| 3 | `IN_USE` | no |
| 4 | `EVICTED` | yes |
| 5 | `LOST` | yes |
| 6 | `FREED` | yes |

If `buffer_present = 0`, buffer ID, state, flags, size, offset, data length,
TTL, and DATA body are zero/empty. If present, the record is authenticated as
owned by the requesting peer, lease, and incarnation. Terminal state has TTL
zero. Relative TTL is at most 1,800,000 and never transmits an absolute Android
monotonic timestamp.

If `reservation_present = 0`, reservation bytes are zero. Reservation presence
is 1 only for successful ALLOC_ACK and successful STAT; it identifies the
single reservation backing the buffer.

Exact successful profiles:

- ALLOC_ACK: 100 bytes; buffer and reservation present; state ALLOCATED;
  offset/data length zero; TTL in `1..=300000`.
- PUT response: 100 bytes; buffer present; state ALLOCATED or READY; offset and
  data length equal the committed write; no body.
- DATA response: `100 + N`; buffer present; state READY; offset matches GET;
  data length and body length are exactly N.
- FREE response: 100 bytes; buffer present; state FREED; offset/data/TTL zero.
- STAT response: 100 bytes; buffer and reservation present; metadata only;
  offset/data zero and no body.
- TOUCH response: 100 bytes; buffer present; nonterminal state; offset/data
  zero; TTL reflects the bounded extension.

FAILED has no DATA body and zero offset/data length. A known, authenticated
buffer or tombstone may be present to communicate its actual state; stale
lease, not found, not owned, wrong incarnation, reservation invalid,
unsupported, and internal failures have no buffer or reservation reference.

### 9.1 C09 local reason registry

| Value | Reason |
|---:|---|
| 0 | `NONE` |
| 1 | `STALE_CONTROLLER_LEASE` |
| 2 | `BUFFER_NOT_FOUND` |
| 3 | `BUFFER_NOT_OWNED` |
| 4 | `BUFFER_WRONG_INCARNATION` |
| 5 | `BUFFER_INVALID_STATE` |
| 6 | `BUFFER_LOST` |
| 7 | `BUFFER_FREED` |
| 8 | `BUFFER_EVICTED` |
| 9 | `BUFFER_RANGE_INVALID` |
| 10 | `BUFFER_RANGE_BUSY` |
| 11 | `PAYLOAD_TOO_LARGE` |
| 12 | `RESOURCE_EXHAUSTED` |
| 13 | `RESERVATION_INVALID` |
| 14 | `UNSUPPORTED_MESSAGE` |
| 15 | `INTERNAL_ERROR` |
| 16..65535 | unassigned |

TTL expiry terminalizes to EVICTED and returns BUFFER_EVICTED. Session loss
terminalizes to LOST and returns BUFFER_LOST if a later authoritative request
finds the tombstone. A request carrying an old worker incarnation returns
BUFFER_WRONG_INCARNATION before handle lookup.

## 10. C09 fragmentation

Only PUT requests and successful DATA responses may fragment. Their complete
logical byte stream is the fixed prefix followed by the body.

For fragmented PUT requests:

- START fragment flags are START|ACK_REQUIRED (`0x0005`).
- Middle flags are ACK_REQUIRED (`0x0004`).
- Final flags are END|ACK_REQUIRED (`0x0006`).

For fragmented DATA responses:

- START flags are START (`0x0001`).
- Middle flags are zero.
- Final flags are END (`0x0002`).

All fragments have the same channel, message type, and request ID. Index starts
at zero and increases exactly by one. Only START carries the full nonzero
`logical_message_len`; continuation fragments carry zero. Each payload is at
most 61,440 bytes. The sum is the declared logical length.

An exact 4 MiB logical message has 69 fragments when greedily filled: 68
fragments of 61,440 bytes and one of 16,384 bytes. Oversize START is rejected
before logical allocation.

## 11. Reservation consumption and budget atomicity

One class-1 reservation backs one buffer. WorkerCore serializes this transaction:

1. Validate authenticated peer, active lease, current incarnation, quotas,
   exact size, COMMITTED ownership, and unconsumed reservation.
2. Allocate and initialize non-visible backing plus a fresh nonzero buffer ID.
3. At one publication point, transition reservation COMMITTED -> CONSUMED and
   insert the ALLOCATED buffer record.
4. Only then return successful ALLOC_ACK.

Before publication, any failure drops staging, leaves the reservation
COMMITTED, and exposes no buffer. At publication, held bytes transfer from the
reservation account to the buffer account without changing total held bytes.
There is no visible state with only one side committed.

FREE, policy EVICT, TTL expiry, and LOST atomically terminalize the buffer,
invalidate backing, transition its reservation to CONSUMED_RELEASED, and
release budget once. Repeated cleanup observes the terminal accounting marker
and releases zero additional bytes. CONSUMED and CONSUMED_RELEASED can never
authorize another ALLOC.

One class-2 reservation backs one native compute job. Compute admission
validates COMMITTED ownership for the authenticated peer, active lease, current
incarnation, and current SecureSession, then atomically transitions the
reservation to CONSUMED while publishing exactly one job. Failed admission
leaves it COMMITTED and unconsumed. Job terminalization transitions it to
CONSUMED_RELEASED and releases its held scratch bytes exactly once. A class-2
reservation never backs ALLOC, and class-1 RemoteBuffer bytes are not charged
again as compute scratch.

## 12. Session and worker loss

Each buffer is internally tagged with the session-generation capability that
authorized ALLOC. That token is not on the wire.

Authoritative SecureSession/transport loss is serialized before a replacement
session may dispatch provider work. It stops admissions, invalidates active
range operations not already authoritatively complete, marks every buffer for
that session LOST, removes access to backing, and releases each buffer budget
once. Reconnect never resurrects those handles.

Worker restart performs the same volatile loss and creates a new
worker_incarnation_id. Old-incarnation requests fail explicitly and no content
or tombstone is reconstructed from persistence.

## 13. Malformed messages and failure scope

Existing C06 scopes remain authoritative:

- Noise failure, sequence gap/duplicate, magic/version/header errors, payload
  framing mismatch, or unknown channel: SESSION.
- Known channel but unknown type, invalid direction, fragment inconsistency,
  logical oversize, reassembly quota/timeout, wrong exact fixed size, invalid
  presence, nonzero absent/reserved bytes, invalid enum, response state/reason
  mismatch, or body-length mismatch: LOGICAL_MESSAGE.
- A structurally valid dispatched domain refusal returns its terminal C08/C09
  response with REQUEST/domain scope.
- Backpressure exhaustion fails before logical allocation under existing C06
  rules.

Malformed data never yields a fabricated domain result and never mutates
ResourceGuard or RemoteBufferStore. If the session remains viable, existing
CONTROL/3 ERROR may report the framing/logical failure.

## 14. Deterministic fixture profile

Test-only constants are:

```text
C08 request_id             0x1112131415161718
C08 native request_id      0x1112131415161722
C09 request_id             0x2122232425262728
lease_id                   00112233445566778899aabbccddeeff
worker_incarnation_id      102132435465768798a9bacbdcedfe0f
reservation_id             202122232425262728292a2b2c2d2e2f
buffer_id                  303132333435363738393a3b3c3d3e3f
resource amount            134,217,728 bytes
native scratch amount      8,388,608 bytes
reservation TTL            30,000 ms
buffer fixture size        134,217,728 bytes
default buffer TTL         300,000 ms
PBMUX sequence             0 except sequential fragmented fixtures
```

The exact 4 MiB PUT body uses:

```text
block = SHA256(u64_be(i / 32))
body[i] = block[i mod 32]
```

`c08_c09_wire_v0_1_vectors_001/README.md` records every oracle and byte digest.
`scripts/check_c08_c09_wire_addendum_001.py` generates and checks bytes with
manual standard-library parsing and does not import runtime encoders.

## 15. Lock status

**LOCKED FOR GENERAL POC IMPLEMENTATION — C08/C09 WIRE V0.1 — 001**
