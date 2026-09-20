# PhoneBoost P2 Controller Lease + ResourceGuard coverage

## Baseline and authority

This matrix records repository coverage **before P2 closure changes** at
`d74fd4a30d7b246d6cb3c8b6788b0faa8615a13d` on
`feat/android-durable-background-worker`.

Authority order is the order recorded in `.pipeline/AUTHORITY.json` and
`.pipeline/STATE.json`: SPEC V0.7, Contract Set V1.3, Tech Sheet V1.3,
Pseudocode V1.1, then Fixture Generation Spec V1.0. Those originals are
recorded as `external/not-local`. The local locked wire addenda and fixture
registries below are subordinate executable sources; this closure does not
claim to replace or amend the external originals.

Local P2 sources used:

- `docs/protocol/PHONEBOOST_C07_WIRE_ADDENDUM_V0_1_LOCKED_20260823_2030_003.md`
- `docs/protocol/PHONEBOOST_C08_C09_WIRE_ADDENDUM_V0_1_LOCKED_001.md`
- `docs/protocol/PHONEBOOST_C10_WIRE_ADDENDUM_V0_1_LOCKED_001.md`
- their checked-in registries, fixtures, independent checker scripts, runtime
  implementation, and tests.

`YES` in **Physical** means the stated behavior was observed on the Galaxy A15.
`NO (not required)` means deterministic proof is the appropriate evidence and
forcing a device into thermal, battery, or memory distress would not provide a
safe or materially better closure proof.

## Coverage before changes

| Requirement | Required | Present | Tested | Integration proven | Physical | Implementation and evidence | Gap before closure |
|---|---:|---:|---:|---:|---:|---|---|
| Android WorkerCore is the single writer for controller authority and accepts only a verified peer session | YES | YES | YES | YES | YES | `crates/pb-worker-core/src/lib.rs`; JNI authenticated routing in `android/core-jni/src/lib.rs`; C07 physical evidence below | None |
| Acquire is same-peer idempotent and rejects a competing peer as busy | YES | YES | YES | YES | NO (not required) | `ControllerLeaseManager`; `L-T01` to `L-T03`; production acquire tests | None |
| Lease ID is nonzero/random, bound to peer and worker incarnation | YES | YES | YES | YES | YES (active authority) | `lease.rs`; `L-T02`, `L-T07`, stale peer/lease tests; production remote proof | None |
| TTL is 60,000 ms, renewal resets the full TTL, and expiry has no grace | YES | YES | YES | YES | NO (not required) | C07 locked addendum; `L-T04`, `L-T06`, reconnect/expiry test | None |
| Release follows revoking to free and terminal replay cannot mutate twice | YES | YES | YES | YES | NO (not required) | `lease.rs`; `L-T05`; lost-ACK terminal replay test | None |
| Command sequence, duplicate replay, out-of-order refusal, 256 terminal cache, and 32 pending bound | YES | YES | YES | YES | NO (not required) | `L-T08` to `L-T15`; C07 golden vectors and checker | None |
| Reconnect preserves only canonically live same-peer authority; expiry and wrong authority fail closed | YES | YES | YES | YES | YES | reconnect/expiry test; `session_loss_removes_available_and_same_peer_reconnect_reuses_lease`; physical Wi-Fi loss/reconnect proof | None |
| A new WorkerCore incarnation invalidates stale lease and volatile state | YES | YES | YES | YES | YES (sticky process recovery) | `L-T07`; `a5_t10_full_core_restart_rotates_incarnation`; `restart_uses_new_incarnation_and_never_publishes_stale_available`; durable worker evidence | None |
| ResourceGuard is Android-local authoritative state and the host cannot override it | YES | YES | YES | YES | YES (nominal admission) | `ResourceGuard` in `resource_guard.rs`; WorkerCore/JNI routing; A6 and remote-compute evidence | None |
| Health sampling is 2 s; absent or older-than-6 s telemetry refuses closed | YES | YES | YES | YES | YES (2 s sampling and passive expiry) | `PHY-T01` to `PHY-T05`, `RG-T05`; A6 and passive gate evidence | None |
| Memory policy uses the locked thresholds, hysteresis, safety margin, and 128 MiB POC cap | YES | YES | YES | YES | NO (not required) | `RG-T06`, `RG-T07`, `RG-T12`, `RG-T13` | None |
| Thermal, battery, charging, and power-save inputs throttle/refuse/recover truthfully | YES | YES | YES | YES | NO (not required) | `RG-T07` to `RG-T11`; local Android observation sampler | None |
| Reserve/commit/release/expire accounting is exact; only committed state authorizes the provider | YES | YES | YES | YES | YES (nominal path) | `RG-T01` to `RG-T04`, `RG-T17`; C08/C09 checker; production remote proof | None |
| Reservation TTL is 30 s; lease/session/incarnation mismatch or loss cannot resurrect resources | YES | YES | YES | YES | YES (session loss) | `resource_guard.rs`; authority tests; C08/C09 and C10 semantic vectors; Wi-Fi loss proof | None |
| Resource request idempotence/conflict handling is bounded (1024/lease, five-minute terminal retention) | YES | YES | YES | YES | NO (not required) | `RG-T14` to `RG-T16`; C08/C09 semantic checker | None |
| Concurrent reservations never exceed authoritative capacity | YES | YES | YES | YES | NO (not required) | `RG-T18` 32 x 8 MiB concurrent attempts | None |
| Compute requires verified session, active lease, current incarnation, committed class-2 reservation, and same-session ready buffer | YES | YES | YES | YES | YES | `WorkerCore::apply_compute_request`; `compute.rs`; automatic end-to-end test; C10 checker; production remote proof | None |
| Failure, timeout, loss, refusal, and local fallback never become `REMOTE_SUCCESS` or stale `AVAILABLE` | YES | YES | YES | YES | YES | auto-use adversarial tests; C10 loss/non-resurrection vectors; physical Wi-Fi loss produced `LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE` | None |
| P1 pairing, durable FGS, Activity-independent runtime, screen-off worker, transport, and real compute remain valid | YES | YES | YES | YES | YES | existing P1 physical record plus 5 min, 30 min, and 3600 s screen-off evidence | None; reused because P2 runtime was unchanged |
| Secure transport partial-record test waits for its responder bootstrap observation before injecting failures | YES | PARTIAL | FLAKY | N/A | N/A | `runtime::tests::committed_transport_partial_records_crypto_and_pbmux_fail_closed`; baseline CI failed at the immediate bootstrap snapshot while 10 isolated repetitions passed | **D — test harness synchronization gap** |
| Raw packet capture and forced physical thermal/battery/memory pressure | NO | NO | N/A | N/A | NOT RUN | Existing evidence explicitly limits these claims | Outside required P2 closure; no result inferred |

