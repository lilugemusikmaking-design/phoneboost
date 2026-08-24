# PhoneBoost implementation evidence

## Release identity

- Validation date: 2026-08-24
- Validated native baseline: `052471ed3cdbbe66a6c1f7b255f1d70580d91fcc`
  (`Use canonical 256-bit peer IDs in worker core`)
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
- The Android app runs the Rust worker through JNI, samples Android-local
  memory/thermal/battery/power state, and keeps the foreground worker and
  secure-session lifecycle explicit. It does not claim a controller lease or
  remote-control readiness when none exists.

### RECORDED EVIDENCE

- `pb-types`: 2 unit tests passed.
- `pb-pbmux`: 58 unit tests passed.
- `pb-worker-core`: 43 unit tests passed.
- `pb-runtime-secure`: 15 unit tests passed. Socket-based tests required normal
  host socket permissions and passed outside the restricted sandbox.
- Full Rust workspace: 278 tests passed, 0 failed; all doc-test targets passed.
  Two pre-existing, non-failing `unused_mut` warnings remain in PBMUX tests.
- `cargo fmt --check`: passed.
- Locked C07 checker: final `C07_WIRE_CHECK PASS`; all 10 C07 command/ACK
  vectors and 8 heartbeat vectors produced their expected accept/reject
  verdicts, and 5 structural/semantic oracle mutations produced their expected
  verdicts.
- Android ARM64 production core: offline release build passed; fixture
  isolation and forbidden-authority-export scans passed.
- Android debug APK: offline `:app:assembleDebug` passed (36 Gradle tasks: 4
  executed, 32 up-to-date); the embedded JNI library passed fixture isolation.
- Repository physical-device captures record earlier A5 worker, A6
  lease/`ResourceGuard`, C04 transport, and C05/C06 secure-pairing runs under
  `docs/evidence/`. Those files are evidence snapshots, not current live UI.

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

## Limitations and ROADMAP

- **ROADMAP:** the production C05-to-C07 lease authority seam. The worker-core
  `AuthenticatedSession` has no production constructor. No `authenticated=true`
  shortcut or peer-ID-to-authority conversion exists. This is intentional:
  identity is not proof of an authenticated session, and introducing a
  secure-runtime/worker-core crate cycle would be invalid.
- **ROADMAP:** applying authenticated C07 commands to
  `ControllerLeaseManager` and `ResourceGuard`. Current C07 runtime handling
  parses and rejects malformed data but does not perform lease mutation.
- **ROADMAP:** `RemoteBuffer` storage/operations, native compute providers, AI
  provider behavior, and end-to-end capacity gains. Channel/type registries do
  not constitute provider implementations.
- **ROADMAP:** browser control center and its live native bridge. No browser
  should report a device, authentication, lease, admissibility, provider
  readiness, throughput, or capacity gain unless supplied by a genuine runtime
  snapshot.

The native core remained unchanged during this release-documentation pass.
