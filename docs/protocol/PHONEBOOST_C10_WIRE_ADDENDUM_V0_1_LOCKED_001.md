# PhoneBoost C10 COMPUTE Wire Addendum V0.1 — Locked 001

Status: **LOCKED FOR GENERAL POC IMPLEMENTATION**

## 1. Authority and scope

Authority remains:

1. SPEC V0.7
2. Contract Set V1.3
3. Tech Sheet V1.3
4. Pseudocode V1.1
5. Fixture Generation Spec V1.0

`PHONEBOOST_C10_REGISTRY_RESOLUTION_V0_1_001.md` and the authorized additive
C08 class-2 erratum close byte-level omissions where those sources are silent.
This addendum defines only PBMUX channel 3 COMPUTE for the General POC. It
changes no C05-C09 message type, PBMUX header, C09 handle, authentication, lease,
or worker-incarnation layout and creates no provider runtime by itself.

## 2. General wire rules

- Payloads are binary; JSON and variable provider strings are forbidden.
- All multi-byte integers are unsigned big-endian.
- A 128-bit opaque ID occupies 16 raw bytes in network order.
- Required `lease_id`, `worker_incarnation_id`, `reservation_id`, `buffer_id`,
  and `job_id` fields are nonzero.
- Presence flags are exactly zero or one. Bytes for an absent field are zero.
- Reserved bytes and PBMUX reserved flag bits are zero.
- No PeerId, VerifiedSessionId, or authentication boolean is on the wire.
  These authorities come only from the committed `VerifiedPeerSession`.
- Every C10 message in V0.1 is fixed-size, unfragmented, and no larger than 88
  payload bytes.
- PBMUX sequence remains per-direction SecureSession order. C10 never uses C07
  `command_seq`.
- Schema identity is PBMUX version 1 plus channel, direction, message type, and
  the exact profile below. There is no inner schema-version byte.

## 3. Registry and direction

PBMUX channel is COMPUTE / 3.

| Type | Name | Host to worker | Worker to host |
|---:|---|---|---|
| 1 | `SUBMIT` | request | invalid |
| 2 | `STATUS` | request | nonterminal status or request failure |
| 3 | `RESULT` | invalid | terminal job/admission result |
| 4 | `CANCEL` | request | terminal cancellation response |

Direction disambiguates STATUS and CANCEL. RESULT may directly complete a
SUBMIT, report later completion correlated to its SUBMIT request ID, or answer
a STATUS request that observes a terminal job. A known type in an invalid
direction is a logical-message `UNSUPPORTED_MESSAGE` failure and never reaches
the compute authority.

## 4. PBMUX profiles and correlation

All host requests use:

```text
flags = START|END|ACK_REQUIRED = 0x0007
request_id = nonzero u64
fragment_index = 0
payload_len = logical_message_len = exact fixed payload size
```

All worker responses use:

```text
flags = START|END = 0x0003
request_id = triggering request_id
fragment_index = 0
payload_len = logical_message_len = exact fixed payload size
```

Fragmented C10 frames, zero request IDs, ACK_REQUIRED on responses, missing
ACK_REQUIRED on requests, or any other flag profile are rejected as logical
messages before compute dispatch.

`request_id` correlates a request/replay; `job_id` identifies a job. They are
never interchangeable. A later asynchronous RESULT uses its originating
SUBMIT request ID. A RESULT produced while answering STATUS uses that STATUS
request ID.

### 4.1 SUBMIT idempotence

The authority key is `(active lease_id, request_id)`. The cached request value
is the exact 84-byte SUBMIT payload.

- Identical replay never creates a second job and returns the existing STATUS,
  RESULT, or cached admission refusal.
- A different SUBMIT payload under the same key returns an absent-job RESULT
  with `REQUEST_ID_CONFLICT` and changes no reservation or job state.
- An admitted job consumes its reservation once; a different request ID cannot
  reuse that consumed reservation.
