# Durable desktop ON/OFF control proposal V0.1

Status: PROPOSED — explicit product-owner approval required before implementation.
Baseline: master `50633fe65c8d1568cc0555d606543dace11cf718`.

## Decision requested

Approve the following bounded C12, C13 and browser-bridge extension as one
implementation scope. No runtime method, route or persistence is activated by
this proposal. Existing C12 methods and result schemas remain unchanged.

Desktop ON means this Linux host may automatically use a paired Android worker.
It does not make Linux a worker and does not mean READY. Desktop OFF prevents
new remote work by this host and invalidates its current auto-use session.
It does not power off the phone, change Android's own participation preference,
unpair devices, or prohibit explicitly requested local BLAKE3 fallback.

## Authority reconciliation

The source mirror's SPEC V0.7 and Contract Set V1.3 remain higher authority.
Contract Set C07 §3 assigns `controller.acquire/release` to the Android lease;
these methods must not be repurposed as a desktop preference. C12 §§2,7 preserve
daemon and remote admission authority; C12 §12 assigns preferences to C13.
C13 §§2–6 already assign Linux `config.json` to the daemon and require private,
atomic storage. Its exact preference fields are not specified there.

This proposal explicitly clarifies the scope of C12's lease requirement:
changing a *local scheduling preference* needs same-UID authorization, not a
live Android lease. All resulting remote work still needs C07/C08 approval.
OFF must work while the phone is absent. This clarification and the desktop
role wording require approval, not inference from the UI handoff's worker text.

## Exact C12 surface

API 1, existing NDJSON envelopes and same-UID authentication; zero descriptors.
Add only these two methods:

```json
{"api":1,"id":"read","method":"auto_use.get","params":{}}
{"api":1,"id":"write","method":"auto_use.set","params":{"enabled":false}}
```

`auto_use.get` is read-only; `auto_use.set` is mutating. Params must have exactly
the keys shown, with no duplicate keys; `enabled` is a JSON boolean, never a
number or string. Both use
the exact success result below. `enabled` is the daemon's actual scheduling
preference, not readiness; `persistence` is `DEFAULT`, `SAVED` or `ERROR`.

```json
{"api":1,"id":"write","ok":true,"result":{"enabled":false,"persistence":"SAVED"}}
```

- `DEFAULT`: no config existed on startup; default ON is in effect.
  `enabled:false,persistence:DEFAULT` is forbidden.
- `SAVED`: the active preference matches a validated durable config.
- `ERROR`: storage could not be validated or committed; effective preference
  is OFF. `enabled:true,persistence:ERROR` is forbidden.
- Successful `set` always returns `SAVED`, after both durable commit and
  application to the controller. An unchanged value is allowed; it does not
  restart discovery or invalidate a healthy epoch merely to acknowledge it.
- `get` has no discovery, cleanup, write or repair side effect.

Closed error codes in the existing REQUEST error envelope:
`LOCAL_BAD_REQUEST` (shape/descriptor), `LOCAL_UNSUPPORTED_METHOD` (no control
context), `LOCAL_STATE_WRITE_FAILED` (persistence failure). Fixed safe messages;
no paths, supplied values, raw I/O errors or secrets. Older daemons reject the
new methods; clients show control unavailable, never emulate success locally.

## Persistence and ordering

Use only C13's daemon-owned `config.json` under the existing private state root.
Exact v1 content, at most 256 bytes, no extra or duplicate keys:

```json
{"schema_version":1,"auto_use_enabled":false}
```

Validate ownership, mode 0600, regular file, size and no symlink traversal;
parents retain C13 mode 0700. Reject malformed or unknown-version existing
files without overwriting them. Start effective OFF with persistence ERROR;
status/control remain reachable, but writes cannot silently repair that file.
The first-run missing file defaults ON. Never import browser localStorage as
daemon authority. Startup loads this preference *before* enabling auto-use.
The separate explicit pairing bootstrap remains separate from auto-use.

Serialize `get`/`set` with one preference mutex, independent of the compute
operation mutex. Concurrent setters take effect in daemon acceptance order.
For OFF: disable/invalidate first, then commit config using same-directory
0600 temp, write, file fsync, rename, directory fsync. For ON: commit first,
then enable. Respond only after both steps. Before returning a write failure,
force effective OFF and mark ERROR; never roll back to active ON automatically.

