# PhoneBoost C07 Wire Addendum V0.1 — Lock Candidate 002

Status: **LOCK CANDIDATE**  
Canonical status: **NOT YET CANONICAL**  
Runtime authorization: **NO RUNTIME AUTHORIZATION YET**

## 1. Authority, scope, and non-supersession

The authority order remains:

1. SPEC V0.7
2. Contract Set V1.3
3. Tech Sheet V1.3
4. Pseudocode V1.1
5. Fixture Generation Spec V1.0

This lock candidate neither modifies nor supersedes those documents. It closes
only the POC byte-level schema gap for PBMUX CONTROL/5 `COMMAND`, CONTROL/6
`COMMAND_ACK`, and METRICS/1 `HEARTBEAT`.

It does not authorize C07 runtime implementation. That requires a later,
explicit status change to **LOCKED FOR POC IMPLEMENTATION**.

## 2. General V0.1 wire rules

- Payloads are binary only; JSON is forbidden.
- All multi-byte integers are big-endian.
- Layouts are deterministic and fixed.
- Optional authoritative fields use a one-byte presence flag.
- A presence flag is exactly `0` or `1`; every other value is invalid.
- When presence is `0`, every byte allocated to the absent field is zero.
- All reserved bytes are zero.
- Lengths and fixed fields are validated before allocation or dispatch.
- There is no silent coercion or defaulting.
- PBMUX `sequence` and C07 `command_seq` are independent authorities.
- Existing PBMUX session/logical-message failure scopes remain authoritative.

## 3. CONTROL/5 COMMAND V0.1

### 3.1 Layout

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

Size and offset audit:

```text
1 + 1 + 16 + 8 + 16 + 1 + 1 + 2 = 46
last field end = 44 + 2 = 46
```

### 3.2 Command types and unknown operations

| Value | Operation |
|---:|---|
| 1 | `ACQUIRE` |
| 2 | `RENEW` |
| 3 | `RELEASE` |

Every other `command_type` is a syntactically valid CONTROL/5 envelope carrying
an unsupported operation. The result is canonical `UNSUPPORTED_MESSAGE` with
failure scope `LOGICAL_MESSAGE`. An unknown discriminator is not generic
malformed bytes.

Structural violations remain malformed logical-message rejections, including
wrong size, invalid presence, noncanonical absent bytes, invalid lease rules,
or unexpected provider/payload data.

### 3.3 Common current-operation rules

For ACQUIRE, RENEW, and RELEASE:

```text
provider_present = 0
provider_id = 0
payload_len = 0
```

No variable operation payload is allowed in V0.1. `trace_id` is a u128 BE
observability value; zero is allowed and has no authority.

No requested TTL is transmitted. Lease TTL remains 60 seconds and the
recommended renewal cadence remains 20 seconds.

### 3.4 Lease rules

ACQUIRE:

```text
lease_present = 0
lease_id = 16 zero bytes
command_seq = 0
```

RENEW and RELEASE:

```text
lease_present = 1
lease_id = actual active lease
command_seq = actual C07 lease command sequence
```

Presence, rather than the numerical zero value, carries absence semantics.

## 4. ACQUIRE idempotence

Initial acquisition has no lease and does not participate in a pre-existing
lease command-sequence space.

If the same authenticated peer requests ACQUIRE while its current lease remains
ACTIVE, Android returns the existing current lease facts and creates no second
lease. If another authenticated peer owns ACTIVE, the result is
`CONTROLLER_BUSY`.

A newly created lease initializes:

- `next_command_seq = 0`;
- terminal cache empty;
- pending set empty.

Subsequent RENEW and RELEASE operations use that lease sequence space.

## 5. CONTROL/6 COMMAND_ACK V0.1

### 5.1 Layout

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

Size and offset audit:

```text
1 + 2 + 8 + 1 + 8 + 1 + 16 + 16 + 4 + 8 + 1 + 32 = 98
last field end = 66 + 32 = 98
```

### 5.2 ACK states

| Value | State | Terminal |
|---:|---|---|
| 1 | `ACCEPTED` | no |
| 2 | `COMPLETED` | yes |
| 3 | `FAILED` | yes |

Current ACQUIRE, RENEW, and RELEASE senders emit only terminal `COMPLETED` or
`FAILED`; they must not emit `ACCEPTED`.

An `ACCEPTED` payload may be decoded for schema compatibility only when:

```text
reason_code = NONE
expected_next_seq_present = 0
expected_next_seq = 0
result_ref_present = 0
ControllerLeaseRef bytes = all zero
result_digest_present = 0
result_digest = all zero
```

`ACCEPTED` does not release terminal command credit, enter the terminal C07 ACK
cache, or prove completion.