- Capacity is 256 entries per active lease. Nonterminal entries are never
  evicted. Terminal entries are retained for five minutes or until session or
  lease end, whichever occurs first. A full table returns
  `IDEMPOTENCE_TABLE_FULL` before admission.
- Eviction never makes an admitted SUBMIT executable again because its
  reservation is permanently consumed/consumed-released.

STATUS and CANCEL are request-ID correlated but do not create jobs. Exact
duplicate cancellation replays its prior response. C10 does not derive
authority from request ID.

## 5. Provider, input, and policy registries

### 5.1 Provider registry

| `provider_id` | `provider_version` | Semantic identity |
|---:|---:|---|
| 1 | 1 | `pb.native.blake3/1` |

Both fields are u8. Zero is invalid/absent. Any pair other than 1/1 is a
structurally parseable SUBMIT that returns `UNSUPPORTED_PROVIDER`; it creates
no job and does not consume the reservation.

### 5.2 Input registry

| `input_kind` | Meaning | Status |
|---:|---|---|
| 1 | `REMOTE_BUFFER` | assigned |
| 2 | `INLINE` | deferred/unassigned |

Only input kind 1 is accepted. Values zero and 2 through 255 return
`INVALID_INPUT`. Inline input is not required by the General POC and is
explicitly deferred rather than given an invented variable-body profile.

REMOTE_BUFFER input uses a nonzero buffer ID, u64 offset, and u64 length.
Length is `0..=134217728`; zero hashes the empty byte string while still
requiring an owned READY buffer. Addition `offset + length` must not overflow
u64 and the half-open range must fit the buffer. The buffer must be
nonterminal, READY, and owned by the same authenticated peer, session
generation, active lease, and worker incarnation. Input bytes never travel in
C10.

### 5.3 Fixed provider policy

Caller-controlled timeout and thread fields are absent. Provider policy fixes:

- maximum runtime: 30,000 ms;
- cancellation checkpoint: at least each 1 MiB or 100 ms work quantum;
- worker threads: one;
- extra scratch: at most 8 MiB, backed by C08 class 2;
- digest: exactly 32 bytes;
- deterministic and PURE;
- no filesystem, network, persistence, or dynamic code.

## 6. Job-state registry

| Value | State | Terminal |
|---:|---|---|
| 0 | `INVALID` / absent only | not a job |
| 1 | `ACCEPTED` | no |
| 2 | `RUNNING` | no |
| 3 | `COMPLETED` | yes |
| 4 | `FAILED` | yes |
| 5 | `CANCELLED` | yes |

An accepted SUBMIT creates exactly one job with a worker-generated random
nonzero 128-bit `job_id`. Failed admission creates no job and uses state zero
with `job_present = 0`. No terminal job returns to a nonterminal state and no
partial digest is published.

## 7. Exact payload layouts

### 7.1 COMPUTE/1 SUBMIT request — exactly 84 bytes

| Offset | Size | Field |
|---:|---:|---|
| 0 | 16 | active `lease_id` |
| 16 | 16 | expected `worker_incarnation_id` |
| 32 | 16 | COMMITTED class-2 `reservation_id` |
| 48 | 1 | `provider_id` |
| 49 | 1 | `provider_version` |
| 50 | 1 | `input_kind` |
| 51 | 1 | reserved zero |
| 52 | 16 | REMOTE_BUFFER `buffer_id` |
| 68 | 8 | `input_offset` u64 BE |
| 76 | 8 | `input_length` u64 BE |

All four IDs are nonzero. No timeout, requested scratch size, resource class,
PeerId, session token, or content body is duplicated here. Class and granted
bytes remain authoritative in the C08 reservation record.

SUBMIT may return:

- STATUS with ACCEPTED/RUNNING and a job ID;
- RESULT with COMPLETED for immediate completion;
- RESULT with a terminal admission failure and no job ID.

### 7.2 COMPUTE/2 STATUS request — exactly 48 bytes

| Offset | Size | Field |
|---:|---:|---|
| 0 | 16 | active `lease_id` |
| 16 | 16 | expected `worker_incarnation_id` |
| 32 | 16 | `job_id` |

