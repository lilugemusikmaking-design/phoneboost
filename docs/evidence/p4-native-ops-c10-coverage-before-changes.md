# P4 General Compute / native.ops C10 coverage before changes

Baseline: `17817c11c2bc556e17d984dcb52a05b9a8497c2f` on
`feat/p4-native-ops-c10`, created from the synchronized authoritative
`master`/`origin/master` checkpoint.

This matrix records the repository state before any P4 implementation change.
No product, wire, Android, JNI, host, or test code had been modified when it
was written.

## Authority

The authority order recorded in `.pipeline/AUTHORITY.json` remains SPEC V0.7,
Contract Set V1.3, Tech Sheet V1.3, Pseudocode V1.1, then Fixture Generation
Spec V1.0. Those originals are `external/not-local` and are not reconstructed.

Applicable local locked material:

- `docs/protocol/PHONEBOOST_C10_REGISTRY_RESOLUTION_V0_1_001.md`;
- `docs/protocol/PHONEBOOST_C10_WIRE_ADDENDUM_V0_1_LOCKED_001.md`;
- `docs/protocol/PHONEBOOST_C08_C09_WIRE_ADDENDUM_V0_1_LOCKED_001.md`;
- `docs/protocol/PHONEBOOST_C07_WIRE_ADDENDUM_V0_1_LOCKED_20260823_2030_003.md`;
- `docs/protocol/PHONEBOOST_C12_AUTO_USE_BLAKE3_PROFILE_V0_1_LOCKED_20260901.md`;
- checked-in golden vectors and independent checker scripts.

The C10 lock assigns only `REMOTE_BUFFER` input kind 1. `INLINE` kind 2 and
its fragmentation/profile are explicitly deferred and unassigned. The
repository therefore has no authoritative 1 MiB inline profile to implement;
kind 2 must be rejected as `INVALID_INPUT`. The applicable BLAKE3 input limit
is `0..=128 MiB` through a READY RemoteBuffer.

## Actual production call path

1. Authenticated local C12 `compute.submit` validates the exact daemon-owned
   `pb.native.blake3/1` / `c10-abc-v1` request in
   `crates/pb-host/src/local_api.rs` and calls
   `AutoUseController::execute_blake3` once.
2. `crates/pb-host/src/auto_use.rs` requires an AVAILABLE/READY authoritative
   session, reserves and commits C08 class-1 storage, creates/uploads/stats a
   C09 RemoteBuffer, reserves and commits C08 class-2 native scratch, then
   submits C10 provider 1/1. It labels `REMOTE_SUCCESS` only after a validated
   COMPLETED/NONE result with a job and 32-byte digest.
3. `crates/pb-runtime-secure` carries fixed C10 PBMUX frames inside the
   authenticated encrypted session. `crates/pb-pbmux/src/compute.rs` enforces
   exact direction, flags, request correlation, fixed sizes, presence fields,
   provider identity, state/reason matrix, and unfragmented payloads.
4. `android/core-jni/src/lib.rs` routes only an already verified session into
   the single global `WorkerCore` mutex. It creates no native-op bypass or
   alternate authority.
5. `WorkerCore::apply_compute_request` validates current incarnation and the
   active C07 lease, then `apply_compute_submit` validates the C10 request,
   same-session READY RemoteBuffer range, and COMMITTED class-2 reservation.
   It consumes the reservation at publication, executes bounded BLAKE3 through
   the immutable RemoteBuffer view, terminalizes once, and releases compute
   budget once.
6. Session loss and lease end terminalize live jobs and release their budget.
   Worker restart rotates incarnation and discards volatile jobs, reservations,
   and buffers. A fresh session cannot query or resurrect the old job.

## Coverage matrix

The classification describes proof present at the baseline, before P4 closure
work or reruns.

