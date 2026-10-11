# P6 C11 authenticated runtime CAPACITY wiring

Date: 2026-10-11

Branch: `feat/p6-c11-runtime-capacity`

Baseline: `0fcd23140123257ae84d8ab7ca8477057abc9a2c`

## Bounded scope

This increment connects the existing C11 decision core to the one existing
PhoneBoost-aware C12 BLAKE3 auto-use path. It deliberately selects
`PlacementMode::Capacity`: that mode preserves the established remote-capacity
behavior and does not invent C13 performance evidence.

The C12 request and response schema is unchanged. No performance-profile
producer, persistence format, benchmark result, SPEED mode, or BALANCED-mode
product default is added. C13 persistence remains a separate increment.

## Runtime authority path

1. Android `WorkerCore` builds a redacted PBMUX `METRICS/1` heartbeat only when
   local C08 health is fresh and the authenticated peer owns the active C07
   controller lease.
2. The secure responder overwrites the payload peer identifier with its own
   authenticated local identity before encrypting and sending the frame. A
   downstream handler therefore cannot forge the sender binding.
3. The Linux initiator retains the latest heartbeat only when its peer ID
   matches the authenticated session. Heartbeat state is cleared on session
   start, cancellation, and end.
4. The host accepts metrics for placement only when they are at most six
   seconds old and match the active peer, lease ID, and worker incarnation.
5. The C11 CAPACITY decision checks authentication, AVAILABLE state, live
   lease, fresh bound metrics, provider availability, a non-zero resource
   estimate, C08 safe budget, thermal state, battery policy, and remaining
   capacity before any remote resource request is dispatched.
6. Any failed gate produces the existing truthful local fallback. The actual
   C08 reserve/commit path remains authoritative after planning, so a change
   between observation and execution still fails closed.

## Compatibility and fail-closed properties

- Existing PBMUX heartbeat layout is reused unchanged.
- `provider_count` and `transport_count` remain zero as required by V0.1.
- Unknown or invalid thermal state never becomes nominal.
- Missing, stale, peer-mismatched, lease-mismatched, or incarnation-mismatched
  metrics cannot authorize remote work.
- Severe thermal state, disallowed battery policy, zero safe budget, and
  insufficient remaining budget select local execution.
- The existing C12 `compute.submit` fixed schema and public source/reason
  reporting remain unchanged.

## Executed evidence before checkpoint

- `cargo check --locked -p pb-runtime-secure -p pb-worker-core -p pb-host -p phoneboost-core-jni`: PASS.
- `cargo test --locked -p pb-runtime-secure -p pb-worker-core --lib`: PASS, 22/22 and 53/53 before the added initiator binding test.
- Initiator heartbeat peer binding and session clearing test: PASS, 1/1.
- C11 deterministic planner tests: PASS, 14/14.
- Runtime CAPACITY fresh/bound/safety decision test: PASS, 1/1.
- Authenticated IK/lease/readiness/heartbeat/remote-BLAKE3 end-to-end test: PASS, 1/1.
- Full `pb-host` library suite: PASS, 181/181, including the 64 MiB remote proof.
- Native `check-fast`: PASS, including 12/12 pipeline tests, Gitleaks synthetic-secret rejection, actionlint, formatting, and diff checks.

The full repository pipeline and exact-head GitHub CI results are intentionally
recorded only after they execute.