All IDs are nonzero. A terminal job is answered with RESULT, not a terminal
state inside a successful STATUS response.

### 7.3 COMPUTE/2 STATUS response — exactly 56 bytes

| Offset | Size | Field |
|---:|---:|---|
| 0 | 1 | `job_state` |
| 1 | 1 | `job_present` |
| 2 | 2 | `reason_code` u16 BE |
| 4 | 16 | `lease_id` |
| 20 | 16 | current `worker_incarnation_id` |
| 36 | 16 | `job_id` |
| 52 | 1 | `provider_id` |
| 53 | 1 | `provider_version` |
| 54 | 2 | reserved zero |

A successful response has `job_present = 1`, state ACCEPTED or RUNNING,
reason NONE, a nonzero job ID, and provider 1/1. A request failure has
`job_present = 0`, state INVALID, zero job/provider fields, and one of the
type-specific reasons in section 9.

### 7.4 COMPUTE/3 RESULT — exactly 88 bytes

| Offset | Size | Field |
|---:|---:|---|
| 0 | 1 | `job_state` |
| 1 | 1 | `job_present` |
| 2 | 2 | `reason_code` u16 BE |
| 4 | 1 | `digest_present` |
| 5 | 1 | `provider_id` |
| 6 | 1 | `provider_version` |
| 7 | 1 | reserved zero |
| 8 | 16 | `lease_id` |
| 24 | 16 | current `worker_incarnation_id` |
| 40 | 16 | `job_id` |
| 56 | 32 | `digest` |

Successful `pb.native.blake3/1` completion is exactly:

```text
job_present = 1
job_state = COMPLETED / 3
reason_code = NONE / 0
provider_id/provider_version = 1/1
digest_present = 1
digest = exact 32-byte BLAKE3 digest
```

On failure, cancellation, or absent-job admission refusal,
`digest_present = 0` and all 32 digest bytes are zero. There is no dynamic
digest length. When `job_present = 0`, job state, job ID, and provider fields
are zero. The response still carries a nonzero lease and current incarnation;
they do not establish authority by themselves.

### 7.5 COMPUTE/4 CANCEL request — exactly 48 bytes

The layout is identical to STATUS request: active lease ID, expected worker
incarnation, and target job ID. Cancellation is cooperative and cannot publish
partial output.

### 7.6 COMPUTE/4 CANCEL terminal response — exactly 56 bytes

The layout is identical to STATUS response. Successful cancellation has a
present job, state CANCELLED, reason NONE, nonzero job ID, and provider 1/1.
A new CANCEL that loses a race to an already terminal job returns that actual
terminal state with `JOB_NOT_CANCELLABLE`; it does not rewrite the job. An
absent/unauthorized lookup zeroes job/provider fields.

## 8. C10 local reason registry

| Value | Reason |
|---:|---|
| 0 | `NONE` |
| 1 | `STALE_CONTROLLER_LEASE` |
| 2 | `WRONG_WORKER_INCARNATION` |
| 3 | `UNSUPPORTED_PROVIDER` |
| 4 | `INVALID_INPUT` |
| 5 | `BUFFER_NOT_FOUND` |
| 6 | `BUFFER_NOT_OWNED` |
| 7 | `BUFFER_WRONG_INCARNATION` |
| 8 | `BUFFER_INVALID_STATE` |
| 9 | `BUFFER_LOST` |
| 10 | `BUFFER_FREED` |
| 11 | `BUFFER_EVICTED` |
| 12 | `INPUT_TOO_LARGE` |
| 13 | `RESERVATION_INVALID` |
| 14 | `RESOURCE_EXHAUSTED` |
| 15 | `REQUEST_ID_CONFLICT` |
| 16 | `IDEMPOTENCE_TABLE_FULL` |
| 17 | `JOB_NOT_FOUND` |
| 18 | `JOB_NOT_OWNED` |
| 19 | `JOB_NOT_CANCELLABLE` |
| 20 | `PROVIDER_TIMEOUT` |
| 21 | `PROVIDER_FAILED` |
| 22 | `SESSION_LOST` |
| 23 | `UNSUPPORTED_MESSAGE` |
| 24 | `INTERNAL_ERROR` |
| 25..65535 | unassigned |