| ID | Requirement | Baseline classification | Implementation and existing proof |
|---|---|---|---|
| A | Admission | IMPLEMENTED + PROVED | `WorkerCore::apply_compute_request` and `apply_compute_submit`; authenticated JNI E-GEN-04/05; C10 checker |
| B | Native-op allowlist | IMPLEMENTED + PROVED | Worker accepts only provider 1/1; E-GEN-04 rejects provider 2/1; locked vector 08 |
| C | Inline input | NOT APPLICABLE | Locked C10 V0.1 explicitly defers kind 2; WorkerCore accepts only `REMOTE_BUFFER_INPUT_KIND` and rejects all other kinds |
| D | RemoteBuffer input | IMPLEMENTED + PROVED | Host C08/C09/C10 path; WorkerCore authoritative view; E-GEN-04 64 MiB execution |
| E | READY requirement | IMPLEMENTED + PROVED | `validate_compute_input`; E-GEN-04 rejects ALLOCATED/not-READY input |
| F | Committed reservation | IMPLEMENTED + PROVED | Exact class-2/COMMITTED snapshot and atomic consume; reservation reuse refusal; C10 semantic oracle |
| G | Lease ownership | IMPLEMENTED + PROVED | C07 validation precedes dispatch; stale/wrong lease refusals; lease-end cleanup tests |
| H | Session ownership | IMPLEMENTED + PROVED | `SessionBinding` on buffer/job/cache; wrong/fresh-session lookups refuse; E-GEN-05/06 |
| I | Worker incarnation | IMPLEMENTED + PROVED | Incarnation checked before lease/job lookup; restart rotates it; E-GEN-04/05 |
| J | Bounds | IMPLEMENTED + PARTIALLY PROVED | Shared 128 MiB constants, checked range addition, 64 MiB real run and >128 MiB refusal are proved; exact 128 MiB execution is bounded by C09 capacity but intentionally not duplicated as another heavy fixture |
| K | Thread/concurrency bound | IMPLEMENTED + PARTIALLY PROVED | JNI serializes through one `Mutex<WorkerCore>` and compute executes synchronously with no thread spawn or unbounded queue; no separate simultaneous-submit stress proof exists |
| L | Authoritative execution | IMPLEMENTED + PROVED | Hash executes inside Android `WorkerCore` after all gates; JNI E-GEN-04 compares the real 64 MiB digest |
| M | Authoritative completion | IMPLEMENTED + PROVED | Only WorkerCore publishes COMPLETED/digest; PBMUX validates terminal matrix; host validates authority fields and job before `REMOTE_SUCCESS` |
| N | Duplicate command behavior | IMPLEMENTED + PROVED | C10 does not use C07 `command_seq` by locked rule; C07 duplicates are separately bounded, while C10 uses `(lease_id, request_id)` |
| O | Replay behavior | IMPLEMENTED + PROVED | Identical SUBMIT replays cached outcome; conflicting payload refuses; reservation cannot authorize a second job |
| P | Disconnect before execution | IMPLEMENTED + PARTIALLY PROVED | Secure framing and E-GEN-05 partial PUT stop before C10 admission; no compute job/result is fabricated |
| Q | Disconnect during execution | IMPLEMENTED + PROVED | Provider checks verified-session liveness at each 1 MiB chunk; JNI stop-during-C10 and host ambiguous-compute tests |
| R | Completion before result receipt | IMPLEMENTED + PROVED | E-GEN-05 closes transport around compute terminal delivery; host observation remains `UNKNOWN_AFTER_DISCONNECT`, never remote success |
| S | Worker/process kill | IMPLEMENTED + PARTIALLY PROVED | Stop/restart rotates incarnation and old jobs cannot be queried; transport-stop-during-C10 covers active loss without claiming a physical process-kill timing |
| T | Provider/executor failure | IMPLEMENTED + PROVED | Bounded provider returns typed timeout/session loss and never a digest; malformed failure-with-digest is rejected |
| U | Timeout | IMPLEMENTED + PROVED | Fixed 30,000 ms provider policy and 1 MiB checkpoints; unit timeout proof; host timeout is ambiguous fallback |
| V | Cleanup | IMPLEMENTED + PROVED | Host cleanup ledger plus WorkerCore session/lease terminalization; ambiguity reconciliation tests |
| W | Reservation release | IMPLEMENTED + PROVED | `scratch_released` exact-once guard; timeout/cancel/session loss and ten-job capacity probes |
| X | No false `REMOTE_SUCCESS` | IMPLEMENTED + PROVED | Host accepts only exact authoritative completion; E-GEN-05 and auto-use loss tests preserve unknown/fallback truth |
| Y | Deterministic result correctness | IMPLEMENTED + PROVED | Empty, one-byte, `abc`, 64 MiB and ten repeated same-buffer digests; source prefix unchanged after compute |
| Z | Host fallback truth | IMPLEMENTED + PROVED | Distinct unavailable and ambiguous fallback sources; C12 exact strings; reconnect then genuine remote success |

## Native operation profile found

Only `pb.native.blake3/1` is registered. It accepts a same-session READY
RemoteBuffer range of zero through 128 MiB and returns exactly 32 digest bytes.
It is deterministic and pure, performs no filesystem/network/dynamic-code
dispatch, uses a COMMITTED C08 `NATIVE_OP_SCRATCH_BYTES` reservation of at most
8 MiB, checks time/session liveness per 1 MiB chunk, and runs within the single
JNI-owned WorkerCore critical section. C10 inline input, other providers,
caller-selected timeouts/threads, persistence, and cross-session resume remain
deferred.

## Authority and failure truth

The effective chain is:

`VerifiedPeerSession -> current incarnation -> active C07 lease -> provider
1/1 -> COMMITTED class-2 reservation -> same-session READY C09 range ->
WorkerCore publication/execution -> exact terminal C10 RESULT -> host-validated
REMOTE_SUCCESS`.

