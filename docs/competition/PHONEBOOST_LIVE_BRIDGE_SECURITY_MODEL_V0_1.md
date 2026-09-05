# PhoneBoost live bridge security model V0.1

Status: implementation contract for the P1 local Control Center bridge.  
Date: 2026-09-05 Europe/Paris.

## Authority and scope

`phoneboost-web-bridge` is a presentation adapter. It is not an authentication,
lease, admission, discovery, or provider authority. It consumes only the existing
typed `pb-cli` client for C12 `system.status` and the locked synchronous
`compute.submit` profile for `pb.native.blake3/1` with fixture `c10-abc-v1`.

It does not change C12, Noise, PBMUX, C07, C08, C09, C10, Android, JNI,
`ResourceGuard`, or worker state. It exposes no generic RPC or Unix-socket proxy.

## Listener and browser model

- The process binds literal IPv4 loopback `127.0.0.1` on an OS-selected port.
- There is no bind-address or public-listener option.
- The sanitized production frontend and bridge API share that exact origin.
- A hosted Control Center is recorded-evidence-only and does not call localhost.
- No CORS headers are emitted and `OPTIONS` is unsupported.
- The exact `Host` header is required. Cross-origin `Origin` and Fetch Metadata
  values fail closed. The native-action POST additionally requires the exact
  same-origin `Origin`; a missing POST origin is rejected.
- A kernel-random 256-bit per-process capability is delivered only in the launch
  URL fragment. The frontend keeps it in memory, removes the fragment immediately,
  and sends it in `X-PhoneBoost-Bridge-Token`.
- The capability is never accepted in a path, query, cookie, or body and is never
  written to request logs.

## Exact API

The only native-facing routes are:

```text
GET  /bridge/v1/snapshot
POST /bridge/v1/compute/blake3
```

The POST body is exactly:

```json
{"fixture":"c10-abc-v1"}
```

Missing, extra, mistyped, unknown, or non-object values are rejected before any
native call. No browser-supplied C12 method, operation identifier, path, input
bytes, provider, endpoint, lease, resource parameter, or command is accepted.

## Limits

- One request is processed at a time by one process.
- HTTP headers are bounded to 8 KiB and 32 fields.
- API request bodies are bounded to 1 KiB.
- Native responses and API responses are bounded to 64 KiB.
- Static assets are restricted to the resolved frontend build root and 8 MiB.
- The build root is fixed to `frontend/build`; there is no caller-selected path.
  Static responses are limited to the explicit HTML, CSS, JavaScript, JSON,
  SVG, PNG, ICO, WOFF, and WOFF2 asset types used by the production bundle.
- Header/body reads and writes have five-second socket timeouts.
- Native status has a two-second I/O timeout; the locked compute action has the
  existing bounded 60-second I/O timeout.
- Connections close after one response; transfer encoding and request pipelining
  are rejected.

## Truth and freshness

A snapshot is `LIVE` only after a current, strictly validated C12 response. It is
stamped with an observation time and a maximum age of three seconds. The frontend
polls every two seconds and removes the LIVE label on failure or expiry; it never
silently reuses stale LIVE data.

The bridge exposes the following independently:

- local daemon reachability: current successful C12 exchange;
- authenticated session: exact C12 `remote_worker_state`;
- provider readiness: exact C12 `remote_blake3_available` for the locked provider;
- auto-use state and reason: exact closed C12 strings;
- last execution source: exact result of a bridge-initiated locked action, with its
  observation time.

C12 does not separately expose discovery/reachability, controller-lease state, or
`ResourceGuard` readiness. Those gates are therefore returned as `UNKNOWN`; the
bridge does not infer them from aggregate readiness. Recorded physical evidence is
shown separately and never substituted for LIVE state.

The compute response preserves exactly one of:

- `REMOTE_SUCCESS`
- `LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE`
- `LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE`

HTTP success never changes a fallback into remote success.

## Frontend integrity

The privileged local bundle contains no third-party scripts, analytics, session
recording, remote fonts, or external network fetches. The bridge emits a self-only
Content Security Policy, denies framing, disables MIME sniffing, and marks native
responses `no-store`.

## Operator proof status

`scripts/prove_p1_live_bridge_local.sh` builds the production frontend and bridge,
starts only the loopback adapter, checks the current native status, and guides one
bounded three-phase browser proof. Android Wi-Fi changes and browser clicks require
explicit operator confirmation; the script does not use ADB, create tunnels, or
mutate networking. The workflow has been completed and recorded as the
operator-observed evidence
`docs/evidence/p1-live-bridge-local-browser-physical.txt`. A private browser
window consumed the current fragment-delivered capability in a clean page context.
The script emitted that URL transiently for operator use; the evidence records
neither the capability nor its URL, and neither is persisted in repository files,
documentation, committed logs, or the handoff.
