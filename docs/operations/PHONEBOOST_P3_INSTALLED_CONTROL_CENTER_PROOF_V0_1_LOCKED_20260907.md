# PhoneBoost P3 installed Control Center proof profile v0.1

Status: **LOCKED — 2026-09-07**

## Purpose and scope

This profile proves that the per-user P3 installation can launch the reviewed
PhoneBoost Control Center from its installed payload. It is an operator proof
of installation and launch integration, not a new protocol, provider, remote
operation, or authority.

The proof assumes that the P3 payload is already installed and that a
production daemon is already reachable with an authenticated Android worker,
ready auto-use, and the BLAKE3 provider available. It must not build artifacts,
start a daemon, mutate networking, operate Android, or use repository binaries.

## Accepted installed surface

The proof resolves one canonical installation prefix and uses only:

- `$prefix/bin/phoneboost-control-center`;
- `$prefix/share/applications/org.phoneboost.ControlCenter.desktop`;
- `$prefix/share/phoneboost/libexec/phoneboostctl`;
- the remaining installed payload reached by the installed launcher.

The desktop entry must launch the installed command and keep a visible terminal.
The production frontend entry point must be present in the installed payload.

## Preconditions

Before launch, the installed CLI must report exactly:

- `PhoneBoost: READY`;
- `Local API: ACTIVE`;
- `Android worker: AUTHENTICATED`;
- `Auto-use: AVAILABLE`;
- `Auto-use reason: READY`;
- `Remote BLAKE3: AVAILABLE`.

This ensures the installed launcher reuses the existing production daemon. A
missing or different value fails closed.

## Browser and compute proof

The installed launcher must publish a fresh one-time URL on literal IPv4
loopback with a 256-bit lowercase hexadecimal capability in the URL fragment.
The proof makes authenticated loopback requests with proxy use and user curl
configuration disabled.

The native snapshot must be fresh and LIVE and show the same ready runtime
state, including:

- daemon `READY` and local API `ACTIVE`;
- authenticated worker;
- discovery, controller-lease, and admission/readiness objects with their exact
  C12 schema and an allowed `state / reason` pair;
- provider and auto-use available, with reason `READY`.

P3 does not require a passive P2 observation to remain fresh. Discovery and
admission/readiness observations intentionally expire fail-closed and a status
read or compute must not refresh them. The proof prints the three observed
pairs exactly and the operator confirms that the browser renders those same
values without promoting `STALE`, `UNKNOWN`, or `UNAVAILABLE` to a fresh state.
P2 freshness is established by its separate durable physical proof; this P3
profile proves the installed launcher and remote compute path.

The operator opens the URL in a private browser window and confirms the fresh
P3 rendering. The operator then runs only fixture `c10-abc-v1`. The bridge
response and its following snapshot must both report:

- input size 3;
- digest `6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85`;
- `execution_source = REMOTE_SUCCESS`;
- `auto_use_reason = READY`.

Fallback or unavailable states never satisfy this proof.

## Bounded shutdown and secret handling

The proof owns only the installed launcher process. Cleanup sends a graceful
termination, waits for a bounded interval, uses `SIGKILL` only if necessary,
and always reaps the child. It then verifies that the capability URL is no
longer reachable. It does not stop the pre-existing daemon.

Temporary files use a private directory and are removed on every exit path.
The capability and capability-bearing URL must not be copied into repository
evidence, documentation, logs, shell history, or the final durable proof
record. Only the bounded result and non-secret observations may be recorded.

## Invariants

- No root, service, autostart, browser automation, ADB, tunnel, LAN listener,
  manual endpoint, or network mutation.
- No generic RPC or command execution route.
- No new dependency or global mutable state.
- Status and snapshot reads remain passive.
- Android, JNI, PBMUX, C07, C08, C09, C10, and C12 layouts are unchanged.
- A PASS may be recorded only after the complete operator workflow succeeds.