For `COMPLETED`, `reason_code` is `NONE`. For `FAILED`, `reason_code` is
nonzero.

### 5.3 Local reason-code registry

This registry is local to CONTROL/6 V0.1, not global.

| Value | Reason |
|---:|---|
| 0 | `NONE` |
| 1 | `CONTROLLER_BUSY` |
| 2 | `STALE_CONTROLLER_LEASE` |
| 3 | `OUT_OF_ORDER` |
| 4 | `DUPLICATE_RESULT_EVICTED` |
| 5 | `UNSUPPORTED_MESSAGE` |
| 6..65535 | unassigned |

An unassigned reason code causes rejection as an unsupported V0.1 logical
payload, without interpretation or coercion. If the SecureSession remains
viable, existing CONTROL/3 ERROR machinery may report canonical
`UNSUPPORTED_MESSAGE`.

### 5.4 Expected next sequence

OUT_OF_ORDER is exactly:

```text
ack_state = FAILED
reason_code = 3
expected_next_seq_present = 1
expected_next_seq = exact expected sequence
```

For every other ACK:

```text
expected_next_seq_present = 0
expected_next_seq = 0
```

### 5.5 ControllerLeaseRef V1

When `result_ref_present = 1`, bytes `21..64` encode:

```text
lease_id:               16 bytes
worker_incarnation_id:  16 bytes
ttl_remaining_ms:       u32 BE
next_command_seq:       u64 BE
```

`ttl_remaining_ms` is relative. Android absolute monotonic expiry is never
transmitted because endpoint monotonic clocks are incomparable.

| Outcome | Result ref |
|---|---|
| successful ACQUIRE | present with current lease facts |
| successful RENEW | present with current lease facts |
| successful RELEASE | absent; all ref bytes zero |
| error | absent; all ref bytes zero |

For ACQUIRE, RENEW, and RELEASE V0.1, `result_digest_present = 0` and all 32
digest bytes are zero.

## 6. PBMUX request correlation and exact profiles

### 6.1 Correlation rules

- CONTROL/5 COMMAND uses a fresh nonzero u64 `request_id`.
- CONTROL/6 COMMAND_ACK copies the exact COMMAND `request_id`.
- `request_id` is correlation-only; C07 dedup authority remains exclusively
  `(lease_id, command_seq)`.
- METRICS/1 HEARTBEAT is unsolicited, one-way, has `request_id = 0`, and never
  has `ACK_REQUIRED`.
- PBMUX sequence follows normal per-direction SecureSession sequencing.

### 6.2 COMMAND header

```text
magic = PBM1
version = 1
channel = CONTROL / 0
flags = START|END|ACK_REQUIRED = 0x0007
message_type = 5
header_len = 40
request_id = fresh nonzero
fragment_index = 0
payload_len = 46
logical_message_len = 46
```

### 6.3 COMMAND_ACK header

```text
magic = PBM1
version = 1
channel = CONTROL / 0
flags = START|END = 0x0003
message_type = 6
header_len = 40
request_id = exact COMMAND request_id
fragment_index = 0
payload_len = 98
logical_message_len = 98
```

### 6.4 HEARTBEAT header

```text
magic = PBM1
version = 1
channel = METRICS / 5
flags = START|END = 0x0003
message_type = 1
header_len = 40
request_id = 0
fragment_index = 0
payload_len = 110
logical_message_len = 110
```

All three payloads are unfragmented. Fragmentation is invalid.

## 7. METRICS/1 HEARTBEAT V0.1

### 7.1 Layout

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

Size and offset audit:

```text
32 + 16 + 1 + 16 + 8 + 1 + 1 + 4 + 1 + 1 + 1
+ 8 + 8 + 8 + 2 + 1 + 1 = 110
last field end = 109 + 1 = 110
```

### 7.2 Core fields

- `peer_id` is the raw 32-byte SHA-256 peer identifier.
- `controller_lease_present` is `0` or `1` only.
- An absent controller lease has 16 zero lease bytes.
- `battery_percent` is exactly in `0..100`.
- `charging` and `power_save` are `0` or `1` only.
- Memory and budget fields are u64 BE.
- `queue_depth` is u16 BE.
- `provider_count = 0` and `transport_count = 0` in V0.1.

Provider and transport repeated records remain intentionally deferred. No
numeric provider or transport registry is created here.

### 7.3 Thermal status

HEARTBEAT V0.1 accepts exactly the current Android `PowerManager` values:

| Value | Android status |
|---:|---|
| 0 | `THERMAL_STATUS_NONE` |
| 1 | `THERMAL_STATUS_LIGHT` |
| 2 | `THERMAL_STATUS_MODERATE` |
| 3 | `THERMAL_STATUS_SEVERE` |
| 4 | `THERMAL_STATUS_CRITICAL` |
| 5 | `THERMAL_STATUS_EMERGENCY` |
| 6 | `THERMAL_STATUS_SHUTDOWN` |

