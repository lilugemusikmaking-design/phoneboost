# PHONEBOOST — C12 IMPORT RESOURCE DECISION V0.1

**Status:** PROPOSED — EXPLICIT PRODUCT-OWNER APPROVAL REQUIRED

**Date:** 2026-10-11

**Branch:** `docs/c12-import-resource-contract-v0-1`

## 0. Decision requested and stop line

This document fixes the remaining resource and lifecycle choices needed before
the approved D4 direction can become a locked C12 ingress contract.

It does not change the C12 public request schema, add descriptor transfer or a
bulk-data path, add an MCP process, enable C11 SPEED/BALANCED, or create a new
provider. Product implementation remains stopped until this decision is
explicitly approved and converted into a locked contract with negative tests.

The exact machine-readable proposal is
`contracts/phoneboost-c12-import-v1.proposed.json`.

## 1. Recommended exact limits

| Limit | Proposed value | Reason |
|---|---:|---|
| Maximum bytes per import | 134,217,728 (128 MiB) | Equal to the existing C09/compute input ceiling; no larger object can become a current RemoteBuffer input. |
| Maximum live imports per requester connection | 4 | Bounds one client while allowing a small pipeline. |
| Maximum live imports per UID | 8 | Matches the existing C12 client cap and C09 per-lease buffer count. |
| Maximum aggregate imported bytes per UID | 268,435,456 (256 MiB) | Allows two maximum-size inputs but prevents eight maximum objects from consuming 1 GiB locally. |
| Concurrent `RECEIVING` per requester | 1 | Prevents one client from creating local I/O fan-out. |
| Concurrent `RECEIVING` per UID | 2 | Keeps daemon-wide copy pressure bounded. |
| Copy chunk | 65,536 bytes | Bounded streaming; at most 128 KiB of copy buffers at the UID concurrency cap. |
| `RECEIVING` deadline | 60,000 ms | Matches the existing C12 local-client idle bound; a stalled source cannot retain quota indefinitely. |
| `READY` TTL | 300,000 ms (5 min) | Matches the C09 default RemoteBuffer TTL without implying that an import is a RemoteBuffer. |
| Cancellation acknowledgement deadline | 5,000 ms | Cancellation cannot wait indefinitely; success is emitted only after local cleanup is confirmed. |
| Terminal tombstone TTL | 300,000 ms | Preserves idempotent cancellation/terminal answers for the same horizon as a ready object. |
| Terminal tombstones per UID | 256 | Bounds idempotency metadata under repeated requests. |

All byte accounting includes both `RECEIVING` and `READY` objects. Space is
reserved incrementally before each copied chunk; crossing any object or
aggregate bound fails before the excess bytes are retained.

## 2. Requester and handle boundary

An import is bound to the tuple:

```text
(authenticated local connection, peer UID, daemon incarnation)
```

The handle contains 256 bits from the operating-system CSPRNG, encoded as 64
lowercase hexadecimal characters. It is never reused, logged, exposed in
diagnostics, or accepted on another local connection. Disconnect cancels both
`RECEIVING` and `READY` imports owned by that connection.

This deliberately chooses containment over reconnect convenience. A future
requester that needs restart-resumable imports requires a new reviewed contract
rather than weakening version 1.

## 3. Descriptor acceptance

Exactly one descriptor is accepted through `SCM_RIGHTS` for an import request.
Missing, extra, truncated, or unexpected ancillary data rejects the request and
closes every received descriptor.

Accepted sources are only:

1. an ordinary regular file owned by the authenticated peer UID; or
2. a `memfd` carrying all four seals `F_SEAL_SEAL`, `F_SEAL_SHRINK`,
   `F_SEAL_GROW`, and `F_SEAL_WRITE`.

Directories, symbolic links, sockets, devices, FIFOs and every other type are
rejected. The daemon uses descriptor-relative metadata and copies from offset
zero without resolving or receiving a caller path. The JSON request cannot
contain a path, FD number, source size, timeout, destination, network target,
or caller-selected storage location.

An ordinary mutable file is copied exactly once into a daemon-owned object.
Later mutation of the caller file cannot change the finalized import. The
daemon never rereads the caller descriptor after finalization.

## 4. Storage and lifecycle

Version 1 uses a daemon-owned, disk-backed, volatile directory below the
private user cache root:

```text
${XDG_CACHE_HOME:-$HOME/.cache}/phoneboost/import/v1
```

Managed directories are owner-only `0700`; receiving and ready objects are
owner-only regular files `0600`. Components are opened descriptor-relatively
with no symlink following. Caller-controlled filenames never enter the store.

The exact lifecycle is:

```text
RECEIVING -> READY -> CONSUMED
          \-> CANCELLED
          \-> EXPIRED
```

- `RECEIVING` begins only after descriptor/type/owner validation.
- The 60-second deadline is monotonic and is not extended by progress.
- `READY` begins only after the bounded copy is finalized; its five-minute TTL
  is monotonic and never extended by lookup or use.
