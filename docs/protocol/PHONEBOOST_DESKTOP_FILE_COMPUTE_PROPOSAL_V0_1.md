# Desktop real file BLAKE3 proposal V0.1

Status: LOCKED FOR IMPLEMENTATION — product-owner approved on 2026-10-15.
Approval: “vas y oui enchaines”, in response to the exact P14 contract request.
Date: 2026-10-15 Europe/Paris.
Baseline: master `800070285fc939016fcf4af766aab60fd2284e36`.

## Product decision

Let the user select one local file in the live desktop Control Center, explicitly
request BLAKE3, and receive its digest, byte count and actual execution source.
Keep the existing `abc` demonstration separate. Accept 1 to 134217728 bytes
(128 MiB), matching the approved native import ceiling. This is a useful input
path, not a speedup claim or transparent CPU/RAM pooling.

This proposal extends the browser ingress boundary only. It reuses
`pb_cli::compute_blake3_import` and the locked C12 `imports.create/get` and
`compute.submit_import` contracts. It adds no C12 method, provider, remote
protocol, persisted preference, MCP adapter, Windows support or placement mode.
Daemon planning, C07/C08 authorization and Android ResourceGuard remain final.

## Exact browser request

Add only `POST /bridge/v1/compute/blake3-file` on the existing literal IPv4
loopback listener. Require the existing in-memory capability, exact Host and
same-origin Origin and Fetch Metadata checks; preserve no CORS, CSP and no-store.
Authentication and route/header validation happen before accepting a bulk body,
allocating a payload object or calling the native client.

The body is raw bytes with exactly `Content-Type: application/octet-stream`
(case-insensitive media type, no parameters) and one decimal `Content-Length`
in the range above. The browser sends the selected File/Blob directly; it must
not load the entire file into a JavaScript ArrayBuffer or encode it as base64.
No filename, path, MIME metadata, URL, destination, operation selector or
placement override is sent as an API parameter. Reject multipart, JSON,
content encoding, transfer encoding, Expect, duplicate headers, queries and
fragments. A request cannot enqueue a second operation through trailing bytes;
one response closes the connection, with buffered excess bytes rejected.

Existing routes retain their exact schemas and 1 KiB body ceiling. The bulk
exception is scoped to this authenticated route, not a global BODY_LIMIT bump.
Headers remain limited to 8 KiB and 32 fields; responses remain within 64 KiB.

## Temporary input and admission

Reserve the existing single compute lane before reading the payload. The lane
is shared with the demonstration action, with no queue and no automatic retry.
Retain at most four HTTP handlers and the independent control/status lane, so
OFF does not wait for an upload or compute operation.

Stream into one anonymous Linux memfd, with a constant non-user-controlled name,
CLOEXEC, owner-only mode and seal support. Use at most 64 KiB copy buffers and
count bytes against the validated declared length. Do not create a named upload
file, public handle, upload directory or user-configurable storage path.
After exact completion, apply WRITE, GROW, SHRINK and SEAL seals and pass only
the descriptor to the existing typed client. If allocation, writing or sealing
fails, close the descriptor and release the lane without invoking compute.

This deliberately spends up to 128 MiB of temporary memfd capacity per bridge
process, in addition to the existing bounded daemon import copy. It is not a
zero-copy or constant-total-memory claim. Kernel backing may use swap; this
does not promise secure erasure or confidentiality from same-UID processes.
The browser may retain its own File backing. Multiple bridge processes retain
the existing same-UID residual trust model and daemon aggregate quotas.
Close the bridge descriptor on every terminal path; the daemon's existing
connection-owned import cleanup rules remain unchanged.

## Deadlines and uncertain outcomes

Use absolute monotonic budgets: 5 seconds for HTTP headers, 30 seconds for the
bulk receive phase with at most 5 seconds idle, at most 5 seconds to establish
the C12 connection, then the existing independent 60-second import-readiness
and 60-second terminal-response budgets, and 5 seconds for response writing.
The browser bounds its wait to 180 seconds. These are proposed resource limits,
not measured throughput guarantees; partial progress never extends a deadline.
Other routes do not gain longer timeouts.

The existing native import client currently connects before setting socket
timeouts. Implementation must bound that connection too, using the existing
P13 nonblocking-connect pattern and no automatic retry; do not hide it behind
an unbounded helper thread. This changes no C12 schema.

Before native submission, an incomplete body, timeout or disconnect closes the
memfd without compute. Once native work may have started, browser cancellation,
lost replies or invalid terminal results do not prove cancellation or failure
of the remote operation. Show outcome unknown, never resubmit automatically,
never substitute the previous digest, and never claim remote cleanup complete.
No cancellation endpoint or resumable upload is introduced. OFF keeps its P13
semantics; it does not forcibly cancel a local fallback. A later user-initiated
request is a new operation, not reconciliation of an unknown result.

