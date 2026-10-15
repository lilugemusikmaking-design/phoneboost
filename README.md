# PhoneBoost

PhoneBoost is a non-production proof of concept for explicit remote compute and
capacity cooperation between a Linux x86-64 host and an Android ARM64 worker.
Workloads opt in to typed remote services; Android keeps final authority over
its own resources.

PhoneBoost is **not** transparent RAM or swap extension, a CPU illusion, or
magical hardware pooling. In particular, a `RemoteBuffer` is a volatile remote
object, not Linux-addressable memory.

## Architecture

- First pairing uses Noise XX, a six-digit SAS confirmed by the user at both
  endpoints, and authenticated PBMUX `CONTROL/8 PAIR_CONFIRM` messages.
- Reconnect uses Noise IK against the pinned static key. A peer ID is the full
  256-bit `SHA-256(static_public_key)`, not the key and not a truncated integer.
- PBMUX provides authenticated channel framing, sequence validation, quotas,
  and fail-closed dispatch.
- A single controller lease is required for remote mutation. The Android
  `ResourceGuard` is the final admission authority; Linux observations are
  estimates, never grants.
- Volatile remote state is invalidated on session or worker-incarnation loss.
  Missing authority, stale telemetry, malformed frames, and unsafe local
  runtime state fail closed.

## Implementation status

### Implemented

- Linux user-mode daemon startup, private Unix control socket, peer-credential
  admission, bounded C12 framing, `system.status`, and pairing actions.
- Local-IP transport and explicit reconnect behavior.
- Noise XX/SAS pairing, mutual commit, Noise IK reconnect, encrypted record
  handling, and PBMUX pairing/committed dispatch.
- Locked PBMUX codecs and fixtures, including C07 `COMMAND`, `COMMAND_ACK`, and
  `METRICS/1 HEARTBEAT` parsing.
- Android foreground worker, JNI boundary, local health sampling, canonical
  worker incarnation, controller-lease logic, and `ResourceGuard` logic.
- Authenticated C07 lease mutation, C08 `ResourceGuard` admission, bounded C09
  remote objects, and the locked C10 `pb.native.blake3/1` provider path.
- C12 auto-use readiness and the exact `c10-abc-v1` BLAKE3 action through
  `phoneboostctl`, including explicit local fallback sources.
- A separate `phoneboost-web-bridge` presentation adapter. It serves the
  production frontend from literal IPv4 loopback, calls only typed `pb-cli`
  operations, and exposes status, durable auto-use control, the locked BLAKE3
  demonstration and the approved bounded real-file BLAKE3 input.

### Physically recorded

- The production C07-C12 remote-compute closure is recorded at
  `docs/evidence/c07-c12-p0-remote-compute-physical.txt`: remote success,
  deliberate disconnect with explicit local fallback and no false remote
  success, authenticated recovery, and a second remote success.
- The bounded P1 local browser proof is recorded at
  `docs/evidence/p1-live-bridge-local-browser-physical.txt`: fresh LIVE state,
  remote success, deliberate disconnect with explicit browser fallback and no
  false remote success, then authenticated recovery and a second remote success.
- The bounded P2 passive-observability proof is recorded at
  `docs/evidence/p2-passive-gate-observability-physical.txt`: one production
  transition exposed the exact fresh discovery, controller-lease, and latest
  admission/readiness observations together; later passive reads showed the
  discovery and proof expiring fail-closed.

### Implemented and physically browser-proven

- The P1 browser bridge and frontend truth model have automated coverage and a
  bounded operator-observed browser proof against the production daemon and
  Android worker. P2 adds passive, expiring observations for discovery, the C07
  controller lease, and the latest C08/C09/C10 admission/readiness proof; it does
  not infer them from authenticated state or create remote work from status
  reads. The P2 production transition and expiry behavior are physically
  recorded without claiming a durable ResourceGuard grant.

### Roadmap

- Add providers or fixtures only under separately locked profiles.
- Produce evidence-backed performance and capacity measurements before making
  any such claim.

### Hash a real input through the native client

With the daemon running, redirect a regular file you own into the client:

```sh
phoneboostctl compute blake3 --stdin < input.bin
```

The client passes the already-open descriptor through the approved private C12
import mechanism, waits for readiness, then requests BLAKE3 on the same
connection. Inputs must contain 1 byte to 128 MiB. Pipes, terminals and path
arguments are rejected; there is no caller-selected remote destination.
The daemon validates ownership and descriptor type and makes the placement
decision through the existing authority path.

