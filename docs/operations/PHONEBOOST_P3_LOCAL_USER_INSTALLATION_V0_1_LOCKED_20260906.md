# PhoneBoost P3 local user installation profile v0.1

Status: **LOCKED — 2026-09-06**

## Scope

This profile packages the already reviewed Linux host, CLI, loopback web bridge,
and production frontend for one local desktop user. It adds installation and
launch convenience only. It grants no new authority and defines no new remote
operation.

The default prefix is `$HOME/.local`. `PHONEBOOST_INSTALL_PREFIX` may select a
different absolute, narrowly scoped prefix for testing or packaging. The
installer rejects broad or unsafe prefix values.

## Installed layout

- `$prefix/bin/phoneboost-control-center`: user command;
- `$prefix/share/applications/org.phoneboost.ControlCenter.desktop`: desktop
  menu entry with a visible terminal;
- `$prefix/share/phoneboost/libexec/`: exact production `phoneboostd`,
  `phoneboostctl`, and `phoneboost-web-bridge` binaries;
- `$prefix/share/phoneboost/frontend/build/`: production frontend build output
  produced from the current checkout;
- `$prefix/share/phoneboost/scripts/run_phoneboost_control_center.sh`: the
  reviewed runtime launcher.

The default installation command builds the current checkout before copying
the payload. `--no-build` is permitted only to install already produced local
artifacts. Updates replace the dedicated application payload through a staging
directory and retain the previous payload until the replacement is in place.
Prefix paths are canonicalized before any installation or removal; values that
resolve to `/` or `$HOME` are rejected. If rollback cannot restore an item, its
backup directory is retained for manual recovery rather than deleted.

## Launch and authority

Launching from the menu or `phoneboost-control-center`:

1. reuses an already reachable production daemon or starts one owned daemon;
2. starts the existing literal-IPv4-loopback web bridge;
3. prints a newly generated per-process capability URL in the visible terminal;
4. stops only processes owned by that launcher when the terminal is closed.

The installation does not start PhoneBoost, enable autostart, install a user or
system service, open a LAN listener, persist a capability, open a browser,
change Android state, or modify networking.

## Uninstallation

The uninstaller removes only the dedicated command, desktop entry, and
`$prefix/share/phoneboost` payload. It intentionally preserves peer pairing
state, configuration, logs, evidence, and repository files.

## Security and protocol invariants

- The bridge remains loopback-only and capability-protected.
- The capability remains transient and is not included in installed metadata.
- There is no generic RPC, command execution route, public listener, or second
  authority.
- Status reads remain passive and fail closed.
- Remote success and local fallback sources remain distinct.
- This profile does not alter Android, JNI, PBMUX, C07, C08, C09, C10, C12 API
  schemas, wire layouts, provider semantics, or ResourceGuard authority.
- No new runtime dependency is introduced.

## Required validation

- shell syntax for installer, uninstaller, installed launcher, and test;
- release build of the three production binaries and production frontend;
- isolated installation and uninstallation under a temporary HOME/prefix;
- byte comparison of installed executables and frontend entry point;
- desktop command and terminal-mode validation;
- capability-secret scan of installed launch metadata;
- existing relevant Rust/frontend checks in proportion to any source diff;
- `git diff --check` and final read-only security audit.