- Consumption is single-use and terminal. Concurrent consumers cannot both
  obtain the object.
- Client disconnect cancels and deletes all owned receiving and ready objects.
- Daemon startup deletes all version-1 receiving and ready objects; version 1
  makes no crash-resume claim.
- Terminal states never return to ready and handles never alias a new object.

## 5. Cancellation and ambiguous cleanup

Cancellation is idempotent. For `RECEIVING` or `READY`, the daemon stops copy
work, closes its source and destination descriptors, removes the private object,
and releases count/byte quota before returning cancellation success.

A repeat cancellation within the tombstone TTL returns `CANCELLED` with
`state_changed=false`. A consumed handle returns `CONSUMED`; an expired handle
returns `EXPIRED`; neither is reported as newly cancelled.

If cleanup cannot be confirmed within 5 seconds, the response is a bounded
cleanup failure, not cancellation success. The affected byte/count quota stays
charged until cleanup is confirmed or the daemon restarts. Blind retry never
creates a second object or releases ambiguous accounting.

This local cancellation says nothing about a later remote operation. Once a
typed operation consumes an import, existing C12/C10 ambiguity and cancellation
truth remains authoritative; import cancellation cannot manufacture remote
success or undo a possibly completed remote computation.

## 6. Backpressure and resource accounting

- Copy uses one fixed 64 KiB buffer per active receiver and never allocates the
  whole input in memory.
- At most two receivers run for the UID, so copy-buffer memory is at most
  128 KiB plus fixed metadata.
- Object count is acquired before copy starts. Byte quota is charged before
  every write and released exactly once on terminal cleanup.
- Size from JSON or initial `st_size` is never trusted as the final size.
- EOF finalizes the copied length; attempting byte 134,217,729 rejects and
  deletes the partial object.
- Quota refusal happens before C11, C08, C09 or C10 state exists.

## 7. Authority and privacy invariants

- An import is local request input, not Linux RAM/swap, a RemoteBuffer, a lease,
  a reservation, a placement decision, or remote admission.
- A later typed operation still passes C11 and the existing C07-C10 / Android
  ResourceGuard authority path.
- The daemon logs only bounded state/reason/size-class counters. It never logs
  paths, payload bytes, excerpts, content digests, descriptor numbers, handles,
  model prompts, or provider data.
- Workspace authorization remains above C12. A future IDE adapter may transfer
  only a descriptor opened under its user-approved workspace policy. PhoneBoost
  never accepts a path and does not claim to reconstruct that policy.
- Adapter absence remains behaviorally neutral.

## 8. Required negative tests before implementation

1. zero, two, truncated and unexpected ancillary descriptors close cleanly;
2. wrong UID and every special object type fail before quota or lifecycle state;
3. unsealed/partially sealed `memfd` fails closed;
4. byte 134,217,728 succeeds and byte 134,217,729 fails without retained excess;
5. requester, UID, object-count, aggregate-byte and receiver-concurrency limits
   are each exercised at equal and greater-than boundaries;
6. stalled input expires at exactly 60,000 ms despite partial progress;
7. ready lookup succeeds at 299,999 ms and expires at 300,000 ms;
8. disconnect, daemon restart, cancel, expiry and quota failure remove partial
   objects and release accounting exactly once;
9. ambiguous cleanup never returns success and keeps quota charged;
10. cancelled, consumed and expired handles never resurrect or alias;
11. the same handle on another connection or daemon incarnation fails closed;
12. mutable-source changes after finalization cannot affect imported bytes;
13. no path, payload, digest, descriptor or handle appears in logs/errors;
14. no import action allocates C08/C09 state or bypasses C11;
15. worker/session loss preserves existing C12 ambiguity and fallback truth;
16. absence of a future adapter leaves daemon behavior unchanged.

## 9. Alternatives deliberately rejected for version 1

- **Eight full-size objects / 1 GiB aggregate:** too much local disk pressure
  for an initial bounded profile.
- **Runtime-directory or `memfd` destination:** can consume tmpfs/RAM and work
  against PhoneBoost's host-pressure objective.
- **Reconnectable handles:** broadens same-UID bearer authority and requires a
  separate requester identity/recovery contract.
- **Caller paths:** reintroduces traversal, workspace-policy confusion and
  time-of-check/time-of-use hazards.
- **Streaming directly to RemoteBuffer:** couples local ingestion to remote
  availability and risks bypassing normal C11/C08 authority.

## 10. Approval effect

Approval of these exact values authorizes a separate locked C12 import contract
and deterministic contract/negative-test fixtures. Product implementation must
then occur in another bounded increment and must not add MCP at the same time.

Changing any byte limit, count, TTL, requester binding, accepted source type,
or cancellation truth after approval requires a versioned contract revision.
