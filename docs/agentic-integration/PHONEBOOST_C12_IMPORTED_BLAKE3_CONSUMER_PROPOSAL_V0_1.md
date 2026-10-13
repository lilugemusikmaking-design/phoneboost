# PHONEBOOST — C12 IMPORTED BLAKE3 CONSUMER PROPOSAL V0.1

**Status:** LOCKED — PRODUCT OWNER APPROVED

**Date:** 2026-10-13

**Branch:** `docs/c12-imported-blake3-consumer-v0-1`

## 0. Purpose and stop condition

This document locks the first typed consumer for a `READY` C12 v1 import.
It authorizes only the bounded product implementation described here and does
not authorize changing an existing locked method,
add generic execution, add payload read/download, add MCP, or weaken C11 and the
existing C07-C10/ResourceGuard authority path.

The proposal is intentionally a new method rather than a second parameter shape
for `compute.submit`: the existing `compute.submit` BLAKE3 fixture profile is
locked to exactly `{"op_id":"pb.native.blake3/1","fixture":"c10-abc-v1"}`.

The exact locked contract is
`contracts/phoneboost-c12-imported-blake3-consumer-v1.json`.

On 2026-10-13, the product owner explicitly approved it with: “J’approuve le
schéma exact `compute.submit_import` de la PR #18.” Implementation may proceed
within this document's boundaries.

## 1. Exact request

The API remains version 1 and adds exactly one proposed mutating method:

```json
{
  "api": 1,
  "id": "<opaque request id>",
  "method": "compute.submit_import",
  "params": {
    "op_id": "pb.native.blake3/1",
    "import_handle": "<64 lowercase hexadecimal characters>"
  }
}
```

Both members are required strings and no other member is accepted. Ancillary
descriptors are forbidden. Paths, inline bytes, base64, URIs, sizes, offsets,
providers, versions, endpoints, deadlines, placement modes and resource values
are not caller-controlled.

`compute.submit` remains byte-for-byte and behaviorally unchanged.

## 2. Validation and consumption order

The proposed handler performs exactly this order:

1. validate the exact request shape and reject any descriptor;
2. require both the immutable auto-use controller and import store contexts;
3. resolve the handle inside the authenticated connection, peer UID and daemon
   incarnation tuple;
4. require `READY` with `input_bytes` in `1..=134217728`;
5. atomically open, unlink and tombstone the private import as `CONSUMED` once;
6. pass the descriptor and exact byte count through C11 and the existing
   C07-C10/ResourceGuard BLAKE3 path;
7. return one terminal C12 response on the same local connection.

Malformed requests and unavailable runtime contexts do not consume an import.
`RECEIVING` returns `IMPORT_NOT_READY` without extending its deadline. A
zero-byte `READY` import returns `IMPORT_INPUT_REJECTED` and remains `READY` so
the requester can cancel it. Unknown, expired, cancelled, consumed and
out-of-authority handles return the same `IMPORT_NOT_FOUND` error.

The transition to `CONSUMED` is the point of no return. After it succeeds, a
local read failure, response loss, transport ambiguity or requester disconnect
cannot resurrect the import or make a second execution possible.

## 3. Bounded execution

The private imported file is processed from its already-open descriptor. The
handler never accepts or reconstructs a caller path and never allocates a
whole-payload buffer.

- C11 receives the exact input length and the same authenticated/fresh runtime
  observations used by the existing BLAKE3 auto-use path.
- Remote placement allocates and fills the existing C09 `RemoteBuffer` in
  existing `MAX_PUT_BODY` chunks before the existing C10 BLAKE3 submit.
- Local execution hashes in fixed 65,536-byte chunks.
- If remote execution is unavailable or ambiguous, the unlinked private file
  descriptor is rewound and the existing truthful local fallback is computed.
- All C09 data, reservations and C10 job state retain their current bounded
  cleanup and ambiguity rules.

The method creates no C12 job identifier, polling method, progress stream or
C12 cancellation surface. Its synchronous envelope does not reinterpret the
internal asynchronous C10 job lifecycle.

## 4. Exact terminal success

Success has exactly this result shape and never echoes the import handle:

```json
{
  "state": "COMPLETED",
  "op_id": "pb.native.blake3/1",
  "input_kind": "PRIVATE_IMPORT_V1",
  "input_bytes": 123,
  "digest_blake3_hex": "<64 lowercase hexadecimal characters>",
  "execution_source": "REMOTE_SUCCESS",
  "auto_use_reason": "READY"
}
```

`execution_source` and `auto_use_reason` use only the already locked C12
BLAKE3 strings. Only a proven remote result may be `REMOTE_SUCCESS`. Known
remote unavailability and ambiguous remote completion remain respectively
`LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE` and
`LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE`.

The digest is returned only to the authenticated requester as the requested
typed result. It is never written to logs, diagnostics or public errors.

## 5. Closed errors

The proposed method uses only:

- `LOCAL_BAD_REQUEST` — the exact method parameters or descriptor rule failed;
- `LOCAL_UNSUPPORTED_METHOD` — the build has no immutable auto-use controller;
- `IMPORT_NOT_FOUND` — the handle is absent, terminal or outside authority;
- `IMPORT_NOT_READY` — the authorized import is still receiving;
- `IMPORT_INPUT_REJECTED` — the authorized ready import is empty;
- `IMPORT_UNAVAILABLE` — bounded private-store/read failure.

Messages contain no handle, UID, connection identifier, path, descriptor,
payload, excerpt, digest or exact input size.

## 6. Idempotency and ambiguous outcomes

The method is deliberately at-most-once, not replayable. Repeating the request
after the atomic consume point returns `IMPORT_NOT_FOUND`; it never re-executes.
`imports.get` and `imports.cancel` continue to report the retained `CONSUMED`
tombstone to the owning connection according to their locked schemas.

Loss of the terminal C12 response is therefore an ambiguous caller outcome,
not permission to retry. It does not fabricate remote success and does not
reverse a possibly completed remote computation.

## 7. Required negative tests before implementation

1. the allowlist expands from exactly 18 to exactly 19 methods;
2. the existing `compute.submit` fixture schema remains exact and unchanged;
3. missing, extra, mistyped or unknown params fail before handle lookup;
4. every ancillary descriptor is rejected and closed;
5. cross-connection, cross-UID, cross-incarnation and unknown handles are
   indistinguishable;
6. `RECEIVING` and empty `READY` inputs are not consumed;
7. two concurrent submissions can consume and execute the same handle at most
   once;
8. consumption unlinks the private object and releases local quota exactly
   once while its open descriptor remains usable by that one operation;
9. streaming uses bounded chunks at 1 and 134,217,728-byte boundaries and
   never creates a whole-payload allocation;
10. C11 local placement, known remote failure and ambiguous remote completion
    preserve the existing exact execution-source truth;
11. connection/response loss after consume cannot resurrect or re-execute;
12. result and error shapes never echo the handle or leak payload metadata;
13. no public read/download/generic consume method appears;
14. no C11, C07-C10 or ResourceGuard bypass appears;
15. MCP remains absent and adapter absence remains behaviorally neutral.

## 8. Approval effect

Approval of this exact proposal authorizes a separate bounded product increment
plus the negative tests above. It does not authorize MCP, another
provider, arbitrary operation identifiers, batching, paths, reconnectable
handles, C12 jobs, public payload access, or a default-branch merge.

Any method name, parameter, result field, error, input range, consumption
point, streaming rule or authority change requires a versioned revision and a
new explicit approval.
