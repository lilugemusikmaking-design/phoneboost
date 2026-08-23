# PhoneBoost C07 Wire Addendum V0.1 — Proposed 001

Status: **PROPOSED FOR REVIEW**  
Canonical status: **NOT YET CANONICAL**  
Scope: **POC WIRE GAP CLOSURE ONLY**

## 1. Authority and non-supersession

The authority order remains:

1. SPEC V0.7
2. Contract Set V1.3
3. Tech Sheet V1.3
4. Pseudocode V1.1
5. Fixture Generation Spec V1.0

This proposed addendum does not modify or supersede any document in that
authority chain. Its only purpose is to propose the missing byte-level payload
schema for:

- PBMUX CONTROL/5 `COMMAND`;
- PBMUX CONTROL/6 `COMMAND_ACK`;
- PBMUX METRICS/1 `HEARTBEAT`.

It does not authorize implementation until its status is explicitly changed to
**LOCKED FOR POC IMPLEMENTATION**.

## 2. General wire rules

- Payload encoding is binary only. JSON is forbidden.
- All multi-byte integers are big-endian.
- Layouts are deterministic and fixed for V0.1.
- Optional authoritative fields use an explicit presence byte.
- Every presence byte is exactly `0` or `1`; any other value is invalid.
- If an authoritative value is absent, its presence byte is `0` and every byte
  allocated to the encoded field is zero.
- All reserved bytes must be zero.
- Lengths are validated before allocation.
- Silent coercion and defaulting are forbidden.
- PBMUX `sequence` remains distinct from C07 `command_seq`.
- The existing PBMUX/1 40-byte header, validation order, limits and failure
  scopes remain authoritative.

## 3. CONTROL/5 COMMAND V0.1

### 3.1 Fixed layout

Payload size is exactly **46 bytes**.

| Offset | Size | Field |
|---:|---:|---|
| 0 | 1 | `command_type` |
| 1 | 1 | `lease_present` |
| 2 | 16 | `lease_id` |
| 18 | 8 | `command_seq` |
| 26 | 16 | `trace_id` |
| 42 | 1 | `provider_present` |
| 43 | 1 | `provider_id` |
| 44 | 2 | `payload_len` |

All multi-byte fields are big-endian.

Independent size recomputation:

```text
1 + 1 + 16 + 8 + 16 + 1 + 1 + 2 = 46 bytes
final offset = 44 + 2 = 46
```

### 3.2 Command type registry

| Value | Operation |
|---:|---|
| 1 | `ACQUIRE` |
| 2 | `RENEW` |
| 3 | `RELEASE` |

Value `0` and values `4..255` are invalid/reserved.

### 3.3 Common V0.1 restrictions

For all three operations:

```text
provider_present = 0
provider_id = 0
payload_len = 0
```

No variable operation payload is allowed in C07 V0.1. Any violation is a
malformed logical message.

`trace_id` is an unsigned 128-bit big-endian value. Zero means “no trace”. It
is observational only and never authoritative.

No `requested_ttl_ms` field exists. Lease TTL remains 60 seconds and the
recommended renewal cadence remains 20 seconds.

### 3.4 ACQUIRE

```text
command_type = 1
lease_present = 0
lease_id = 16 zero bytes
command_seq = 0
```

The presence byte, not the zero value, carries absence semantics.

### 3.5 RENEW and RELEASE

```text
command_type = 2 or 3
lease_present = 1
lease_id = actual active lease identifier
command_seq = actual C07 command sequence
```

## 4. CONTROL/6 COMMAND_ACK V0.1

### 4.1 Fixed layout

Payload size is exactly **98 bytes**.

| Offset | Size | Field |
|---:|---:|---|
| 0 | 1 | `ack_state` |
| 1 | 2 | `reason_code` |
| 3 | 8 | `command_seq` |
| 11 | 1 | `expected_next_seq_present` |
| 12 | 8 | `expected_next_seq` |
| 20 | 1 | `result_ref_present` |
| 21 | 16 | `lease_id` |
| 37 | 16 | `worker_incarnation_id` |
| 53 | 4 | `ttl_remaining_ms` |
| 57 | 8 | `next_command_seq` |
| 65 | 1 | `result_digest_present` |
| 66 | 32 | `result_digest` |

All multi-byte fields are big-endian.

Independent size recomputation:

```text
1 + 2 + 8 + 1 + 8 + 1 + 16 + 16 + 4 + 8 + 1 + 32 = 98 bytes
final offset = 66 + 32 = 98
```

### 4.2 ACK state registry

| Value | State |
|---:|---|
| 1 | `ACCEPTED` |
| 2 | `COMPLETED` |
| 3 | `FAILED` |

Value `0` is invalid.

For current immediate C07 operations, a direct terminal `COMPLETED` ACK is
preferred. An `ACCEPTED` ACK must not be manufactured when no asynchronous
completion follows.

### 4.3 Local reason-code registry

This registry is local to CONTROL/6 V0.1 and is not a global numeric
PhoneBoost reason-code registry.

