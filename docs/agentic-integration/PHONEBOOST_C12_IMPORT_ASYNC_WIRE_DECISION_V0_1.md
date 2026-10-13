# PHONEBOOST — C12 ASYNCHRONOUS IMPORT WIRE DECISION V0.1

**Status:** LOCKED — PRODUCT OWNER APPROVED

**Date:** 2026-10-12

**Implementation branch:** `feat/p9-c12-import-ingress-v1`

## 0. Approval and boundary

The product owner explicitly approved the proposed asynchronous C12 import
schema on 2026-10-12 with the statement `schéma C12 async approuvé`.

This decision adds exactly three future API-1 methods. It does not activate
them by itself, add MCP, add a caller path, add a generic read/download API, or
permit generic execution. The existing C11 and C07-C10 authority path remains
mandatory for any later typed operation that consumes an import.

The exact machine-readable contract is
`contracts/phoneboost-c12-import-async-wire-v1.json`.

## 1. Why the import is asynchronous

The locked lifecycle permits cancellation while an object is `RECEIVING`.
The existing C12 connection loop processes requests sequentially, so a
synchronous copy would make an explicit cancellation request unreachable until
the copy had already finished. Version 1 therefore returns a connection-bound
handle after validation, quota reservation and receiver scheduling, while the
bounded copy proceeds independently.

The `imports.create` response records the accepted initial `RECEIVING` state.
It is not a claim that a later `imports.get` cannot already observe `READY`.

## 2. Exact methods

### 2.1 `imports.create`

- class: mutating;
- params: exactly `{}`;
- ancillary data: exactly one non-truncated `SCM_RIGHTS` descriptor;
- response result: exactly
  `{"state":"RECEIVING","import_handle":"<64 lowercase hex>"}`;
- the response is emitted only after source owner/type validation, handle
  allocation, object/count reservation and receiver scheduling succeed;
- no source size, path, timeout, destination, provider or operation parameter
  is accepted.

Missing or multiple descriptors are request-scoped `LOCAL_BAD_REQUEST` and all
received descriptors are closed. Truncated or unexpected ancillary data is a
connection-scoped framing failure and closes every received descriptor.

### 2.2 `imports.get`

- class: read-only and passive;
- params: exactly `{"import_handle":"<64 lowercase hex>"}`;
- ancillary descriptors: forbidden;
- live result is exactly one of:
  - `{"state":"RECEIVING"}`;
  - `{"state":"READY","input_bytes":N}`;
- retained terminal result is exactly one of:
  - `{"state":"CANCELLED"}`;
  - `{"state":"CONSUMED"}`;
  - `{"state":"EXPIRED"}`.

`imports.get` never extends a deadline or TTL, consumes an object, opens its
payload, allocates C08/C09 state, or triggers placement.

### 2.3 `imports.cancel`

- class: mutating;
- params: exactly `{"import_handle":"<64 lowercase hex>"}`;
- ancillary descriptors: forbidden;
- result is exactly
  `{"state":"CANCELLED|CONSUMED|EXPIRED","state_changed":true|false}`.

For `RECEIVING` or `READY`, `CANCELLED` with `state_changed=true` is returned
only after the receiver has stopped, its descriptors are closed, the private
object is absent and quota release is confirmed. This acknowledgement is
bounded by the locked five-second deadline. If cleanup is not confirmed, the
request returns `IMPORT_CLEANUP_UNCONFIRMED`; it never returns cancellation
success and the quota remains charged.

## 3. Handle and connection behavior

All three methods resolve a handle only inside the authenticated local
connection, peer UID and daemon incarnation that created it. Unknown handles
and correct handles presented outside that tuple both return the same
`IMPORT_NOT_FOUND` response. They do not reveal whether an object exists.

Connection close requests cancellation of every live import belonging to that
connection. A receiver owns its source and destination descriptors and must
acknowledge cleanup before explicit cancellation can succeed. Daemon restart
deletes all v1 receiving and ready files before serving requests.

## 4. Closed public errors

The methods use only these safe codes:

- `LOCAL_BAD_REQUEST` — invalid exact params or descriptor count;
- `IMPORT_NOT_FOUND` — unknown or out-of-authority handle;
- `IMPORT_LIMIT_REACHED` — an approved byte, count or receiver bound refused;
- `IMPORT_SOURCE_REJECTED` — source owner, type or memfd seals refused;
- `IMPORT_RECEIVING_TIMEOUT` — the fixed 60-second deadline elapsed;
- `IMPORT_CLEANUP_UNCONFIRMED` — cleanup was not confirmed within five seconds;
- `IMPORT_UNAVAILABLE` — bounded internal I/O or entropy failure.

Messages contain no handle, UID, connection identifier, path, descriptor
number, payload, excerpt, digest or exact payload size.

## 5. Consumption boundary

Version 1 exposes no `imports.read`, `imports.download`, `imports.consume` or
generic execution method. A later separately locked typed C12 operation may
consume a `READY` handle exactly once. Consumption remains internal to that
typed handler and still passes C11 plus the existing C07-C10/ResourceGuard
authority path.

## 6. Required implementation tests

1. the method allowlist expands from 15 to exactly 18 names;
2. create accepts exactly empty params plus one descriptor;
3. get/cancel accept exactly one lowercase-hex handle and no descriptor;
4. the create response always records the accepted `RECEIVING` transition;
5. get is passive and exact at every live and retained terminal state;
6. cancel during receiving stops and closes the worker before success;
7. cancellation timeout returns cleanup-unconfirmed and retains quota;
8. connection close cannot leave a reachable ready object;
9. wrong connection, UID, incarnation and unknown handles are indistinguishable;
10. terminal states never resurrect and their TTL/count remain bounded;
11. no generic payload read or consumption method exists;
12. existing C12 methods remain descriptor-free and behaviorally unchanged;
13. logs/events remain limited to bounded state, reason and size-class data;
14. absence of a requester leaves daemon behavior unchanged;
15. MCP remains absent.

## 7. Change control

Changing a method name, exact parameter shape, response state, public error,
descriptor rule, cancellation acknowledgement truth or consumption boundary
requires a versioned contract revision and explicit product-owner approval.
