# PhoneBoost implementation evidence

## Release identity

- Validation date: 2026-09-02
- Evidence-record baseline: `b53ea3b84a4085ab45de58385f115f1cbd9176ed`
  (`Record physical P0 remote compute closure`)
- Physical workflow baseline exercised: `290767a19b52fb0713d514641169a14b2a4148d5`
  (`Add P0 remote compute closure workflow`)
- Production path audited at: `162539c2ec3721f1aa45557900988e2a4291202f`
  (`Expose production auto-use BLAKE3`)
- Toolchain: Rust 1.98.0, pinned by `rust-toolchain.toml`
- Scope: non-production Linux x86-64 and Android ARM64 proof of concept

This report uses three provenance labels:

- **LIVE** — real behavior reachable through the current native runtime or
  product path when its required Linux/Android endpoints are running.
- **RECORDED EVIDENCE** — real tests, fixtures, protocol vectors, builds, or
  captured physical-device results; not necessarily visible in a competition
  browser UI.
- **ROADMAP** — not implemented.

## Architecture and current reachability

PhoneBoost is an explicit remote resource/service fabric. It does not turn
phone RAM into Linux RAM, extend swap, or transparently schedule Linux CPU work
on ARM64. Linux orchestrates requests; the Android trusted worker owns remote
truth, and its `ResourceGuard` remains the final admission authority.

### LIVE

- Linux `phoneboostd` validates a private XDG runtime directory, owns a `0600`
  Unix control socket, authenticates local peers by kernel credentials, and
  serves bounded C12 requests. `phoneboostctl status` renders the real
  `system.status` response; unsupported methods return an explicit error.
- A literal local-IP endpoint can carry a real Linux-to-Android connection.
  First pairing uses Noise XX, exact QR-01A SAS derivation, user confirmation,
  PBMUX `CONTROL/8 PAIR_CONFIRM`, and atomic trust commit. Reconnect uses Noise
  IK with pinned-key validation.
- Authenticated PBMUX runtime dispatch validates CONTROL `COMMAND` and
  `COMMAND_ACK` frames and METRICS `HEARTBEAT` frames. Channels and message
  types are checked independently; METRICS/1 is not treated as CONTROL/PING.
- The Android responder passes the unforgeable `VerifiedPeerSession` directly
  through its authenticated JNI handler to the Rust `WorkerCore`. C07 ACQUIRE,
  RENEW, and RELEASE mutate the real `ControllerLeaseManager`; no peer ID or
  boolean can substitute for that proof.
- Authenticated RESOURCE requests reach the Android `ResourceGuard`, which
  validates the active lease, current worker incarnation, Android-local health,
  request idempotence, and budget before RESERVE/COMMIT/RELEASE succeeds.
- Authenticated REMOTE_BUFFER and COMPUTE requests reach the Android
  `RemoteBufferStore` and native `pb.native.blake3/1` provider. C09 objects and
  C10 jobs remain bound to the exact secure session, lease, and incarnation.
- The Linux auto-use controller performs DNS-SD discovery, pinned Noise IK
  reconnect for a committed peer, real C07 acquisition, a real C08/C09/C10
  readiness probe, lease renewal, remote BLAKE3, and deterministic cleanup.
  `phoneboostctl compute blake3 c10-abc-v1` reaches this production path through
  C12 and reports `REMOTE_SUCCESS` only after the remote result and cleanup are
  both terminal.
- The Android app runs the Rust worker through JNI, samples Android-local
  memory/thermal/battery/power state, and keeps the foreground worker and
  secure-session lifecycle explicit. It does not claim a controller lease or
  remote-control readiness when none exists.

### RECORDED EVIDENCE

- `pb-types`: 3 unit tests passed.
- `pb-pbmux`: 65 unit tests passed.
- `pb-worker-core`: 52 unit tests passed.
- `pb-runtime-secure`: 22 unit tests passed. Socket-based tests required normal
  host socket permissions and passed outside the restricted sandbox.
- `pb-host`: 161 library tests and 2 daemon tests passed.
- `pb-cli`: 14 tests passed.
- Android JNI host-side suite: 16 tests passed.
- Full Rust workspace: 352 unit tests and 6 doc-tests passed, 0 failed.
  Two pre-existing, non-failing `unused_mut` warnings remain in PBMUX tests.
- `cargo fmt --check`: passed.
- Strict Clippy for the changed-facing `pb-host` and `pb-cli` targets passed
  with `-D warnings`. A broader advisory run also including unchanged
  `pb-runtime-secure`, `pb-worker-core`, and JNI targets remains non-green on
  five pre-existing style lints in `pb-runtime-secure/src/runtime.rs`; no P0
  authority or runtime behavior is implicated by those lints.
- Locked C07 checker: final `C07_WIRE_CHECK PASS`; all 10 C07 command/ACK
  vectors and 8 heartbeat vectors produced their expected accept/reject
  verdicts, and 5 structural/semantic oracle mutations produced their expected
  verdicts.
- Locked C08/C09 checker: final `C08_C09_WIRE_CHECK PASS`.
- Locked C10 checker: final `C10_WIRE_CHECK PASS`.
- Android ARM64 production core: offline release build passed; fixture
  isolation and forbidden-authority-export scans passed.