| Value | Reason |
|---:|---|
| 0 | `NONE` |
| 1 | `CONTROLLER_BUSY` |
| 2 | `STALE_CONTROLLER_LEASE` |
| 3 | `OUT_OF_ORDER` |
| 4 | `DUPLICATE_RESULT_EVICTED` |
| 5 | `UNSUPPORTED_MESSAGE` |

### 4.4 Expected sequence

`OUT_OF_ORDER` requires:

```text
ack_state = FAILED
reason_code = 3
expected_next_seq_present = 1
expected_next_seq = exact expected sequence
```

For every other outcome:

```text
expected_next_seq_present = 0
expected_next_seq = 0
```

### 4.5 ControllerLeaseRef V1

When `result_ref_present = 1`, bytes `21..64` encode:

```text
lease_id:             16 bytes
worker_incarnation_id: 16 bytes
ttl_remaining_ms:      u32 BE
next_command_seq:       u64 BE
```

`ttl_remaining_ms` is a relative duration. Android absolute monotonic expiry
must not be transmitted because endpoint monotonic clocks are not comparable.

Result rules:

| Outcome | `result_ref_present` | ControllerLeaseRef bytes |
|---|---:|---|
| Successful ACQUIRE | 1 | current lease facts |
| Successful RENEW | 1 | current lease facts |
| Successful RELEASE | 0 | all zero |
| Any error | 0 | all zero |

### 4.6 Result digest

For ACQUIRE, RENEW and RELEASE V0.1:

```text
result_digest_present = 0
result_digest = 32 zero bytes
```

The field is reserved for a separately defined future use.

## 5. ACQUIRE idempotence semantics

Initial acquisition has no lease and does not participate in an existing lease
`command_seq` space. Its wire convention is:

```text
lease_present = 0
lease_id = 0
command_seq = 0
```

If the same authenticated peer requests ACQUIRE while its current lease is
still ACTIVE, the worker returns the existing current lease facts and does not
create another lease.

If another authenticated peer owns the ACTIVE lease, the result is
`CONTROLLER_BUSY`.

Creating a new lease initializes:

- `next_command_seq = 0`;
- an empty terminal cache;
- an empty pending set.

Subsequent RENEW and RELEASE operations use this lease sequence space.

## 6. METRICS/1 HEARTBEAT V0.1

### 6.1 Fixed layout

Payload size is exactly **110 bytes**.

| Offset | Size | Field |
|---:|---:|---|
| 0 | 32 | `peer_id` |
| 32 | 16 | `worker_incarnation_id` |
| 48 | 1 | `controller_lease_present` |
| 49 | 16 | `controller_lease_id` |
| 65 | 8 | `monotonic_ms` |
| 73 | 1 | `thermal_status` |
| 74 | 1 | `thermal_headroom_present` |
| 75 | 4 | `thermal_headroom_f32_bits` |
| 79 | 1 | `battery_percent` |
| 80 | 1 | `charging` |
| 81 | 1 | `power_save` |
| 82 | 8 | `android_available_bytes` |
| 90 | 8 | `safe_remote_budget_bytes` |
| 98 | 8 | `reserved_bytes` |
| 106 | 2 | `queue_depth` |
| 108 | 1 | `provider_count` |
| 109 | 1 | `transport_count` |

All integer fields are big-endian.

Independent size recomputation:

```text
32 + 16 + 1 + 16 + 8 + 1 + 1 + 4 + 1 + 1 + 1
+ 8 + 8 + 8 + 2 + 1 + 1 = 110 bytes
final offset = 109 + 1 = 110
```

### 6.2 Field rules

- `peer_id` is the raw 32-byte SHA-256 peer identifier.
- `controller_lease_present` is exactly `0` or `1`.
- If the controller lease is absent, `controller_lease_id` is all zero.
- `thermal_status` is the raw Android platform status byte. Current known
  values are validated conservatively; an unknown future value must never be
  interpreted as nominal.
- If `thermal_headroom_present = 0`, the four headroom bytes are zero.
- If `thermal_headroom_present = 1`, the four bytes contain IEEE-754 `f32`
  bits in big-endian order and represent a finite non-negative value.
- NaN or unsupported headroom is encoded as absent.
- `battery_percent` is in `0..100`.
- `charging` and `power_save` are each exactly `0` or `1`.
- Memory and budget fields are unsigned 64-bit big-endian values.
- `queue_depth` is an unsigned 16-bit big-endian value.
- For V0.1, `provider_count = 0` and `transport_count = 0`.

No provider-state or transport-metric repeated record is frozen here. Adding
such records requires a later explicit wire version bump or addendum.

## 7. PBMUX framing and bounds

| Message | Exact payload size |
|---|---:|
| CONTROL/5 COMMAND | 46 bytes |
| CONTROL/6 COMMAND_ACK | 98 bytes |
| METRICS/1 HEARTBEAT | 110 bytes |

Each message fits in one PBMUX frame and requires:

```text
flags = START|END
fragment_index = 0
logical_message_len = exact payload size
payload_len = exact payload size
```

Fragmentation is invalid for these V0.1 messages. Every payload remains below
the canonical PBMUX payload maximum of 61,440 bytes.

## 8. Rejection rules

Reject before dispatch if any of the following holds:

