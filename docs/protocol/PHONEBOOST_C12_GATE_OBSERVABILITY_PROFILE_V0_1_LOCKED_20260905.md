# PhoneBoost C12 Passive Gate Observability Profile V0.1 — Locked 2026-09-05

Status: **LOCKED FOR IMPLEMENTATION**

## Authority and scope

This profile is subordinate to SPEC V0.7, Contract Set V1.3, Tech Sheet V1.3,
Pseudocode V1.1, Fixture Generation Spec V1.0, and applicable locked wire
addenda. It adds three sanitized, passive observations to C12 `system.status`
API 1. It does not alter C07, C08, C09, C10, PBMUX, Android, JNI,
ResourceGuard, or any wire layout.

## Passive/event-driven rule

`system.status`, `phoneboostctl status`, and bridge snapshot reads MUST be pure
local projections. They MUST NOT cause discovery, a C07 acquire/renew/release,
a C08/C09/C10 probe, compute, cleanup, reservation, remote buffer operation,
network scan, wake-up cadence, or any other remote mutation.

Observations are recorded only when the existing production auto-use path has
already performed the corresponding operation. P2 adds no polling loop, timer,
background workload, or remote request.

## Exact additive API-1 schema

```json
{
  "discovery_observation": {"state": "FRESH_HINT", "reason": "C04_CANDIDATE_OBSERVED"},
  "controller_lease": {"state": "ACTIVE", "reason": "C07_ACK_FRESH"},
  "resource_guard_admission_proof": {"state": "FRESH_PASS", "reason": "C08_C09_C10_PROBE_PASSED"}
}
```

Each object has exactly the two string members shown: `state` and `reason`.
Only the following state/reason pairs are valid.

### `discovery_observation`

| State | Reason |
|---|---|
| `FRESH_HINT` | `C04_CANDIDATE_OBSERVED` |
| `NO_HINT` | `C04_NO_CANDIDATE` |
| `BACKEND_UNAVAILABLE` | `DISCOVERY_BACKEND_UNAVAILABLE` |
| `STALE` | `OBSERVATION_EXPIRED` |
| `UNKNOWN` | `EPOCH_INVALIDATED` |
| `UNKNOWN` | `NOT_OBSERVED` |
| `UNKNOWN` | `NOT_EXPOSED_BY_C12` |

`FRESH_HINT` means only that a recent untrusted C04 transport hint was observed.
It does not prove pairing, identity, authentication, authority, endpoint safety,
or worker availability. Records are made only from existing `discover()` calls
and expire after 30 seconds. An authenticated session does not refresh or infer
this observation.

### `controller_lease`

| State | Reason |
|---|---|
| `ACTIVE` | `C07_ACK_FRESH` |
| `EXPIRED` | `ACK_TTL_ELAPSED` |
| `UNAVAILABLE` | `C07_ACQUIRE_FAILED` |
| `UNAVAILABLE` | `C07_RENEW_FAILED` |
| `UNAVAILABLE` | `SESSION_INVALIDATED` |
| `UNAVAILABLE` | `IDENTITY_OR_INCARNATION_CHANGED` |
| `UNAVAILABLE` | `AUTO_USE_DISABLED` |
| `UNKNOWN` | `NOT_OBSERVED` |
| `UNKNOWN` | `NOT_EXPOSED_BY_C12` |

Only existing acquire/renew ACK events create `ACTIVE`. The internal TTL is
never exposed. The local expiry is conservatively bounded as command send time
plus ACK `ttl_remaining_ms`; status reads only compare that bound to monotonic
time and never renew or acquire a lease.

### `resource_guard_admission_proof`

| State | Reason |
|---|---|
| `FRESH_PASS` | `C08_C09_C10_PROBE_PASSED` |
| `FAILED` | `C08_C09_C10_PROBE_FAILED` |
| `STALE` | `PROOF_EXPIRED` |
| `UNKNOWN` | `SESSION_INVALIDATED` |
| `UNKNOWN` | `LEASE_INVALIDATED` |
| `UNKNOWN` | `IDENTITY_OR_INCARNATION_CHANGED` |
| `UNKNOWN` | `AUTO_USE_DISABLED` |
| `UNKNOWN` | `NOT_OBSERVED` |
| `UNKNOWN` | `NOT_EXPOSED_BY_C12` |

This is explicitly the **last admission/readiness proof observation**, not a
durable claim that ResourceGuard currently admits every operation. A result is
recorded only when existing production `readiness_with_purge_proof()` performs
its actual C08 reserve/release and C09/C10 BLAKE3 proof. P2 never synthesizes a
probe. A result is fresh for at most two seconds, then becomes `STALE`.

## Invalidation and compatibility

Positive observations are session/identity/incarnation/lease-bound where those
boundaries exist. They are removed before disconnect, reconnect, disable,
revocation, lease expiry, or identity/incarnation change. Discovery is bound to
the connection epoch because it occurs before a session exists.

P2 producers always emit all three objects. A P2 client receiving an older API
1 daemon that omits an entire object maps it to `UNKNOWN/NOT_EXPOSED_BY_C12`.
A present object with a missing member, extra member, wrong type, unknown
string, or invalid cross-pair is rejected fail-closed. No positive state may be
inferred from authentication, peer identity, auto-use, or remote BLAKE3
availability.

The existing `remote_blake3_available` invariant remains unchanged. P2 does not
alter remote success versus local fallback reporting.