An unassigned response reason is a malformed V0.1 logical payload.

## 9. Exact state/reason/presence matrix

### 9.1 Common rules

- `job_present = 0` requires INVALID state, zero job ID, and provider 0/0.
- `job_present = 1` requires a nonzero job ID and provider 1/1.
- NONE is used only by a successful state profile.
- A nonzero reason is required by every failure profile.
- Digest presence follows section 7.4 exactly.

### 9.2 STATUS response

Allowed profiles are:

- present ACCEPTED/NONE;
- present RUNNING/NONE;
- absent INVALID with one of STALE_CONTROLLER_LEASE,
  WRONG_WORKER_INCARNATION, JOB_NOT_FOUND, JOB_NOT_OWNED,
  UNSUPPORTED_MESSAGE, or INTERNAL_ERROR.

Terminal present states are invalid in STATUS; the worker sends RESULT.

### 9.3 RESULT

Allowed profiles are:

- absent INVALID with an admission reason numbered 1 through 16, 23, or 24;
- present COMPLETED/NONE with the digest present;
- present FAILED with BUFFER_INVALID_STATE, BUFFER_LOST, BUFFER_FREED,
  BUFFER_EVICTED, RESOURCE_EXHAUSTED, PROVIDER_TIMEOUT, PROVIDER_FAILED,
  SESSION_LOST, or INTERNAL_ERROR;
- present CANCELLED/NONE for explicit cancellation;
- present CANCELLED/SESSION_LOST when session teardown chooses cancellation as
  the closest worker-side terminal state.

ACCEPTED/RUNNING RESULT, COMPLETED with a nonzero reason, FAILED with NONE,
or any digest outside the completed-success profile is malformed.

### 9.4 CANCEL response

Allowed profiles are:

- present CANCELLED/NONE for success;
- present COMPLETED, FAILED, or CANCELLED with JOB_NOT_CANCELLABLE when a new
  cancellation request observes an already terminal job;
- absent INVALID with STALE_CONTROLLER_LEASE, WRONG_WORKER_INCARNATION,
  JOB_NOT_FOUND, JOB_NOT_OWNED, UNSUPPORTED_MESSAGE, or INTERNAL_ERROR.

An identical replay of the successful original CANCEL replays CANCELLED/NONE.

## 10. Authority and compute reservation semantics

The production admission chain is:

```text
VerifiedPeerSession
-> active C07 controller lease
-> current worker incarnation
-> supported provider 1/1
-> COMMITTED C08 class-2 reservation owned by same peer/lease/incarnation
-> READY C09 buffer owned by same peer/session/lease/incarnation
-> atomically published compute job
```

No authority comes from buffer ID, reservation ID, lease ID, PeerId, request
ID, job ID, host ledger, runtime snapshot, or boolean alone.

One COMMITTED `NATIVE_OP_SCRATCH_BYTES` reservation backs exactly one job.
Successful admission atomically transitions COMMITTED to CONSUMED while the
job becomes visible. Any failure before that publication leaves the reservation
COMMITTED and creates no job. Queue/full-table/resource refusals do not consume
it. A consumed or consumed-released reservation can never authorize another
job. Job terminalization releases its compute-held budget exactly once.

The class-2 granted amount is nonzero and at most 8 MiB for provider 1/1.
RemoteBuffer class-1 held bytes are neither transferred nor double-counted.

## 11. Execution, cancellation, and races

One native job may be RUNNING per worker and at most eight may be queued per
active lease. Accepted jobs preserve submission order for this single worker.

The provider reads through an authoritative immutable execution view acquired
inside the single-writer WorkerCore boundary. If FREE/LOST/EVICT or invalid
state wins before acquisition, admission or the accepted job fails with the
matching typed reason and no digest. Once the immutable view wins, backing
cannot be reused or exposed mutably until the operation completes; this does
not alter C09 wire or persistence semantics.