## Gap decision at the baseline

- **A — already satisfied:** Controller Lease, ResourceGuard, authenticated
  integration, failure truthfulness, and the useful physical lifecycle.
- **B — implemented but under-tested:** none demonstrated after targeted
  reconciliation.
- **C — real implementation gap:** none demonstrated.
- **D — harness/documentation gap:** no consolidated P2 matrix existed; the
  secure transport partial-record test sampled responder state with an
  immediate assertion even though its helper already exposes a bounded wait.
- **E — outside P2:** RemoteBuffer architecture work and P3; raw packet capture;
  deliberately inducing unsafe physical resource pressure.
- **F — uncertain:** the external canonical originals remain unavailable in
  this clone. Their absence is recorded and no conflicting local requirement
  was found; no new semantic constant is introduced by this closure.

## Existing physical proof reused

- `docs/evidence/c07-c12-p0-remote-compute-physical.txt`: authenticated worker,
  active C07 authority, ResourceGuard readiness, real `REMOTE_SUCCESS`,
  deliberate Wi-Fi loss, explicit local fallback with no false remote success,
  authenticated recovery, and a second real `REMOTE_SUCCESS`.
- `docs/evidence/a6-lease-resourceguard-physical.txt`: Android-local health
  sampler observations; it does not claim a live controller sequence.
- `docs/evidence/p2-passive-gate-observability-physical.txt`: bounded passive
  lease/admission observations and fail-closed expiry; it does not claim a raw
  transcript or packet capture.
- `docs/evidence/android-durable-worker-screen-off-1h.txt`: unchanged durable
  worker path after 3600 seconds with the Activity absent.

No additional physical run is needed for this closure because no Controller
Lease, ResourceGuard, Android lifecycle, pairing, transport, or compute runtime
behavior is changed.

## Closure result

The documentation gap is closed by this matrix. The test-harness gap is closed
by waiting, with the existing five-second bound, for the authenticated responder
snapshot and first heartbeat after the Pong is received. The partial-prefix,
partial-payload, crypto-failure, and PBMUX-failure injections and all their
fail-closed assertions remain unchanged. No production runtime path changed.

Targeted closure validation:

- Controller Lease tests: 20/20 PASS.
- ResourceGuard tests: 23/23 PASS.
- Host authority/admission, refusal/recovery, session-loss/reconnect, and new
  incarnation integration tests: 4/4 PASS.
- C07, C08/C09, and C10 independent wire checkers: PASS.
- Secure transport partial-record test: PASS after the harness correction.
- `scripts/project-pipeline check-fast`: PASS.
- `scripts/project-pipeline check-full`: 14/14 commands PASS, including nextest
  359/359 (one profile-declared skip covered by the mandatory cargo-test pass),
  full workspace Rust tests, frontend 331/331, Playwright 10/10, Android,
  security, and Maestro syntax.