A rename/fsync failure can leave the old or new complete file. Do not claim
durability or automatically retry the mutation. A later read reports effective
OFF/ERROR; after restart the validated complete file determines the preference.
Thus a failed OFF save does NOT promise OFF across restart. The UI must say so.
A subsequent explicit set may retry a transient write failure, but not overwrite
an invalid or newer-version config. Crash before an acknowledgement likewise
has unknown outcome; clients reconnect and read, never blindly replay.

OFF reuses existing epoch invalidation and session cancellation. It does not
promise immediate remote resource reclamation: existing release/expiry and
bounded cleanup rules apply. Work whose result was already proven may retain
REMOTE_SUCCESS. A lost or ambiguous remote result retains its existing truthful
fallback classification. The local fallback itself is not forcibly cancelled.

## Typed browser and CLI mapping

Add CLI `phoneboostctl auto-use status|on|off`; no generic RPC or path arguments.
Add authenticated bridge routes `GET /bridge/v1/auto-use` (empty body) and
`POST /bridge/v1/auto-use` (exact `{"enabled":boolean}`). Bridge maps these to
the typed client methods; the browser never supplies a C12 method or endpoint.
HTTP 200 body is the exact validated two-field C12 result. HTTP 400 represents
invalid input, 503 unavailable native control, 502 malformed native response,
504 native timeout/unknown write outcome. Error body has exactly `{"error":code}`
where code is respectively `BAD_REQUEST`, `CONTROL_UNAVAILABLE`,
`INVALID_NATIVE_RESPONSE`, `OUTCOME_UNKNOWN` (POST timeout) or
`CONTROL_TIMEOUT` (GET timeout). A native write failure is 503 with
`STATE_WRITE_FAILED`; no response text passthrough. Existing auth errors stay
unchanged. No automatic POST retry on any status or lost HTTP response.

Preserve loopback-only binding, capability token, exact Host/Origin checks,
no CORS, bounded bodies and existing fixed compute route/schema. Extend the
bridge's serial processing to bounded concurrency: at most four live HTTP
handlers, at most one native compute and one native control/status operation;
excess work returns 503 `{"error":"BUSY"}`, with no queue or backend call.
Header/body socket bounds remain five seconds. Control calls have an absolute
five-second native deadline on a separate C12 connection, independent of
compute's existing bound. OFF must
not queue behind a 60-second compute operation. Admission counters must be
released on every timeout/error; no unbounded task spawning.

The UI reads the preference on connect/reconnect, polls every two seconds and
expires it after three seconds. During mutation show pending, not confirmed
OFF/ON; drop pre-mutation reads and prevent an older response overwriting a
newer one. On timeout mark unknown and reconcile through a new read. Runtime
readiness remains separate. A stale/missing preference cannot support a
Disabled claim. ERROR shows effective OFF plus a persistence warning, including
that restart may restore the previously saved ON. Never write desired browser
state back merely because the page loaded. Live compute action is disabled
while preference is OFF, unknown or pending; native callers retain local fallback.

## Required implementation evidence after approval

1. Exact C12 schema, descriptor rejection, same-UID rejection, old-daemon
   unsupported handling and no mutation on invalid input.
2. First-run default, OFF/ON across real daemon restart; corrupt/newer config,
   duplicate keys, wrong owner/mode, symlink, oversized and nonregular files.
3. Inject failures at write, file fsync, rename and directory fsync. Verify
   fail-closed live state, no false durable success, old/new complete storage,
   no silent repair and explicit retry/reconciliation semantics.
4. Authenticated OFF during discovery, lease/admission, upload, compute and
   reconnect; no subsequent remote request after invalidation, exact fallback
   digest and truthful source. Re-enable must acquire fresh authority.
5. GET/POST security and saturation tests; OFF handled while a compute request
   is blocked; budgets and counters recover on error without mutation retry.
6. UI fresh/stale, pending, failure, multi-tab polling, out-of-order responses
   and restart cases. No localStorage false OFF or synthetic READY.
7. Real isolated daemon/client/bridge smoke, native fast checks, targeted
   Rust/frontend tests, then existing CI. Physical phone proof remains separate.

The proposal checker validates only request/result/config vectors and proposal
consistency. It is NOT evidence of the unimplemented storage, HTTP or UI paths.
No MCP, new provider, Windows work, bulk browser upload or performance claim.