A value greater than 6 rejects the HEARTBEAT as an invalid logical message.
The host does not update its latest-health sample. Unknown values are never
interpreted as nominal. Without a subsequent valid heartbeat, existing
telemetry naturally becomes stale under the canonical greater-than-six-second
ResourceGuard rule. No new thermal reason code is introduced.

Official evidence: Android Developers, `android.os.PowerManager` API reference,
thermal status constants 0 through 6:
<https://developer.android.com/reference/android/os/PowerManager> (reviewed
2026-08-23).

### 7.4 Thermal headroom

If `thermal_headroom_present = 0`, bits are exactly `0x00000000`.

If `thermal_headroom_present = 1`, bits are IEEE-754 binary32 in big-endian bit
order. Accepted values are finite and greater than or equal to `+0.0`.

Rejected/noncanonical values are:

- NaN;
- positive or negative infinity;
- negative finite values;
- negative zero bit pattern `0x80000000`.

A sender canonicalizes `-0.0` to `+0.0` (`0x00000000`) when present.
Unsupported or NaN platform observations are encoded as absent.

## 8. Payload schema identity and compatibility

V0.1 has no inner payload-version byte. Schema identity is the joint tuple:

```text
PBMUX version 1 + channel + message_type + exact payload size/layout
```

A future incompatible layout requires an explicit compatibility decision, a
new addendum or protocol-version decision, and new golden vectors. A different
payload length must never be inferred as a future schema.

## 9. Rejection and unsupported-operation rules

Reject before dispatch for structural or semantic noncanonical data, including:

- wrong exact size or PBMUX profile;
- invalid presence byte;
- nonzero bytes for an absent field;
- nonzero reserved bytes;
- ACQUIRE carrying a lease or nonzero command sequence;
- RENEW/RELEASE without a lease;
- unexpected provider or operation payload;
- unknown `ack_state`;
- unassigned CONTROL/6 reason code as unsupported V0.1 payload;
- invalid ACK state/reason/result combination;
- OUT_OF_ORDER without the exact expected-sequence form;
- invalid heartbeat lease, battery, boolean, thermal, provider, or transport
  field.

An unknown but structurally valid CONTROL/5 `command_type` is not malformed; it
produces `UNSUPPORTED_MESSAGE` at `LOGICAL_MESSAGE` scope.

## 10. Deterministic test-only fixture profile

These constants exist only for documentation/oracle fixtures:

```text
COMMAND request_id       = 0x0102030405060708
COMMAND_ACK request_id   = 0x0102030405060708
HEARTBEAT request_id     = 0x0000000000000000
PBMUX sequence           = 0
fragment_index           = 0
lease_id                 = 00112233445566778899aabbccddeeff
worker_incarnation_id    = 102132435465768798a9bacbdcedfe0f
trace_id                 = 1112131415161718191a1b1c1d1e1f20
peer_id                  = 000102030405060708090a0b0c0d0e0f
                           101112131415161718191a1b1c1d1e1f
```

Fixture `sequence = 0` does not assert that runtime messages always use zero.

The byte-exact full PBMUX plaintext frames, raw payload hex and expected parser
outcomes are recorded in `c07_wire_v0_1_vectors_002/README.md`. Noise
ciphertext is intentionally absent.

## 11. Golden-vector and independent-checker gate

The lock-candidate artifact set contains:

- 18 required full PBMUX plaintext `.bin` frames;
- `c07_wire_v0_1_vectors_002/README.md` with payload/frame hex and expected
  outcome for every vector;
- `c07_wire_v0_1_vectors_002/MANIFEST.sha256`;
- `scripts/check_c07_wire_addendum_002.py`, which manually parses offsets and
  does not import any runtime or `pb-pbmux` encoder/decoder.

The checker must reject a structural length mutation and must independently
prove the semantic oracles for unknown command, missing OUT_OF_ORDER expected
sequence, thermal status 7, and negative-zero headroom.

## 12. Explicitly deferred

This lock candidate does not define:

- provider-state records;
- transport-metric records;
- result-reference formats outside ControllerLeaseRef V1;
- a global numeric reason-code registry;
- RESOURCE, REMOTE_BUFFER, COMPUTE, AI, or WAMR payloads;
- future CONTROL operation payloads.

## 13. Lock status

The previous review ambiguities addressed by this candidate are resolved, but
the document remains **NOT YET CANONICAL** and grants **NO RUNTIME
AUTHORIZATION YET**. Explicit project approval is required before changing its
status to **LOCKED FOR POC IMPLEMENTATION**.

