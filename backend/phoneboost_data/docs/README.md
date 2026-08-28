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

### Partially wired

- C07 command, ACK, and heartbeat frames reach the authenticated runtime and
  are validated, but command frames do not yet mutate Android lease state.
- Lease and `ResourceGuard` actors are implemented and tested, but their
  production authorization bridge from the authenticated Noise session is
  intentionally closed. Possessing a peer ID alone never grants authority.
- The Android diagnostic surface is native and functional; a competition
  browser dashboard is not included.

### Roadmap

- Define and review the no-cycle C05-to-C07 authorization seam, then connect
  authenticated session authority to lease mutation.
- Implement `RemoteBuffer`, bounded compute providers, and their end-to-end
  product paths.
- Add a control-center presentation layer that labels data as live runtime,
  recorded evidence, or roadmap without substituting mocks for live state.

## Build and test

Rust 1.98 is pinned by `rust-toolchain.toml`:

```sh
export PATH="$HOME/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin:$PATH"
cargo fmt --check
cargo test --workspace
python3 scripts/check_c07_wire_addendum_002.py
```

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
