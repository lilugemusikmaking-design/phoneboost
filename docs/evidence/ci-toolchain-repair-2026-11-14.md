# CI dependency, Android pin and JNI test repair — 2026-11-14

Scope: dedicated branch `fix/ci-dependency-android-pin-v0-1`, based on
`origin/master` at `1312cb1e7ddc40e71a9512d85663264eb63b30c6`.
PRs #27, #28 and #29 are preserved, open and unmerged. No product fix from those
PRs is imported. This change does not qualify Android health, RAM or multicore.

## Specialized tooling checked first

| Tool | Availability / actual probe | Use in this repair |
| --- | --- | --- |
| GitHub MCP / `gh` | Connector responds; `gh` 2.46.0 installed and authenticated | PR metadata, Actions jobs and first-failure logs via MCP; publication |
| Context7 MCP | Responds | Cargo dependency/update and official Android CLI documentation |
| Other configured MCP | Exa, Node REPL, PhoneBoost browser listed; not exercised | No need for browser automation or broad search for these failures |
| Ponytail | Not found in the tool catalog or targeted local tool/config inspection | Unavailable; not installed |
| Rust / Cargo | 1.98.0 functional; fmt and Clippy available | Exact dependency graph, targeted resolution, tests and compilation |
| rust-analyzer | rustup proxy present; component unavailable for the active toolchain | Not functional; not installed |
| cargo-deny / cargo-audit / cargo-nextest / Trivy | Not found locally; deny, nextest and Trivy configured in CI | Existing GitHub security logs and official advisory; full security gate remains CI-owned |
| Gitleaks / actionlint | 8.30.0 / 1.7.7 installed and functional | Native staged-patch/full-index secret scans and workflow checks |
| Android / Java / Gradle | Existing CLI 1.0.16261425, adb 34.0.5, Temurin 21.0.12.1+1 and Gradle 9.5.0 respond | Existing NDK for JNI compilation; no new local CLI/SDK installation |
| CodeRabbit | CLI 0.8.0 installed; authentication configured | Capability audited; no remote review invoked; deterministic checks and independent read-only diff review used |

No tooling was installed or reinstalled. Downloading the requested Rust dependency
is distinct from installing a development tool. Android metadata and the launcher
were inspected without installing or running a new Android CLI.

## Baseline versus PR regression

The five CI inputs (`Cargo.toml`, `Cargo.lock`, `.pipeline/android-toolchain.json`,
`scripts/pipeline-android-setup`, `.github/workflows/pipeline.yml`) are byte-identical
between master and each of the three PR branches. Both shared failures therefore
belong to the baseline, not to these PRs. This was checked against remote master
at `1312cb1`; it is not a claim that a new master workflow was dispatched.

| PR | Head | Existing pipeline run | Observed result |
| --- | --- | --- | --- |
| #27 | `d566b3a42aef3d4926a071c30a123e4f6abdb876` | [43575217539](https://github.com/lilugemusikmaking-design/phoneboost/actions/runs/43575217539) | Advisory and Android setup fail; Rust succeeds |
| #28 | `be988c700584cc0b9113866dfae25b2030783be0` | [43575240379](https://github.com/lilugemusikmaking-design/phoneboost/actions/runs/43575240379) | Advisory and Android setup fail; Rust succeeds |
| #29 | `d641d5e549a7b60e564347b418aab71a8aa42676` | [43575245742](https://github.com/lilugemusikmaking-design/phoneboost/actions/runs/43575245742) | Same shared failures plus JNI teardown assertion and poisoned-lock secondary failures |

### Rust dependency

`rustix` is a direct workspace dependency with an exact `=1.1.4` manifest constraint.
A lockfile-only change cannot satisfy that constraint. Direct consumers are
`pb-cli`, `pb-host`, `pb-runtime-secure` and `pb-web-bridge`; `pb-worker-core` and
`phoneboost-core-jni` also receive it through the dependency graph. There is one
resolved version, shared by these consumers.

