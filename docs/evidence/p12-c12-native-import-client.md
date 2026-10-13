# P12 — native client for private C12 BLAKE3 imports

Baseline: `master` at `aba2cf157b9b55f7006feda994fe3f4926adacf9`, clean,
with PRs #12–#19 integrated. Its Project pipeline and CodeQL were green.
Writer branch: `feat/p12-c12-import-cli-v1`.

## User-visible gap and result

P11 implemented the approved real-input daemon path, but `phoneboostctl` could
only invoke the immutable three-byte fixture. The new command is:

```sh
phoneboostctl compute blake3 --stdin < input.bin
```

The operator opens the file through shell redirection. The client accepts no
path parameter and sends exactly one already-open descriptor with
`imports.create`, polls `imports.get`, and calls `compute.submit_import` once.
All requests use one connection, preserving the connection-bound authority.
The existing fixture command remains unchanged. The client copies no payload.

## Authority and boundaries

- Existing approved C12 import resource, async wire and imported BLAKE3
  consumer contracts are unchanged; no new daemon method or parameter.
- The existing pinned `rustix` dependency supplies safe SCM_RIGHTS sending.
- The daemon enforces ownership/type, 128 MiB size limit, private storage,
  C11 placement and C07–C10/ResourceGuard authority.
- A partial request write never resends the descriptor. Mutations are not
  retried, re-imported or moved onto another connection.
- Readiness and terminal response have separate absolute 60-second bounds.
  Byte-by-byte response trickling cannot extend those bounds.
- A lost, timed-out or malformed terminal response is explicitly unknown;
  it produces no digest/success and no retry. Client disconnect delegates
  cleanup to the existing daemon lifecycle, without claiming an acknowledgement.
- Result validation checks all seven typed fields, expected byte count,
  lowercase digest encoding and allowed source/reason combinations.
- Exit 0 is remote success; 3 is explicit completed local fallback; 1 is
  failure/unknown; 2 is syntax rejection.
- Private handles and descriptor values are never displayed. Remote error
  text is replaced with a fixed diagnostic; Debug excludes the result digest.
- No MCP, new provider, licence change, arbitrary execution, performance
  claim, phone configuration or frontend API change.

## Executed local validation

- `cargo test --locked -p pb-cli -p pb-web-bridge`: PASS, 23 client library
  tests, 2 CLI command tests, 18 bridge tests (43 total).
- `cargo clippy --locked -p pb-cli -p pb-web-bridge --all-targets --no-deps -- -D warnings`:
  PASS.
- `scripts/project-pipeline check-fast`: PASS, including 12 native pipeline
  tests, actionlint, diff check and workspace formatting.
- Three existing C12 contract checkers: PASS (11, 12 and 16 negative oracles).
- Production daemon/client build: PASS.
- `python3 scripts/test_c12_import_cli.py`: PASS on real isolated processes:
  exact `abc` digest and truthful local fallback; multi-chunk input; empty and
  oversized input rejection; pipe rejection; arbitrary path argument rejection;
  confirmed removal of private spool files after each operation.

The real-process test is now a required step of the Rust CI job. It uses fresh
temporary XDG directories and never reuses pairing state or changes the phone.
Full workspace/platform validation is delegated to the existing GitHub pipeline
on the containing commit; use that commit's PR checks for the final CI result.

## Limits and next action

Physical phone test: NOT RUN in P12. No new physical or remote-success claim.
Remote provenance is tested using protocol responses; the actual daemon smoke
proves local fallback, not phone execution. Existing P11 authenticated remote
tests remain in the full workspace suite without being promoted to physical proof.

The browser remains on the immutable demonstration action. A browser file
workflow and MCP require their own bounded integration design. This increment
is complete once the containing commit's CI is green; default-branch merge
requires authorization beyond the completed #12–#19 integration.