C10 explicitly does not carry or consume C07 `command_seq`; sequence validity
belongs to acquisition/renewal/release of the lease. C10 itself uses PBMUX
session sequence plus `(lease_id, request_id)` idempotence. Treating
`command_seq` as a new C10 field would contradict the locked addendum.

When transport outcome is ambiguous, the secure initiator reports
`UnknownAfterDisconnect` and auto-use may recompute the pure BLAKE3 locally,
but reports `LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE`. It never promotes that
local completion to `REMOTE_SUCCESS`. Worker-side session loss independently
prevents later success publication or resurrection.

## Root-gap classification before changes

- **PRODUCT GAP:** none demonstrated.
- **TEST GAP:** no completion-blocking gap demonstrated. Exact 128 MiB execution
  and simultaneous-submit stress are intentionally not duplicated because the
  locked bounds and single-writer path are already directly enforced; the
  existing 64 MiB authenticated proof is retained.
- **HARNESS GAP:** none demonstrated.
- **DOCUMENTATION/EVIDENCE GAP:** no consolidated P4 architecture, A-Z matrix,
  root-gap decision, validation record, or final resume state existed.
- **ENVIRONMENT GAP:** none demonstrated.
- **NO GAP:** production C10/native.ops behavior is already present and
  conformant with the locally available locked authority.

The minimal P4 closure is therefore evidence and pipeline state only unless
targeted deterministic validation exposes a real defect. No production or test
change is justified solely to create activity for this phase.

## Physical-test decision before validation

The baseline already contains Galaxy A15 proof of authenticated authority,
ResourceGuard admission, real BLAKE3 `REMOTE_SUCCESS`, controlled loss with
truthful local fallback, reconnect, second remote success, and one-hour durable
screen-off execution. This P4 closure changes no runtime behavior. A new
physical run would not prove a new behavior and is therefore planned as
`NOT RUN`; prior evidence remains cited only for the facts it actually records.

## Targeted reconciliation result

No production, transport, Android, JNI, host, or test implementation change
was justified. The following existing proofs were rerun against the baseline:

- independent C10 checker: PASS, all 17 vectors plus registry, manifest,
  offsets, idempotence, exact-once release, non-resurrection, and negative
  matrices;
- PBMUX C10 tests: PASS, 2/2;
- WorkerCore compute tests: PASS, 3/3;
- authenticated JNI E-GEN-04: PASS, one real 64 MiB BLAKE3 plus nine repeated
  jobs over the same READY RemoteBuffer, exact digests, unchanged input prefix,
  typed refusals, replay/conflict behavior, and budget recovery;
- authenticated JNI E-GEN-05: PASS, nine fixed-seed loss/reconnect scenarios
  including three compute terminal-delivery losses, with
  `UNKNOWN_AFTER_DISCONNECT` and no stale success;
- JNI secure transport stop during C09/C10: PASS, session revoked before a
  blocked compute resumes;
- host automatic IK authority/readiness/renewal/remote BLAKE3: PASS;
- host ambiguous remote compute: PASS, exact local recomputation reported as
  `LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE`, followed by genuine recovery;
- C12 local API suite: PASS, 16/16, including exact closed schema, one compute
  invocation, readiness invariant, source strings, and connection continuity.

The root-gap decision remains `DOCUMENTATION/EVIDENCE GAP` only. This file is
the minimal closure artifact; final pipeline and CI outcomes are recorded in
`.pipeline/STATE.json` and its resume data.

## Final local validation

- `scripts/project-pipeline check-fast`: PASS.
- `scripts/project-pipeline check-full`: PASS, 14/14 commands.
- Rust nextest: PASS, 360/360 with the profile-declared heavy host proof
  covered by the mandatory workspace test.
- Mandatory Rust workspace: PASS, including host 166/166, PBMUX 65/65,
  WorkerCore 53/53 and JNI 16/16.
- Frontend: PASS, 331/331 tests and production build.
- Playwright: PASS, 10/10.
- Android: PASS, release Rust JNI build, debug APK, controller state 5/5,
  pairing SAS 4/4 and lint.
- Security: PASS, cargo-deny policy, Trivy with zero detected lockfile
  vulnerabilities, and checkpoint secret scan.
- Maestro: PASS for syntax only; physical execution was intentionally NOT RUN
  for this evidence-only closure.

## GitHub CI

GitHub Actions run `35643969840` for checkpoint
`bc6cb01d39c6bc336a0e916cc40cec057d353a9d` completed PASS with 7/7 jobs:
deterministic, Maestro structure, Rust, frontend, security/dependency, Android,
and Playwright. GitHub's Node 20 deprecation annotations did not fail a job and
do not change the P4 result.
