# PhoneBoost C12 Auto-Use BLAKE3 Profile V0.1 — Locked 2026-09-01

Status: **LOCKED FOR IMPLEMENTATION**  
Frozen: **2026-09-01 Europe/Paris**

## 1. Authority, scope, and non-supersession

Authority remains, in descending order:

1. SPEC V0.7
2. Contract Set V1.3
3. Tech Sheet V1.3
4. Pseudocode V1.1
5. Fixture Generation Spec V1.0
6. applicable locked wire addenda

This profile is subordinate to that chain. It closes only the minimum local
C12/API-1 production profile needed to observe auto-use BLAKE3 readiness and
invoke the existing `AutoUseController::execute_blake3` path. It does not
create a new transport, trust authority, worker provider, or test endpoint.

This profile does not alter any C07, C08, C09, or C10 wire layout, registry,
message, lifecycle, authority, validation order, or failure scope. It also does
not alter C05, C06, PBMUX, ResourceGuard, Android, or JNI behavior.

## 2. Terminal synchronous `compute.submit`

For this profile, C12 `compute.submit` is a synchronous terminal request. It
returns one terminal C12 success or error envelope on the same authenticated
local connection. It creates no C12 job identifier and has no C12 polling
phase. The existing bounded C12 framing, connection idle timeout, and the
existing bounded C07-C10 auto-use operation timeouts remain in force.

`jobs.get` and `compute.cancel` remain deferred for API 1. This profile does
not reinterpret the asynchronous C10 worker job machinery: that machinery is
an internal part of the already-existing auto-use execution path.

## 3. Exact request schema

The request envelope remains C12 API 1. `params` MUST be a JSON object with
exactly these two members and values:

```json
{
  "op_id": "pb.native.blake3/1",
  "fixture": "c10-abc-v1"
}
```

Both members are required strings. Missing members, extra members, mistyped
members, non-object `params`, unknown operation identifiers, and unknown
fixture identifiers are rejected without execution as:

```json
{
  "api": 1,
  "id": "<echoed-request-id>",
  "ok": false,
  "error": {
    "code": "LOCAL_BAD_REQUEST",
    "scope": "REQUEST",
    "message_safe": "invalid compute.submit params"
  }
}
```

If the production daemon has no auto-use controller in its immutable local API
context, an otherwise valid request is rejected without execution as:

```json
{
  "api": 1,
  "id": "<echoed-request-id>",
  "ok": false,
  "error": {
    "code": "LOCAL_UNSUPPORTED_METHOD",
    "scope": "REQUEST",
    "message_safe": "method unavailable in current build"
  }
}
```

`<echoed-request-id>` denotes the unchanged opaque JSON `id` from the request;
it is not a literal wire value.

## 4. Daemon-owned fixture registry

The only fixture in this profile is immutable and daemon-owned:

| Identifier | Exact bytes | Size | Expected BLAKE3 digest (lowercase hex) |
|---|---|---:|---|
| `c10-abc-v1` | ASCII/UTF-8 bytes `61 62 63` (`abc`) | 3 | `6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85` |

No path, URI, inline byte string, base64 value, arbitrary fixture, raw payload,
or caller-selected size is accepted. The handler passes exactly the three
bytes `abc` once to `AutoUseController::execute_blake3`.

## 5. Exact terminal success profiles

Every completed execution, whether remote or local fallback, uses `ok: true`
and this exact seven-member result object:

```json
{
  "api": 1,
  "id": "<echoed-request-id>",
  "ok": true,
  "result": {
    "state": "COMPLETED",
    "op_id": "pb.native.blake3/1",
    "fixture": "c10-abc-v1",
    "input_bytes": 3,
    "digest_blake3_hex": "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85",
    "execution_source": "REMOTE_SUCCESS",
    "auto_use_reason": "READY"
  }
}
```

For an explicit local fallback because remote execution is unavailable, only
the following two values vary:

```json
{
  "execution_source": "LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE",
  "auto_use_reason": "<exact AutoUseReason string>"
}
```

For an explicit local fallback after an ambiguous remote result, only the
following two values vary:

```json
{
  "execution_source": "LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE",
  "auto_use_reason": "<exact AutoUseReason string>"
}
```

