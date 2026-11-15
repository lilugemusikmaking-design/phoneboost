# Android decrypt v0.1

Base master: `5446240bedb630be93bdec4f619afee7a6b781c7`, #35 merged with full history.

## Scope

Qualify only local Android Snow decrypt cost. No protocol, Noise pattern, cipher, keys, framing, PBMUX, frame-size, polling, ResourceGuard, timeouts, CPU affinity or RAM/GPU/NPU changes. Product APK remains intact.

## Exact backend audit

Rust 1.98.0, LLVM 22.1.8, `aarch64-linux-android`, ordinary Cargo release. No profile override, RUSTFLAGS, encoded flags or wrapper in the baseline environment; no repository Cargo config. The diagnostic APK uses `phoneboost-core-jni/diagnostic-timing` (including experimental-multicore); host is the frozen #35 release build.

Locked chain: Snow 0.9.6 default-resolver → ChaCha20Poly1305 0.10.1 → ChaCha20 0.9.1 + Poly1305 0.8.0. ARM64 advertises NEON, but ChaCha20 0.9.1 selects its soft backend unless `chacha20_force_neon` is explicitly set. Poly1305 has no NEON backend in this version. No forced-soft flag exists in the baseline.

Pinned sources: [ChaCha dispatch](https://github.com/RustCrypto/stream-ciphers/blob/b5d39c45c45df97335f4b6effb853cde97810de1/chacha20/src/lib.rs), [NEON gate](https://github.com/RustCrypto/stream-ciphers/blob/b5d39c45c45df97335f4b6effb853cde97810de1/chacha20/src/backends.rs), [changelog](https://github.com/RustCrypto/stream-ciphers/blob/b5d39c45c45df97335f4b6effb853cde97810de1/chacha20/CHANGELOG.md). README's old Cargo `neon` feature and “nightly-only” source wording are stale; the actual pinned cfg compiles with stable rustc 1.98 and is validated physically here. No dependency update or new crypto implementation.

## Isolated physical microbenchmark

Opt-in standalone example, never linked into product. Same Snow backend/dependencies and exact ciphertext-size distribution from #35 run2: 1,115 records / 67,172,828 encrypted bytes. An independently generated x86_64 fixture is SHA-256 checked on Android. Fixed public test-only keys/ephemerals exist only in the example; no real credentials are used or exported.

Timer matches the existing inner `read_message` probe; allocations and I/O are outside it. CLOCK_THREAD_CPUTIME_ID is local to Android and separate from wall time. Empty clock-pair overhead is recorded without subtraction. Every plaintext is exact; altered tags, altered ciphertext at a nonzero nonce and replay are rejected without advancing the receiving nonce.

Order ABAB, each process one warmup plus three retained samples. Baseline ~278–282 ms; NEON ~213–214 ms. Thread CPU follows wall time. All pre/post Health checks nominal, powerSave=false, thermal HAL Ready without override. This is evidence for an APK trial, not yet evidence of product/end-to-end benefit. Scheduling/cache conditions differ from the live application. See [numeric evidence](../evidence/android-decrypt-v0-1.json).

## Current decision

Only candidate: existing ARM64 ChaCha20 NEON cfg. Buffer resizing is rejected from this experiment because allocation is outside the measured dominant decrypt scope. APK paired A/B remains required before retaining any build change.

NEXT_ACTION = build equivalent baseline/candidate diagnostic APKs, then measure the same 64 MiB C12 scenario with frozen release host and strict Health checks.
