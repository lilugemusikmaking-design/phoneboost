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

## First full-pipeline diagnostic and correction

The first full run passed Clippy and nextest (376/376), then exposed that the
JNI protocol test client assumed every next encrypted frame was its solicited
response. A valid unsolicited heartbeat could therefore be parsed as a C09
response and fail the test. Production initiator dispatch already handled the
asynchronous metrics channel correctly.

The JNI test client now validates and consumes `METRICS/1` frames while waiting
for its application response, including ambiguous-delivery probes. The complete
`phoneboost-core-jni` suite then passed 16/16, including the 64 MiB repeated
remote proof and randomized disconnect/restart cases.

The same first run also found that this fresh worktree did not point at the
already verified ignored Playwright, Android, security, and pre-commit caches.
The worktree-local ignored tool paths were connected to those existing caches;
no dependency version or product source was changed for that environment fix.

## Final software and CI result

- Full repository pipeline: PASS, 14/14 commands.
- Rust nextest: PASS, 376/376 ordinary tests; the scoped 64 MiB test remained
  excluded from nextest and passed in the mandatory workspace `cargo test`.
- Mandatory Rust workspace tests: PASS, including `phoneboost-core-jni` 16/16
  and `pb-host` 181/181.
- Frontend: PASS, 331/331 tests and production build.
- Playwright: PASS, 10/10 journeys.
- Android: PASS, release JNI build, debug APK, lease-state 5/5, SAS-display 4/4,
  and lint.
- Security: cargo-deny PASS; Trivy reported zero detected vulnerabilities in
  Cargo, Python, and Yarn inputs and the CRITICAL gate passed.
- Maestro structure: PASS for both flows.
- First CI checkpoint `96c001d590bbe71afd2d42e0439a94627aff311f`:
  FAIL only in the Rust job because of the asynchronous JNI test-client issue
  documented above; the other six jobs passed.
- Corrected exact-head CI checkpoint
  `67c00547626ff57d258f0dceeadf122878dea04d`: PASS 7/7 in GitHub Actions,
  run `38126598519`.

## Closure-checkpoint transport correction

The documentation-only closure checkpoint
`c0246adbf0723bb5513ae9fd72e01cf34a017a53` exposed a real load-sensitive
transport defect in exact-head GitHub Actions run `38127226539`. The first
attempt and its single bounded rerun both failed only the 64 MiB host proof:
the remote delivery became ambiguously lost instead of completing. No further
blind rerun was performed.

Two coupled causes were reproduced locally:

1. the authenticated loop left its TCP socket nonblocking while
   `write_all` emitted encrypted records, so normal send-buffer backpressure
   could be misclassified as session loss;
2. after a long fragmented request send, the initiator evaluated the
   ten-second idle-session bound before consuming already queued authenticated
   traffic. A valid response could therefore lose to the idle check.

Encrypted record writes now use bounded blocking I/O and restore nonblocking
polling afterwards. Queued authenticated traffic is consumed before the idle
check. Idle sessions still fail closed after ten seconds; an outstanding
application request uses the already established 35-second request deadline.
A deterministic unit test locks this distinction.

The resilience JNI client also retained a five-second socket read timeout
after authentication. Under measured load, a fragmented 4 MiB PUT could
legitimately exceed that test-only bound. Authentication and PING remain
bounded to five seconds; after the authenticated PONG, application reads use
the same 35-second bound as the production initiator.

Final correction evidence before the new checkpoint:

- targeted 64 MiB host proof: PASS, 1/1;
- secure-runtime suite: PASS, 24/24;
- complete host library suite: PASS, 181/181;
- deterministic randomized JNI disconnect scenario: PASS, 1/1 across all
  nine scheduled failures;
- complete JNI suite: PASS, 16/16;
- native `check-fast`: PASS;
- final repository `check-full`: PASS, all 14 commands, including nextest
  377/377, mandatory workspace Rust tests, frontend 331/331, Playwright 10/10,
  Android build/tests/lint, clean dependency security gates, and Maestro
  syntax.

Exact-head GitHub Actions for this correction is recorded only after the new
checkpoint is pushed and completes.

## Galaxy A15 physical closure

The exact corrected commit was installed and exercised on the authorized
Galaxy A15 (`SM-A155F`, serial `RZ8X500LL3A`). After human-confirmed SAS
pairing, fresh authenticated capacity metrics produced the exact BLAKE3 digest
with `REMOTE_SUCCESS`.

The Android process was then suspended for 6.59 seconds: long enough to cross
the strict six-second capacity-metric freshness bound, but shorter than the
ten-second transport liveness bound. Authentication remained present, the
node remained `AVAILABLE / READY`, and the controller lease remained
`ACTIVE / C07_ACK_FRESH`. The same request returned the exact local digest as
`LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE / WORKER_UNHEALTHY`, never a false
remote success. Resuming the process restored a second genuine
`REMOTE_SUCCESS` without re-pairing.

The sanitized transcript, hashes, timings, cleanup, and scope limits are in
`docs/evidence/p6-c11-runtime-capacity-physical.txt`. No ADB forward or reverse
tunnel was present; ADB was used only to inject and safely recover the bounded
physical pause.