A local fallback is a completed digest operation but MUST NOT be reported as
remote success. No result other than a proven remote result may use
`REMOTE_SUCCESS`.

## 6. Closed strings

`execution_source` is exactly one of:

- `REMOTE_SUCCESS`
- `LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE`
- `LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE`

`auto_use_state` is exactly one of:

- `OFF`
- `DISCOVERING`
- `CONNECTING`
- `AUTHENTICATING`
- `ACQUIRING_AUTHORITY`
- `CHECKING_READINESS`
- `AVAILABLE`
- `DEGRADED`
- `RECONNECTING`
- `UNAVAILABLE`

`auto_use_reason` is exactly one of:

- `OFF`
- `NO_DEVICE`
- `NOT_PAIRED`
- `AUTH_FAILED`
- `LEASE_UNAVAILABLE`
- `WORKER_UNHEALTHY`
- `RESOURCE_REFUSED`
- `TRANSPORT_LOST`
- `RECONNECTING`
- `DISCOVERY_BACKEND_UNAVAILABLE`
- `READY`

Case, whitespace, spelling, and underscores are significant. No alias is
defined.

## 7. Additive `system.status` fields

The C12/API-1 `system.status` result adds exactly these fields:

```json
{
  "auto_use_state": "AVAILABLE",
  "auto_use_reason": "READY",
  "remote_blake3_available": true
}
```

The invariant is:

```text
remote_blake3_available ==
    (auto_use_state == "AVAILABLE" && auto_use_reason == "READY")
```

When the daemon has no auto-use controller, the exact values are `OFF`, `OFF`,
and `false`. A client MUST fail closed for readiness proof if any of the three
fields is absent, mistyped, unknown, or inconsistent with the invariant.

## 8. Production CLI

The only compute syntax is:

```text
phoneboostctl compute blake3 c10-abc-v1
```

Its exact successful/fallback output is:

```text
BLAKE3 fixture: c10-abc-v1
Input bytes: 3
BLAKE3 digest: 6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85
Execution source: <exact execution_source>
Auto-use reason: <exact auto_use_reason>
```

Exit codes are:

| Code | Meaning |
|---:|---|
| 0 | `REMOTE_SUCCESS` only |
| 3 | either explicit local fallback source |
| 2 | CLI syntax error, including any other fixture or operation spelling |
| 1 | C12, I/O, unavailable-runtime, or malformed-local-response error |

`phoneboostctl status` prints the three readiness observations and MUST NOT
treat a legacy response lacking them as remote-ready.

## 9. Ownership and sharing

The daemon owns one immutable `LocalApiContext` containing
`Option<Arc<AutoUseController>>` and shares that context with every C12 client
handler. There is no new global mutable state and no external mutex around the
controller. The controller's existing internal synchronization and authority
remain exclusive.

For a valid request with a controller, the C12 handler invokes
`AutoUseController::execute_blake3` exactly once. It neither preflights through
a second compute call nor retries at C12 level.

## 10. Security limits and fail-closed rules

- API remains exactly `1`.
- Existing C12 authentication, peer credential admission, NDJSON framing,
  64-KiB line bound, response bound, and connection lifecycle are unchanged.
- Validation is exact and occurs before controller execution.
- Rejection of one request does not relax validation or corrupt the continuing
  authenticated C12 connection.
- Only the enumerated three-byte daemon fixture is addressable.
- No filesystem path, user bytes, network endpoint, trust input, lease input,
  resource parameter, provider parameter, or wire field is caller-controlled.
- Digest, source, and reason come from the one real auto-use execution result;
  fallback is never relabeled as remote success.
- Unknown response states, reasons, sources, fields in the exact compute result,
  malformed envelopes, and missing readiness fields fail closed in the CLI.
- No new dependency is authorized by this profile.

## 11. Explicitly deferred features

The following remain deferred and unauthorized by this profile: arbitrary
input bytes; arbitrary paths; more fixtures; more providers; asynchronous C12
jobs; C12 job identifiers; `jobs.get`; `compute.cancel`; progress streaming;
batching; concurrent compute submission policy changes; dispatch outside the
existing auto-use controller; manual endpoints; test harness endpoints; and
all C07/C08/C09/C10 wire changes.
