# P13 durable auto-use native implementation

User approved PR #21's exact C12/C13/bridge contract with “allons y , faut avancer”.
Implementation branch: `feat/p13-durable-auto-use-control`, based on `d630234`.

## Implemented scope

- Daemon-owned `auto_use.get/set`, exact shapes, no descriptors, duplicate
  parameter rejection and fixed safe errors. Allowlist now has 21 methods.
- Private 0600 C13 config with descriptor-relative no-symlink ancestor traversal,
  bounded strict parsing, atomic write/file-fsync/rename/directory-fsync.
- Missing config defaults ON; invalid config starts OFF/ERROR, remains observable
  and is never silently repaired. Save failure forces OFF/ERROR without claiming
  durable OFF or automatically restoring ON. New explicit commands may retry a
  transient write failure. Control serialization is separate from compute.
- `phoneboostctl auto-use status|on|off`. Fresh nonblocking-connect C12 channel,
  absolute five-second exchange bound, strict replies and no mutation replay.
- Serde is reused at its existing locked version; no new package version.

## Actual local evidence

- Native preference tests: PASS 6/6, including concurrent setters, corruption,
  bad mode/symlink/directory/oversize and injected write/fsync/rename failures.
- C12 handler tests: PASS 25/25, including live wire get/set, no mutation from
  malformed params, duplicate fields or SCM_RIGHTS, and unavailable context.
- Local validation tests: PASS 37/37.
- CLI and existing web bridge tests: PASS 28 CLI library, 2 CLI command,
  18 web bridge. Client tests include absolute deadline, lost response without
  replay, strict result agreement, duplicate results and safe error mapping.
- Existing authenticated controller disable tests: PASS 3/3.
- Clippy host/client all targets, no dependencies, warnings denied: PASS.
- Native check-fast and pre-commit-fast: PASS.
- Fresh executable build plus `scripts/test_auto_use_control_cli.py`: PASS,
  isolated real daemon/CLI OFF and ON across restart, exact local BLAKE3 fallback,
  invalid request rejection and unknown-version config preserved OFF/ERROR.
- Existing `scripts/test_c12_import_cli.py`: PASS, all real-file import cases.

The new real-process smoke is mandatory in the existing Rust CI job. Full CI
is tracked separately on the checkpoint commit. No physical phone test or new
remote-success/speedup claim. Wrong-owner config logic is present but no chown
or privileged owner-injection test was run in this native increment.

## Remaining approved work

The desktop switch remains browser-local until the typed HTTP/control lane and
frontend reconciliation are implemented. This is a native checkpoint, NOT a
claim that the complete P13 desktop ON/OFF is finished. The explicit manual
pairing bootstrap without an auto-use controller returns unsupported control;
it does not acquire new auto-use authority. No merge to master is authorized.
