# P3 RemoteBuffer coverage before changes

Baseline: `0583c0aaf639b51c0d4da939bcbebcf2117f8d93` on
`feat/android-durable-background-worker`.

## Authority

The original authority order remains recorded in `.pipeline/AUTHORITY.json` as
SPEC V0.7, Contract Set V1.3, Tech Sheet V1.3, Pseudocode V1.1, then Fixture
Generation Spec V1.0. Those originals are external/not-local. The applicable
local locked protocol source is
`docs/protocol/PHONEBOOST_C08_C09_WIRE_ADDENDUM_V0_1_LOCKED_001.md`, with the
project-owner registry correction in
`docs/protocol/PHONEBOOST_C08_C09_REGISTRY_RESOLUTION_V0_1_001.md`.

## Coverage matrix

| Requirement | Existing implementation | Existing proof before changes | Baseline result |
|---|---|---|---|
| Authenticated authority | `WorkerCore::apply_remote_buffer_request` requires a `VerifiedPeerSession`, current incarnation and active C07 lease | authenticated JNI C08/C09 end-to-end test | PASS |
| ALLOC | exact committed class-1 C08 reservation; staged backing and ID before one publication point | worker-core lifecycle and reservation-consumption tests; JNI end-to-end | PASS |
| PUT | nonempty bounded range, checked addition, staged initialized-range merge, READY only after complete coverage | worker-core lifecycle; PBMUX malformed-length and 4 MiB fixture | PASS |
| GET | READY-only, checked nonempty range and DATA profile | worker-core lifecycle; authenticated JNI end-to-end | PASS |
| FREE | terminal FREED tombstone, backing removed, budget released exactly once; repeat FREE is idempotent while retained | worker-core lifecycle | PASS |
| Quota/capacity | 128 MiB global capacity, at most 8 live buffers per lease, exact reservation size and class | worker-core quota test | PASS |
| TTL/TOUCH | 300 s initial TTL; TOUCH bounded by 1,800 s absolute lifetime; expiry becomes EVICTED | worker-core quota and expiry tests | PASS |
| Session loss | exact-session buffers become LOST, backing is removed and accounting released once | worker-core session-loss test; authenticated JNI loss/reconnect test | PASS |
| Transport loss during C09 | session is revoked before the handler returns and cleanup runs before join | dedicated JNI secure-transport-stop test | PASS |
| Worker restart | volatile store and rotated incarnation; old-incarnation request refused before handle lookup | authenticated JNI restart test | PASS |
| Wire/profile | locked types, lengths, reason/state profiles and exact 4 MiB fragmentation | independent C08/C09 checker and PBMUX tests | PASS |
| Compute integration/truth | host uses C08 reserve/commit, C09 ALLOC/PUT/STAT/FREE and C10; REMOTE_SUCCESS requires an exact completed remote result | existing host/JNI integration and retained physical P2 proof | PASS for software integration; physical proof is reused only for its recorded P2 facts |
| C07 authority disappearance | C09 access is refused after lease loss, but explicit RELEASE only cleaned C10 jobs; consumed C09 backing and class-1 accounting remained until the C09 TTL | code trace in `WorkerCore::apply_controller_command`; no direct regression test | GAP |

## Deterministic baseline commands

- `python3 scripts/check_c08_c09_wire_addendum_001.py`: PASS, all 28 vectors,
  fragmentation, manifest, semantic and negative checks accepted.
- `cargo test --locked -p pb-worker-core remote_buffer::tests -- --nocapture`:
  PASS, 3/3.
- `cargo test --locked -p pb-pbmux remote_buffer::tests -- --nocapture`: PASS,
  3/3.
- `cargo test --locked -p phoneboost-core-jni
  tests::e_gen_03_authenticated_c08_c09_end_to_end_and_loss_restart_fail_closed
  -- --exact --nocapture`: PASS, 1/1.
- `cargo test --locked -p phoneboost-core-jni
  tests::secure_transport_stop_during_active_c09_revokes_and_cleans_before_join
  -- --exact --nocapture`: PASS, 1/1.

## Root gap and minimal closure

RemoteBuffer authority is downstream of C07. When that authority disappeared,
the old handle was inaccessible but its backing and consumed class-1 budget
could retain the global 128 MiB capacity until the 300 s buffer TTL. This is a
resource-lifetime gap: the locked addendum defines policy eviction as a terminal
path that removes backing and releases the consumed reservation exactly once.

The minimal closure is to evict only buffers bound to the ended lease, reuse the
existing EVICTED state and exact-once terminalization, and invoke it beside the
existing C10 lease cleanup on explicit RELEASE and on detected lease expiry.
No wire value, quota, timeout, authority rule or product architecture changes.

## Closure evidence

The store now applies the existing policy-EVICT terminalization to every live
buffer owned by the ended lease. `WorkerCore` invokes this cleanup next to C10
cleanup after a successful C07 RELEASE. It also reconciles a newly expired lease
before local health processing or any C07/C08/C09/C10 request, so the expired
lease ID is not discarded before its dependent objects are cleaned.

Targeted validation after the change:

- complete `pb-worker-core` suite: PASS, 53/53 unit tests and 2/2 compile-fail
  doc tests;
- RemoteBuffer subset: PASS, 4/4, including lease-end exact-once eviction;
- exact C07 TTL test: PASS, including recovery of the expired lease ID;
- authenticated JNI C08/C09 lifecycle: PASS, including RELEASE with a live
  consumed buffer followed by immediate successful 128 MiB class-1 reservation
  under the successor lease;
- secure transport stop during active C09: PASS;
- host automatic IK authority/readiness/renewal/Remote BLAKE3 integration:
  PASS with genuine end-to-end remote completion.

No Android UI, wire profile or physical-runtime behavior changed. A new physical
run would not add evidence for this worker-core accounting transition, so it was
not run. The retained Galaxy A15 P2 evidence continues to prove only its recorded
authenticated remote-compute, loss/fallback and reconnect facts. No new memory
or thermal pressure test was run; the change removes backing earlier and adds no
periodic work beyond existing WorkerCore entry points.
