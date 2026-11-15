# Android decrypt v0.1

Base master: `5446240bedb630be93bdec4f619afee7a6b781c7`, #35 merged with full history.

## Scope

Qualify only local Android Snow decrypt cost. No protocol, Noise pattern, cipher, keys, framing, PBMUX, frame-size, polling, ResourceGuard, timeouts, CPU affinity or RAM/GPU/NPU changes. Product APK remains intact.

## Exact backend audit

Rust 1.98.0, LLVM 22.1.8, `aarch64-linux-android`, ordinary Cargo release. No profile override, RUSTFLAGS, encoded flags or wrapper in the baseline environment; no repository Cargo config. The diagnostic APK uses `phoneboost-core-jni/diagnostic-timing` (including experimental-multicore); host is the frozen #35 release build.

Locked chain: Snow 0.9.6 default-resolver → ChaCha20Poly1305 0.10.1 → ChaCha20 0.9.1 + Poly1305 0.8.0. ARM64 advertises NEON, but ChaCha20 0.9.1 selects its soft backend unless `chacha20_force_neon` is explicitly set. Poly1305 has no NEON backend in this version. No forced-soft flag exists in the baseline.

Pinned sources: [ChaCha dispatch](https://github.com/RustCrypto/stream-ciphers/blob/b5d39c45c45df97335f4b6effb853cde97810de1/chacha20/src/lib.rs), [NEON gate](https://github.com/RustCrypto/stream-ciphers/blob/b5d39c45c45df97335f4b6effb853cde97810de1/chacha20/src/backends.rs), [changelog](https://github.com/RustCrypto/stream-ciphers/blob/b5d39c45c45df97335f4b6effb853cde97810de1/chacha20/CHANGELOG.md). README's old Cargo `neon` feature and “nightly-only” source wording are stale; the actual pinned cfg compiles with stable rustc 1.98 and is validated physically here. No dependency update or new crypto implementation. Evidence `build` describes the isolated microbenchmark; `apk_build` describes the paired APKs. Their source-content checkpoint is3dcfdd32, committed after the microbenchmark from the same bytes.

## Isolated physical microbenchmark

Opt-in standalone example, never linked into product. Same Snow backend/dependencies and exact ciphertext-size distribution from #35 run2: 1,115 records / 67,172,828 encrypted bytes. An independently generated x86_64 fixture is SHA-256 checked on Android. Fixed public test-only keys/ephemerals exist only in the example; no real credentials are used or exported.

Timer matches the existing inner `read_message` probe; allocations and I/O are outside it. CLOCK_THREAD_CPUTIME_ID is local to Android and separate from wall time. Empty clock-pair overhead is recorded without subtraction. Every plaintext is exact; altered tags, altered ciphertext at a nonzero nonce and replay are rejected without advancing the receiving nonce.

Order ABAB, each process one warmup plus three retained samples. Baseline ~278–282 ms; NEON ~213–214 ms. Thread CPU follows wall time. All pre/post Health checks nominal, powerSave=false, thermal HAL Ready without override. This is evidence for an APK trial, not yet evidence of product/end-to-end benefit. Scheduling/cache conditions differ from the live application. See [numeric evidence](../evidence/android-decrypt-v0-1.json).

## Paired application proof

Two warmups then three pairs, order A/B, B/A, A/B. Same Samsung, same 64MiB input/digest, frozen #35 release host, same USB/ADB, CPU profile 2, no affinity. All 8 outcomes REMOTE_SUCCESS with exact digest. Source checkpoint for both APKs: `3dcfdd32bce15c4425dce68a5c32cb731f1bb447`; only the target cfg differs. Every APK zip entry is byte-identical except JNI. Baseline APK is exactly #35's `efc70d8a…`; candidate `685c05c1…`. Full hashes, effective Cargo dependency fingerprints and raw numeric traces are in the evidence.

| measure | baseline median | NEON median | interpretation |
|---|---:|---:|---|
| Process end-to-end |3402.680ms|3293.384ms|−109.296ms / −3.21% between medians|
| Inner Decrypt aggregate |667.408ms|570.166ms|−14.57%; independently bracketed snapshots|
| Decrypt on 1105 PUT records |667.695ms|570.611ms|−14.54%; equal payload, per-record trace envelope|
| Host SessionPoll |3053.748ms|3050.796ms|overlapping wait view; not added to Android durations|
| PUT loop |2815.271ms|2734.225ms|same 17PUT /64MiB|
| Useful PUT throughput |22.733MiB/s|23.407MiB/s|USB/ADB path actually measured|
| Android process CPU |1190ms|1070ms|ADB-bracketed ticks; includes capture overhead|
| Host process CPU |310ms|300ms|CLI-bracketed process ticks|

Every pair favors NEON for decrypt and end-to-end. Paired end-to-end speedups are1.04481×,1.02519×,1.01892×; median paired gain2.46% (distinct from ratio of medians). Local Decrypt trace spans include probe bookkeeping, while the aggregate timer ends slightly earlier; these are independent views, never subtracted or added.

Application frames stay 1113 host→Android and 25 Android→host; PUT geometry stays 16×4194240+1024 bytes,1105 encrypted PUT records. Whole-snapshot Decrypt counts can be1114 or1115 because a40-byte control frame plus16-byte tag falls inside one capture envelope. The validator keeps all raw counts and checks control-adjusted payload equality; it does not erase a frame. The primary per-PUT comparison uses exactly the same1105 records.

Health remains nominal before/during/after every run, sample age within 6000ms, HALReady with no override, no ResourceGuard refusal, memory recovered with unchanged 896MiB/10s policy. No sustained-thermal or other-device claim follows from these short trials.

## Retained change

Root `.cargo/config.toml` selects the existing ChaCha20 NEON backend only for `aarch64-linux-android`. No target-cpu specialization, new dependency, protocol/key/cipher/framing change, buffer/polling/ResourceGuard change or host codegen change. It applies to Android ChaCha operations in both directions; the tiny response encrypt cost remains~0.21ms. Poly1305 remains the pinned software backend.

Packaging checks reject environment flags and discovered ancestor/Cargo-home rustflags (including conflicting force-soft cfgs) that override or merge with the measured target config and confirm target NEON availability. A5/A6 scripts enter the repository root so Cargo discovers the config. For baseline reproduction, use the preserved source checkpoint above (before target config), standard release and no flags; candidate reproduction uses the same source with target-scoped `--cfg chacha20_force_neon`. The current committed config must reproduce the candidate APK SHA exactly.

Buffer resizing is deliberately excluded: allocation sits outside the dominant inner Decrypt timing and was not the tested variable.

## Residual and next action

The largest directly localized reducible family is Android polling: median~634.017ms of receive polling plus post-handler completion delay, versus~570.611ms decrypt. Receive/read spans are~595.845ms and contain transport/arrival/scheduling wait, not pureUSB latency. Host SessionPoll~3050.796ms overlaps these views and must not be summed. No transport-capacity claim or polling change is made here.

NEXT_ACTION = traiter le polling Android résiduel mesuré (~634 ms réception + retour handler), sans modifier la crypto ni le protocole.

## Validation and restoration

Physical microbenchmark and 8 whole-path runs passed exactness/Health. Runtime/security release tests passed 31+6 tests and 4 doctests; both APK assemble/lint passed. Build-guard tests and evidence validation/rejection tests passed. Strict extra `-D warnings` Clippy encounters a pre-existing diagnostic warning; project-policy Clippy passes, with no unrelated source edit. The full product Android pipeline passes (packaging, JNI/Health/lease/pairing tests and lint), without installing the product APK. The retained config reproduces the measured candidate APK SHA exactly. GitHub CI remains pending publication.

Automatic restoration attempted in finally and failed explicitly on this Samsung. Profile 1 and timing OFF are restored; manual Battery Saver ON confirmation is pending. No additional workload is authorized or running.
