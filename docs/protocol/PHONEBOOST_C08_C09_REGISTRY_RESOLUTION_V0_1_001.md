# PhoneBoost C08/C09 Registry Resolution V0.1 — 001

Status: **AUTHORITATIVE GENERAL POC ERRATUM**

## Scope

SPEC V0.7 requires `BUFFER_TOUCH` in the C09 provider API, while its PBMUX V1
registry assigns REMOTE_BUFFER values 1 through 7 without a TOUCH value. The
project owner resolves that internal omission as follows. Existing assignments
are not renumbered or reinterpreted.

The project owner additionally resolves the C10 native-operation admission
class required by Contract C10. This is an additive C08 resource-class
extension. It changes no RESOURCE message type, payload offset, or payload
length and does not reinterpret the existing RemoteBuffer class.

## C08 RESOURCE registry

| Value | Name | Host to worker | Worker to host |
|---:|---|---|---|
| 1 | `RESERVE` | request | invalid |
| 2 | `RESERVE_ACK` | invalid | terminal response to type 1 |
| 3 | `COMMIT` | request | terminal response to type 3 |
| 4 | `RELEASE` | request | terminal response to type 4 |
| 5 | `EXPIRE_NOTIFY` | invalid | unsolicited advisory notification |

Types 3 and 4 are direction-disambiguated request/response envelopes. There is
no generic RESOURCE result type.

### C08 resource-class registry

| Value | Name | Purpose |
|---:|---|---|
| 1 | `REMOTE_BUFFER_BYTES` | C09 RemoteBuffer backing storage only |
| 2 | `NATIVE_OP_SCRATCH_BYTES` | trusted native-operation/provider execution scratch only |

Zero and values 3 through 255 are unassigned. Class 1 semantics remain
unchanged. Class 2 uses the existing RESERVE, COMMIT, RELEASE, result, and
EXPIRE_NOTIFY envelopes. For `pb.native.blake3/1`, a class-2 request is nonzero
and no greater than 8 MiB.

One COMMITTED class-2 reservation backs exactly one compute job. Compute
admission atomically consumes it; terminal cleanup releases its held budget
exactly once. Failed admission leaves it COMMITTED and unconsumed. Bytes held
by a referenced class-1 RemoteBuffer are never charged again through class 2.

## C09 REMOTE_BUFFER registry

| Value | Name | Host to worker | Worker to host |
|---:|---|---|---|
| 1 | `ALLOC` | request | invalid |
| 2 | `ALLOC_ACK` | invalid | terminal response to type 1 |
| 3 | `PUT` | request | terminal response to type 3 |
| 4 | `GET` | request | invalid |
| 5 | `DATA` | invalid | terminal response to type 4 |
| 6 | `FREE` | request | terminal response to type 6 |
| 7 | `STAT` | request | terminal response to type 7 |
| 8 | `TOUCH` | request | terminal response to type 8 |

Type 8 TOUCH is the only registry extension. There is no generic REMOTE_BUFFER
result type. The canonical wrong-owner reason name remains
`BUFFER_NOT_OWNED`.

## Compatibility

This erratum changes no C05, C06, or C07 byte layout. It does not reassign
RESOURCE values 1 through 5 or REMOTE_BUFFER values 1 through 7. The additive
resource class 2 changes no C08 envelope layout. Implementations
that do not recognize REMOTE_BUFFER type 8 reject it as `UNSUPPORTED_MESSAGE`;
V0.1 General POC implementations implementing C09 must recognize it.

Exact V0.1 payloads, validation profiles, vectors, and failure scopes are
defined by `PHONEBOOST_C08_C09_WIRE_ADDENDUM_V0_1_LOCKED_001.md`.