- wrong exact payload size;
- invalid presence byte;
- absent optional field with nonzero encoded bytes;
- nonzero reserved field;
- unknown `command_type`;
- unknown `ack_state`;
- ACQUIRE carries a lease;
- ACQUIRE has `command_seq != 0`;
- RENEW or RELEASE has no lease;
- unexpected provider or variable payload in C07 V0.1;
- OUT_OF_ORDER lacks `expected_next_seq`;
- heartbeat lease absent with nonzero lease bytes;
- invalid battery or boolean value;
- heartbeat `provider_count != 0`;
- heartbeat `transport_count != 0`;
- malformed thermal headroom.

Existing canonical PBMUX session/logical-message failure scopes apply. This
addendum does not introduce General reason-code strings such as
`FRAME_MALFORMED` or `ACK_MALFORMED`.

An unknown CONTROL/5 operation maps to canonical `UNSUPPORTED_MESSAGE` where
the existing C06 rules permit.

## 9. Required golden vectors

Before implementation, byte-exact vectors must be generated and independently
checked for at least:

| ID | Case |
|---|---|
| GV-C07-01 | valid ACQUIRE |
| GV-C07-02 | valid RENEW with `command_seq = 0` |
| GV-C07-03 | valid RELEASE |
| GV-C07-04 | malformed ACQUIRE with `lease_present = 1` |
| GV-C07-05 | malformed RENEW with `lease_present = 0` |
| GV-C07-06 | ACQUIRE COMPLETED ACK with ControllerLeaseRef |
| GV-C07-07 | RENEW COMPLETED ACK |
| GV-C07-08 | RELEASE COMPLETED ACK without result ref |
| GV-C07-09 | FAILED/OUT_OF_ORDER ACK with expected sequence |
| GV-C07-10 | malformed OUT_OF_ORDER ACK without expected sequence |
| GV-HB-01 | heartbeat without lease |
| GV-HB-02 | heartbeat with active lease |
| GV-HB-03 | heartbeat with thermal headroom absent |
| GV-HB-04 | heartbeat with thermal headroom present |
| GV-HB-05 | malformed nonzero provider count |
| GV-HB-06 | malformed nonzero transport count |

Each vector must later contain:

- raw payload hex;
- full PBMUX plaintext frame hex;
- expected parser result.

Noise ciphertext vectors are not required by this addendum. Existing frozen
Noise framing fixtures remain authoritative.

## 10. Explicitly not frozen

This proposal does not define:

- provider-state wire records;
- transport-metric wire records;
- result-reference formats outside ControllerLeaseRef V1;
- a global numeric reason-code registry;
- RESOURCE channel payloads;
- REMOTE_BUFFER payloads;
- COMPUTE payloads;
- AI/WAMR payloads;
- future CONTROL operation payloads.

These require separate decisions or addenda.

## 11. Internal consistency validation

### 11.1 Size and offset audit

All three stated sizes recompute exactly and all fields are contiguous:

- COMMAND: offsets `0..45`, 46 bytes;
- COMMAND_ACK: offsets `0..97`, 98 bytes;
- HEARTBEAT: offsets `0..109`, 110 bytes.

No overlap or offset hole was found.

### 11.2 Contradiction requiring review

The proposal currently gives two potentially conflicting outcomes for an
unknown CONTROL/5 discriminator:

1. `command_type = 0` or `4..255` is invalid/reserved and unknown
   `command_type` is rejected before dispatch as a malformed logical message.
2. An unknown CONTROL/5 operation maps to canonical `UNSUPPORTED_MESSAGE`
   where existing C06 rules permit.

Because `command_type` is the only V0.1 operation selector, the distinction
between “unknown command type” and “unknown operation” is not byte-level
deterministic. Review must select one exact failure mapping before the document
can be locked.

### 11.3 Items still undefined after this proposal

The following remain undefined or intentionally deferred:

- exact accepted numeric range and rejection/degradation behavior for unknown
  future Android `thermal_status` values;
- validation of unassigned CONTROL/6 `reason_code` values `6..65535`;
- permitted field combinations for an incoming `ACCEPTED` ACK for the three
  immediate V0.1 operations;
- PBMUX `request_id` allocation and COMMAND-to-COMMAND_ACK correlation rules;
- PBMUX `request_id` convention for unsolicited HEARTBEAT messages;
- on-wire negotiation or signaling of the addendum payload version;
- concrete values for PBMUX header fields in the required golden vectors;
- treatment of IEEE-754 negative zero as thermal headroom;
- every item listed in Section 10.

## 12. Compatibility and lock gate

This proposal adds a POC payload schema under the already locked PBMUX/1
framing. It does not change:

- the PBMUX 40-byte header;
- Noise profiles;
- pairing semantics;
- lease TTL;
- ResourceGuard policy;
- A1–A3 fixtures.

Before any implementation, it requires:

1. resolution of the contradiction in Section 11.2;
2. resolution or explicit deferral of the relevant items in Section 11.3;
3. byte-exact golden vector generation;
4. independent parser/checker verification;
5. explicit project approval changing status to **LOCKED FOR POC
   IMPLEMENTATION**.

