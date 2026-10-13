# P10 C12 asynchronous import runtime evidence

Date: 2026-10-13

Branch: `feat/p9-c12-import-ingress-v1`

Baseline checkpoint: `0bf1877d3b8196d28358cf6c17a2d8b5ff0fd580`

## Authority and scope

This increment implements only the product-owner-approved contracts:

- `PHONEBOOST_C12_IMPORT_RESOURCE_DECISION_V0_1.md`;
- `PHONEBOOST_C12_IMPORT_ASYNC_WIRE_DECISION_V0_1.md`;
- `phoneboost-c12-import-v1.json`;
- `phoneboost-c12-import-async-wire-v1.json`.

The C12 allowlist expands from 15 to exactly 18 methods with
`imports.create`, `imports.get` and `imports.cancel`. No payload read/download
method, public consume method, generic execution surface, MCP process, C11
bypass or ResourceGuard bypass is added.

## Runtime closure

- `imports.create` accepts exactly empty params plus one non-truncated
  `SCM_RIGHTS` descriptor and schedules one bounded receiver before returning a
  connection-bound 256-bit handle in `RECEIVING`.
- The receiver uses one 64 KiB buffer, reserves byte quota before each write,
  enforces the fixed 60-second monotonic deadline and publishes `READY` only
  after flush, file sync, descriptor close and private rename.
- `imports.get` is passive and returns only the locked receiving, ready or
  retained terminal shapes. It does not extend TTL, consume data or create
  remote authority.
- `imports.cancel` signals a live receiver and waits for source/destination
  descriptor close, private-object absence and quota release before success.
  The wait is bounded at five seconds. Unconfirmed cleanup returns
  `IMPORT_CLEANUP_UNCONFIRMED` while accounting remains charged.
- Connection close invokes the same confirmed cancellation path for all owned
  receiving and ready objects.
- Handles resolve only for the authenticated connection, peer UID and current
  daemon incarnation. Cross-connection access and unknown handles both return
  `IMPORT_NOT_FOUND`.
- Import storage initializes and clears stale v1 objects before the daemon can
  print `READY`; initialization failure is fail-closed.

## Deterministic negative coverage

Tests prove:

1. exact 18-name allowlist and method classes;
2. missing/two descriptors, extra params, descriptors on get/cancel and invalid
   handle encodings fail closed;
3. real `SCM_RIGHTS` create/get/cancel exchange reaches ready and terminal
   states with exact response shapes;
4. another authenticated local connection cannot resolve the handle;
5. cancellation during receiving waits for receiver shutdown;
6. zero-deadline cancellation reports cleanup-unconfirmed and retains quota at
   the response boundary;
7. later receiver cleanup releases that quota exactly once;
8. connection close removes a live receiving object;
9. receiving timeout is retained as a bounded public error without authority
   disclosure;
10. terminal state is idempotent and cannot resurrect;
11. payloads, handles, paths, UIDs and descriptor numbers remain absent from
    public errors and debug output;
12. all prior descriptor-free C12 behavior remains green.

## Validation actually executed

- admission validation: 37/37 PASS;
- private import lifecycle: 18/18 PASS;
- local C12 API: 18/18 PASS;
- complete `pb-host` library: 219/219 PASS, including the authenticated 64 MiB
  proof;
- changed-crate Clippy, all targets, no dependencies, `-D warnings`: PASS;
- locked resource checker: PASS with 11/11 negative mutations;
- locked async wire checker: PASS with 12/12 negative mutations;
- agentic boundary checker: PASS, including MCP absent;
- native `check-fast`: PASS;
- corrected full software aggregate on the unchanged tree: PASS 14/14 — Rust
  Clippy, nextest 415/415, mandatory workspace tests, frontend 331/331,
  production build, Playwright 10/10, Android release JNI plus assemble/tests/
  lint, cargo-deny, Trivy fixed-CRITICAL gate and Maestro syntax 2/2.

The first full attempt exposed only missing local Playwright/Rust/Android/
security tool installations in the new worktree. The repository-pinned tools
were installed, and the previously unavailable components were rerun and
passed. No product failure was hidden or retried unchanged.

## Physical evidence and remaining boundary

No phone or physical run was performed. This increment changes only the Linux
same-UID local ingress and does not change Android behavior, remote execution,
transport selection, pairing or performance claims.

The imported object intentionally has no public consumer in this increment. A
future typed operation must be separately contracted and must still traverse
C11 and the existing C07/C08/C09/C10/ResourceGuard authority path. MCP remains
absent and unauthorized for implementation by this closure.