- Android debug APK: offline `:app:assembleDebug` passed (36 Gradle tasks
  up-to-date).
- Repository physical-device captures record A5 worker, A6
  lease/`ResourceGuard`, C04 transport, C05/C06 secure pairing, and the completed
  C07-C12 P0 remote-compute closure under `docs/evidence/`. Those files are
  evidence snapshots, not current live UI.

## Implemented components

- Canonical types and full 256-bit peer IDs:
  `SHA-256(static_public_key)`.
- PBMUX header codec, sequencing, quotas, fragmentation/reassembly, pairing
  gate, C07 command/ACK/heartbeat codecs, fixtures, and oracle checker.
- Noise XX/SAS pair lifecycle, durable pinning, IK reconnect, encrypted record
  transport, authenticated runtime routing, and fail-closed session loss.
- Linux startup/local authority, local C12 framing and validation, status and
  pairing endpoints, local-IP transport, and reconnect policy.
- Android foreground service, Rust/JNI panic boundary, worker incarnation,
  Android-local health sampling, controller-lease state machine, and
  single-writer `ResourceGuard` admission logic.

## P0 remote-compute physical closure

The read-only P0 trace at `162539c` found no missing production boundary from
the CLI through C12, auto-use, authenticated PBMUX, Android JNI, C07, C08, C09,
C10, result correlation, or cleanup. The earlier roadmap statements for those
boundaries are superseded by the production implementations and regression
tests listed above.

The bounded production workflow
`scripts/prove_p0_remote_compute_closure_physical.sh` was subsequently completed
against the real Android worker. The operator-observed record is
`docs/evidence/c07-c12-p0-remote-compute-physical.txt`.

The run physically proved authenticated production reconnect, C07 controller
authority and ResourceGuard readiness through the `AVAILABLE / READY` invariant,
the expected BLAKE3 digest with `REMOTE_SUCCESS`, removal of remote availability
after deliberate Android Wi-Fi loss, explicit
`LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE` with
`NO_FALSE_REMOTE_SUCCESS PASS`, authenticated recovery after Wi-Fi restoration,
and a second `REMOTE_SUCCESS` with terminal cleanup.

For the already-paired, durably committed peer, the implementation selects the
locked Noise IK reconnect path. The physical observation proves the production
authenticated reconnect behavior; it is not an independent packet-level capture
of the Noise IK handshake bytes.

No P0 physical blocker remains for the locked `c10-abc-v1` BLAKE3 closure
profile. This evidence does not extend to arbitrary inputs, other providers,
performance or capacity claims, or unrelated product surfaces.

## P1 local browser bridge implementation status

The P1 change adds a separate `phoneboost-web-bridge`
presentation process. It binds literal IPv4 loopback on an OS-selected port,
serves the sanitized production frontend from the fixed `frontend/build` root,
and calls the existing typed `pb-cli` library. Its only native-facing routes are
the current C12-backed snapshot and the locked `c10-abc-v1` BLAKE3 action.

The browser labels a snapshot `LIVE` only while its validated observation is no
more than three seconds old. Failure or expiry removes that label. The three C12
fields for auto-use readiness and the exact execution source are preserved;
fallback is never rendered as remote success. Discovery, controller lease, and
`ResourceGuard` are `UNKNOWN` because C12 does not expose them independently.

The security boundary and limits are frozen in
`docs/competition/PHONEBOOST_LIVE_BRIDGE_SECURITY_MODEL_V0_1.md`. The bounded,
interactive workflow `scripts/prove_p1_live_bridge_local.sh` has been completed
as an operator-observed physical/browser proof against the production daemon and
real Android worker. Its durable record is
`docs/evidence/p1-live-bridge-local-browser-physical.txt`; it is a confirmed
summary, not an automatically captured raw transcript.

Automated P1 validation on 2026-09-05 used Rust 1.98.0. The typed `pb-cli`
suite passed 14/14 tests, the bridge passed 15/15, the full Rust workspace
passed 367 unit tests plus 6 doc-tests, the frontend passed 9/9 tests and its
optimized build completed, and the recorded-evidence backend passed 6/6 tests.
Strict Clippy passed for all changed Rust targets. A non-browser launch check
served that optimized frontend from a literal `127.0.0.1` endpoint with the
expected security headers. The two existing `unused_mut` warnings in PBMUX test
code remain unchanged and non-failing.

The completed P1 proof observed fresh browser LIVE state for the ready production
path, `REMOTE_SUCCESS`, deliberate Android Wi-Fi loss with explicit
`LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE` and `NO_FALSE_REMOTE_SUCCESS PASS`,
then authenticated recovery and a second `REMOTE_SUCCESS`. The current capability
URL was emitted transiently by the proof script for operator use and consumed in
a private browser window. No token or token URL is persisted in repository files,
evidence, documentation, committed logs, or the handoff.

CodeRabbit's final review after the capability-lifetime correction reported no
actionable issues. The proof does not establish LAN access, generic RPC,
arbitrary compute, other providers, throughput, capacity, or C12-unexposed
discovery, controller-lease, or ResourceGuard state. Those browser gates remain
`UNKNOWN` unless independently exposed by C12.