The result reports the digest and execution source. Exit 0 means proven remote
success; exit 3 means a completed local fallback (the digest is still valid);
exit 1 means failure or an unknown outcome; exit 2 means invalid command syntax.
A missing terminal response is reported as unknown and is never automatically
retried. The client has separate 60-second readiness and response bounds. The
existing `compute blake3 c10-abc-v1` demonstration remains available unchanged.

This provides a typed real-input compute path, not a measured speedup claim.

### Select a real file in the local Control Center

The local live Control Center accepts one explicitly selected file, from 1 byte
to 128 MiB, with a fresh confirmed ON preference. Select the file and choose
**Calculer BLAKE3**. The result shows the digest, byte count and actual execution
source; a local fallback is a valid digest, not remote success or a speedup.
The hosted evidence page has no file picker.

Only file bytes enter the authenticated loopback bridge; no filename or path
is sent as an API parameter. A sealed anonymous memfd temporarily uses up to
128 MiB in addition to the daemon's existing private import copy. The browser
does not encode the file as base64 or persist its result. OFF remains independent
of upload/compute. A missing terminal response means outcome unknown, never an
automatic retry; closing the page does not promise native cancellation.
The approved profile is
`docs/protocol/PHONEBOOST_DESKTOP_FILE_COMPUTE_PROPOSAL_V0_1.md`.

### Native automatic remote use control

With `phoneboostd` running, use `phoneboostctl auto-use status`,
`phoneboostctl auto-use off`, or `phoneboostctl auto-use on`.
The daemon saves this preference across restart. ON permits automatic use but
does not mean the phone is ready. OFF stops automatic remote work; explicit
native compute requests can still complete as truthful local fallback.
`Persistence: ERROR` means live OFF but unconfirmed saved state: restart may
restore ON. A failed command is never replayed automatically; read status to
reconcile. The local browser switch uses this same native control, displays
pending/unknown until confirmed, and never treats browser localStorage as
daemon authority. Without a live bridge it is unavailable, not a simulated OFF.

## Build and test

Rust 1.98 is pinned by `rust-toolchain.toml`:

```sh
export PATH="$HOME/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin:$PATH"
cargo fmt --check
cargo test --workspace
python3 scripts/check_c07_wire_addendum_002.py
```

Build the production frontend, start or reuse the production daemon, and launch
the loopback-only Control Center from the repository root with one command:

```sh
scripts/run_phoneboost_control_center.sh
```

The launcher prints current native status followed by a per-process capability
URL. It leaves a pre-existing daemon running and stops only processes that it
started. After a successful build, use
`scripts/run_phoneboost_control_center.sh --no-build` for a faster relaunch.
The complete interactive P1 operator workflow is
`scripts/prove_p1_live_bridge_local.sh`; it deliberately does not alter Android
networking by itself.

Install the same reviewed launcher, production binaries, frontend bundle, and a
desktop-menu entry for the current Linux user with:

```sh
scripts/install_phoneboost_control_center_user.sh
```

This user-local installation does not configure autostart or a background
service. The menu entry opens a terminal and prints a fresh one-time loopback
URL. Remove only the installed application payload with
`scripts/uninstall_phoneboost_control_center_user.sh`; pairing state and other
user data are preserved. The locked operational contract is
`docs/operations/PHONEBOOST_P3_LOCAL_USER_INSTALLATION_V0_1_LOCKED_20260906.md`.

After installation, an already-ready production daemon and authenticated
Android worker can be checked through the installed payload with:

```sh
scripts/prove_p3_installed_control_center_physical.sh
```

This bounded operator proof does not build, start a daemon, operate Android, or
change networking. Its locked contract is
`docs/operations/PHONEBOOST_P3_INSTALLED_CONTROL_CENTER_PROOF_V0_1_LOCKED_20260907.md`.

Focused core checks:

```sh
cargo test -p pb-types
cargo test -p pb-pbmux
cargo test -p pb-worker-core
cargo test -p pb-runtime-secure
```

The Android scripts run offline against a pinned tooling bundle. Place that
bundle at `.tooling/`, or set `PHONEBOOST_SHARED_TOOLING` to a directory
containing `android-sdk`, `java`, `gradle`, `rustup`, and `cargo`:

```sh
scripts/build_a6_product_core.sh
scripts/build_a5_android.sh
```

See [implementation evidence](docs/competition/IMPLEMENTATION_EVIDENCE.md) and
the [Emergent handoff](docs/competition/EMERGENT_HANDOFF.md) for competition
provenance and UI constraints.