[RUSTSEC-2026-0486](https://rustsec.org/advisories/RUSTSEC-2026-0486.html) identifies
the xattr Buffer issue and the fixed minimum `1.1.6`. The
[upstream fix](https://github.com/bytecodealliance/rustix/commit/8e6aaa6713d2bb64b1fdd8c472e3a32a10a76d70)
is consistent with that range. No calls to the affected xattr APIs were found in
PhoneBoost source; this is not a reason to waive the advisory.

The manifest is now exactly `=1.1.6`. `cargo update -p rustix --precise 1.1.6`
changes only that package's version and checksum among 81 lockfile packages:
no added/removed package, feature change or unrelated dependency update.
The crate's declared MSRV is 1.65, compatible with PhoneBoost's Rust 1.98.
An initial offline update could not find the new version in the cached index;
one bounded online resolution succeeded. The existing update to 1.1.5 would
still be affected and is not used.

### Android CLI drift

The expected CLI remains **1.0.16500706**. The old setup downloaded Google's
`commandlinetools-linux-16111833_latest.zip` (command-line tools 23.0), verified
its SHA-256, then executed its `android` launcher. That launcher downloads a
second-stage CLI from `/android/cli/latest/`. Thus a fixed launcher archive did
not fix the executed payload. The CI log shows this download before reporting
**1.1.16919734**, then correctly refusing the mismatch before SDK/test execution.

The fix pins the actual Linux x86-64 payload, rather than advancing the expected
version or removing validation:

- [Versioned payload](https://dl.google.com/android/cli/1.0.16500706/linux_x86_64/android-cli).
- [Official SHA-256](https://dl.google.com/android/cli/1.0.16500706/linux_x86_64/android-cli.sha256):
  `508840c2a9ce8768a9246ec7962834713967b46dd0cf0dad80fcde3d444947c0`.
- [Official metadata](https://dl.google.com/android/cli/1.0.16500706/linux_x86_64/METADATA)
  reports `version=1.0.16500706`. The payload URL returned HTTP 200, size 93,074,696 bytes.

The setup checks the download and cached executable bytes, then executes that
payload directly. Exact CLI version and installed SDK revision checks remain.
Java, Gradle, API level, build-tools, NDK and platform-tools pins are unchanged.
Context7 and the official [CLI download](https://developer.android.com/tools/agents/android-cli/download),
[update](https://developer.android.com/tools/agents/android-cli/commands/update)
and [sdkmanager](https://developer.android.com/tools/sdkmanager) documentation
were consulted. No `android update` is executed.

The four synthetic setup tests cover direct execution despite a leftover launcher,
checksum refusal before execution, version drift refusal before SDK installation,
and repair of a tampered executable from the verified cache. These tests are not
proof of a real Android installation. A fresh download/setup of the locked payload
is deliberately left to the existing GitHub job; no local tool installation was
performed for this repair.

### JNI failure: teardown race in the test oracle

The #29 log reaches all 17 puts and 10 exact 64 MiB compute results. The first
failure occurs in `with_production_android_session`, after the test deliberately
closes the client: actual `Wire(Io)`, expected only `SessionLost`. Twelve later
failures are consequences of the poisoned serialization lock. The "over 60 seconds"
line is a libtest progress notice, not a timeout verdict.

That helper and the 64 MiB test already exist on master. Three isolated executions
on unchanged baseline passed (76.54 s, 83.57 s, 73.61 s test durations). Those passes
alone do not classify the CI failure. A small real TCP probe then queued 32
authenticated pings and closed the client while pongs were pending. With the
original helper assertion it reproduced the identical `Wire(Io)` versus
`SessionLost` failure in 0.12 s, without the 64 MiB workload or any other test.
This causal probe used the updated rustix pin but the unchanged baseline helper.

Production already has two valid terminal paths: observing EOF returns
`SessionLost`; writing a queued response/heartbeat to the closed connection can
return `Wire(Io)`. Both report `DEVICE_LOST`; the authenticated loop revokes the
session before returning. The existing runtime partial-record/crypto/PBMUX
fail-closed test passed (15.61 s) without modification.

The repair is entirely inside `#[cfg(test)]`: accept those two precise terminal
errors **after intentional client shutdown**, and additionally assert
`DEVICE_LOST`, unauthenticated state and `Paired`. Other error classes still fail.
The short regression then passed in 0.12 s. No production error mapping, timeout,
mutex poison handling, compute algorithm or ResourceGuard behavior changes.
Evidence supports a timing-dependent test teardown oracle defect on the baseline;
it does not show a compute defect, CI timeout, or dependency on test order.

## Local validation

Tests used the installed toolchains and existing caches, with one heavy job at a
time and `CARGO_BUILD_JOBS=1`. Rust commands used the locked graph and offline mode
after the single targeted dependency resolution.

| Command / check | Actual result |
| --- | --- |
| Baseline isolated `cargo test --locked --offline -p phoneboost-core-jni --lib tests::e_gen_04_authenticated_remote_blake3_64_mib_repeats_and_failures -- --exact --nocapture --test-threads=1` | Three passes before the correction; durations above |
| Queued-pong probe, original / corrected oracle | Reproduced failure / PASS, 0.12 s each |
| Existing runtime `committed_transport_partial_records_crypto_and_pbmux_fail_closed` | PASS, 15.61 s |
| `scripts/pre-commit-fast` | PASS, 3.58 s: 20 pipeline tests including the four new setup checks, six proposal tests, contract validators, actionlint and `cargo fmt --all -- --check` |
| `cargo test --locked --offline --workspace -- --test-threads=1` | PASS: 486 tests including doctests, zero failures/ignored; 17 JNI tests; 627.31 s including compilation |
| `cargo clippy --locked --offline --workspace --all-targets` | Exit 0, 42.88 s; warnings in unchanged PBMUX/runtime code, no `-D warnings` policy added |
| `cargo build --locked --offline --release --target aarch64-linux-android -p phoneboost-core-jni` | PASS, 28.71 s; existing NDK 29.0.14206865, including rustix 1.1.6 |
| Lockfile comparison, dependency reverse tree, baseline/PR input comparison, `git diff --check` | Only rustix version/checksum differ among 81 packages; no other dependency changes; CI inputs unchanged by the three original PRs |

The first full workspace attempt stopped during compilation because an existing
`serde` cache output directory lacked generated `private.rs` (exit 101, 25.49 s,
no test verdict). Only `serde` build artifacts were invalidated with
`cargo clean --offline -p serde`; the successful run above is the subsequent,
separate attempt. No source or dependency change was needed for that cache repair.

Native checkpoint publication enforces the writer lock, explicit staged paths,
`git diff --cached --check`, and Gitleaks scans of both staged patch and complete
index. The pipeline checkpoint command's automatic state rewrite/push is not used.
Raw logs and attempt records stay local; they are not imported into product files.

Not run locally: cargo-deny, cargo-audit, nextest, Trivy, the new real CLI setup,
Gradle APK/tests/lint, or device tests. These are not marked PASS. GitHub's existing
security and Android jobs supply the remaining clean-run validation. No tool was
installed just to duplicate those CI gates, and no frontend or benchmark campaign
was rerun for this change.

## Resume boundary

Only this CI/toolchain branch is eligible for publication in this task. The three
existing PRs and historical source worktree remain unchanged. No merge or rebase
is authorized here. No new physical proof or benchmark is claimed.

**NEXT_ACTION:** inspect the dedicated PR's GitHub Actions results and obtain the
owner's review before any merge.