At 30,000 ms the worker requests cancellation, allows at most 1,000 ms cleanup
grace, discards all partial output, and terminalizes FAILED/PROVIDER_TIMEOUT.
The wire contains no caller timeout. Explicit CANCEL checkpoints follow the
same no-partial-output rule.

## 12. Session and worker loss

Every job internally binds owner PeerId, active lease, worker incarnation,
exact VerifiedSessionId generation, provider, input reference, and class-2
reservation. The session token is never serialized.

If authoritative session loss wins before terminal publication:

1. new admissions stop;
2. the job becomes worker-side FAILED/SESSION_LOST or
   CANCELLED/SESSION_LOST according to the closest safe state;
3. partial digest is discarded;
4. compute budget releases exactly once;
5. no later worker transition may report that job as successful;
6. a fresh session cannot query, resume, or resurrect it.

Because the old transport is gone, the host may receive no terminal RESULT.
Its C14 view therefore remains `UNKNOWN_AFTER_DISCONNECT`, exactly as the
higher-authority pseudocode requires. Worker-side failure is not fabricated
terminal evidence at the host.

A RESULT authoritatively published before loss remains terminal. Cached
terminal replay is bounded by section 4.1 and never crosses a dead session or
ended lease by inference. Worker restart creates a new incarnation and clears
all jobs, C10 idempotence records, and reservations.

## 13. Malformed messages and failure scope

Existing C06 scopes remain authoritative:

- Noise failure, PBMUX sequence error, invalid fixed header, or unknown channel:
  SESSION.
- Known COMPUTE channel with unknown type, invalid direction/flags, any
  fragmentation, wrong fixed length, zero request ID, zero required ID,
  nonzero reserved byte, invalid presence, unassigned response state/reason,
  or invalid state/reason/digest matrix: LOGICAL_MESSAGE.
- A structurally valid SUBMIT with unsupported provider/input, invalid
  authority, buffer, reservation, quota, or idempotence state returns a typed
  C10 RESULT with REQUEST/PROVIDER domain scope.
- Backpressure fails before logical allocation under C06.

Malformed data never creates a job, consumes a reservation, reads a buffer, or
fabricates a result. Existing CONTROL/3 ERROR may report a logical rejection if
the session remains viable.

## 14. Deterministic fixture profile

Test-only constants are:

```text
C10 request_id             0x3132333435363738
lease_id                   00112233445566778899aabbccddeeff
worker_incarnation_id      102132435465768798a9bacbdcedfe0f
compute reservation_id     404142434445464748494a4b4c4d4e4f
buffer_id                  303132333435363738393a3b3c3d3e3f
job_id                     505152535455565758595a5b5c5d5e5f
provider                   1/1 = pb.native.blake3/1
input                      RemoteBuffer offset 0, length 3, contents `abc`
class-2 scratch            8,388,608 bytes
BLAKE3(`abc`)              6437b3ac38465133ffb63b75273a8db5
                            48c558465d79db03fd359c6cd5bd9d85
PBMUX sequence             0 (fixture only)
```

`c10_wire_v0_1_vectors_001/README.md` records every byte digest and semantic
oracle. `scripts/check_c10_wire_addendum_001.py` independently generates and
parses all offsets with Python standard library only. It verifies direction,
flags, reserved-zero, provider/input registries, state/reason/digest matrices,
SUBMIT replay/conflict, single reservation consumption, exactly-once release,
and session-loss non-resurrection.

## 15. Explicitly deferred

- inline input and its fragmentation profile;
- provider values other than 1/1;
- variable output and dynamic digest lengths;
- caller-selected timeout/thread/memory parameters;
- persistent jobs/results or cross-session resume;
- C10 planner, benchmark, UI, JNI, and provider runtime implementation;
- additional COMPUTE message types.

## 16. Lock status

**LOCKED FOR GENERAL POC IMPLEMENTATION**  
**C10 COMPUTE WIRE V0.1 — 001**
