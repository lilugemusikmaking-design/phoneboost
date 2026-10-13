# P11 — typed C12 imported BLAKE3 consumer evidence

Date: 2026-10-13

## Locked scope

The product owner approved the exact `compute.submit_import` contract in PR #18.
This increment implements only that method for `pb.native.blake3/1`. It adds no
generic execution, public payload read/download, path input, reconnectable
handle, C12 job surface or MCP adapter.

## Implementation

- The closed C12 allowlist contains exactly 19 methods and classifies
  `compute.submit_import` as mutating.
- Validation accepts only `op_id` plus one exact lowercase 256-bit import
  handle and rejects every ancillary descriptor before lookup.
- The authenticated connection/UID/incarnation-bound store distinguishes
  `RECEIVING`, empty `READY`, usable `READY` and terminal/out-of-authority
  states without exposing the handle.
- A usable import is opened, unlinked and tombstoned atomically before
  execution. Concurrent consumption succeeds at most once and the open
  descriptor remains usable after unlink.
- C11 receives the exact byte length. Remote C09 upload reads existing
  `MAX_PUT_BODY` chunks; local BLAKE3 reads fixed 65,536-byte chunks. Neither
  path allocates the full imported payload.
- Known and ambiguous remote failures rewind the same unlinked descriptor and
  preserve the existing truthful execution-source strings. Input I/O failure
  returns `IMPORT_UNAVAILABLE` without resurrection.
- The terminal success has exactly seven locked result fields and never echoes
  the private handle.

## Executed evidence

- Locked consumer checker: PASS, exact 1/1 method and 16/16 negative oracles.
- Existing async import checker: PASS, exact 3/3 methods and 12/12 negative
  oracles; MCP absent.
- `cargo test --locked -p pb-host --lib`: PASS 224/224 after the core
  implementation, including the authenticated 64 MiB proof.
- Added targeted tests: PASS for exact params, FD rejection, cross-connection
  authority, empty non-consumption, single-use/replay, concurrent consume,
  real create/get/submit/get wire flow, 1-byte and 128 MiB bounded local
  streaming, and streamed remote C09/C10 success.
- `cargo clippy --locked -p pb-host --all-targets --no-deps -- -D warnings`:
  PASS.
- `git diff --check`: PASS.

The phone was not used for this checkpoint. The changed ingress is the Linux
same-UID C12 socket; authenticated remote behavior is covered by the unchanged
C07-C10 host integration harness. A new physical run is deferred unless final
software/CI evidence exposes an Android-specific regression or a dedicated
physical acceptance step is selected.

## Exact-head CI and review

- Locked contract commit `25772796dc2a8dfd0a49087d63c958b7993e6370`:
  CodeQL PASS; its first Project pipeline attempt hit one unchanged JNI
  disconnect assertion (`Wire(Io)` versus `SessionLost`) and poisoned the
  serialized JNI test lock; the single bounded rerun PASSed.
- Product commit `0201a9310639202440ff6779a31c244a71fd6aae`:
  Project pipeline runs `38425087918` and `38425128934` PASS 7/7; CodeQL run
  `38425128984` PASS; CodeRabbit PASS; PR #19 merge state `CLEAN`.

No retry beyond the single recorded contract rerun was needed.