## Exact terminal response and errors

HTTP 200 contains exactly the seven validated fields of the existing imported
compute result, with no handle, path, filename, raw native error or payload:

```json
{"state":"COMPLETED","op_id":"pb.native.blake3/1","input_kind":"PRIVATE_IMPORT_V1","input_bytes":3,"digest_blake3_hex":"6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85","execution_source":"REMOTE_SUCCESS","auto_use_reason":"READY"}
```

The byte count must equal the accepted HTTP payload length. Preserve the native
closed enums and digest validation, including REMOTE_SUCCESS requiring READY.
Both native local-fallback sources remain valid completed results and are shown
as local fallback, never as remote success. Unknown outcome is not HTTP 200.

Existing framing, authentication and method errors retain their existing safe
responses. After those checks, new-route errors are exactly `{"error":code}`:

| HTTP | Code | Condition |
| --- | --- | --- |
| 400 | BAD_REQUEST | Empty input or invalid bulk-specific header/shape |
| 411 | CONTENT_LENGTH_REQUIRED | Missing length |
| 413 | INPUT_TOO_LARGE | Declared length exceeds 128 MiB |
| 415 | CONTENT_TYPE_REQUIRED | Missing or unsupported media type |
| 408 | UPLOAD_TIMEOUT | Receive deadline/idle limit before native call |
| 503 | BUSY | Compute lane unavailable, no queuing |
| 503 | INPUT_STORAGE_UNAVAILABLE | memfd allocation/write/seal failure |
| 503 | NATIVE_REQUEST_FAILED | Native client rejected/unavailable, no safe terminal result |
| 504 | IMPORT_TIMEOUT | Existing native readiness deadline reached |
| 504 | OUTCOME_UNKNOWN | Native submission may have occurred but terminal proof is absent/invalid |

Generic pre-submit I/O errors may conservatively use NATIVE_REQUEST_FAILED;
they must never be labelled safe-to-retry. Unclassifiable post-submission errors
are OUTCOME_UNKNOWN. Never return raw native codes/messages to the browser.

## Desktop behavior

Expose the picker only in the local live bridge mode, not hosted evidence mode.
Validate nonempty size before sending. Show filename only as escaped local text;
do not persist the file, its name, digest or token to localStorage or logs.
Require a fresh confirmed ON preference and no pending request before starting.
ON is permission, not remote readiness; the eventual source comes from C12.

Show transfer/processing as pending without invented percentage or remote claim.
Clear the previous result on new selection/request and bind replies to the
current request generation. A late reply cannot populate another file's result.
Refresh does not replay a request. Missing/invalid replies show unknown or error,
not a retained success. The new file result stays in the page's own memory;
do not relabel the existing fixture-specific snapshot history as a file result.

## Required evidence after approval

| Requirement | Implementation boundary | Required proof |
| --- | --- | --- |
| No unauthenticated bulk allocation | HTTP header/auth split | Forged Host/Origin/token and oversized/duplicate/chunked requests make no memfd/native call |
| Exact bounded payload | Streaming reader and memfd | 1 byte, 128 MiB, empty, oversized, short/excess, stalled receive, write/seal failure; no retained descriptors |
| Immutable native source | memfd seal and typed client | Real daemon accepts sealed input and returns exact digest/byte count; no new C12 method |
| Independent OFF | Shared compute lane and control lane | OFF completes during blocked upload/compute; competing compute receives BUSY; permits recover on error |
| Honest terminal state | Typed native result and HTTP mapping | Each fallback source, malformed/late/missing terminal response and unknown result; no retry |
| Bounded lifetime | Connect/read/write deadlines | Full Unix backlog fails boundedly; slow trickle cannot extend upload; import/terminal timeout cleanup |
| Correct file association | Desktop request generation | Picker replacement, stale reply, reload, preference OFF/stale and hosted mode in component and browser tests |

Run native pipeline checks, focused Rust/frontend tests, real isolated
daemon/CLI/bridge input smoke and Playwright before exact-head CI. A later
physical phone proof must separately establish real remote file execution;
automated local fallback and historical `abc` proofs do not establish it.

## Approval and stopping point

The approved scope is this exact bulk browser profile, including temporary
memfd capacity and the HTTP parser, native client and UI. The agentic preparation
documents remain non-normative; their stop condition for a new bulk local IPC
path is respected here. If implementation needs different schemas, ceilings,
storage or cancellation guarantees, stop and request that change explicitly.
This approval does not authorize a master merge.
