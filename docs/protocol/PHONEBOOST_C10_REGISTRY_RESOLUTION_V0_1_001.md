# PhoneBoost C10 Registry Resolution V0.1 — 001

Status: **AUTHORITATIVE GENERAL POC ERRATUM**

## Scope

The canonical chain assigns PBMUX channel 3 to COMPUTE and recognizes values
1 through 4, but does not assign their exact meanings or direction profiles.
The project owner resolves that byte-level omission without changing C05-C09
message registries or any PBMUX header field.

## C10 COMPUTE registry

| Value | Name | Host to worker | Worker to host |
|---:|---|---|---|
| 1 | `SUBMIT` | request | invalid |
| 2 | `STATUS` | request | nonterminal response or request failure |
| 3 | `RESULT` | invalid | terminal job/admission result |
| 4 | `CANCEL` | request | terminal cancellation response |

Direction disambiguates STATUS and CANCEL request/response envelopes. RESULT
may directly complete SUBMIT, complete an earlier accepted SUBMIT, or answer a
STATUS request for a terminal job. No fifth ACK or generic result type exists.

## Provider registry

| Provider ID | Version | Semantic identity |
|---:|---:|---|
| 1 | 1 | `pb.native.blake3/1` |

Provider ID or version zero is invalid/absent. Every other pair is unassigned
and produces the typed `UNSUPPORTED_PROVIDER` refusal. Provider identity is
not carried as variable UTF-8.

## Input registry

| Value | Name | V0.1 status |
|---:|---|---|
| 1 | `REMOTE_BUFFER` | assigned |
| 2 | `INLINE` | deferred/unassigned |

RemoteBuffer input is a nonzero buffer ID plus u64 offset and length. The
referenced bytes travel through C09 and are never copied into a C10 message.
Inline input is optional in the higher-level profile and is explicitly
deferred from this V0.1 wire lock.

## C08 dependency resolution

The accompanying authorized additive C08 resource-class registry is:

| Value | Name |
|---:|---|
| 1 | `REMOTE_BUFFER_BYTES` |
| 2 | `NATIVE_OP_SCRATCH_BYTES` |

C10 SUBMIT carries a COMMITTED class-2 reservation ID. Class 1 remains C09
storage authority and is never charged again for C10 execution. No C08 message
type, payload offset, or payload length changes.

## Compatibility

Existing COMPUTE values 1 through 4 are preserved and frozen as
SUBMIT/STATUS/RESULT/CANCEL. No C05-C09 message type is renumbered or
reinterpreted. Implementations recognizing the old numeric range but lacking
these exact C10 payload profiles must remain fail-closed with
`UNSUPPORTED_MESSAGE` rather than dispatching guessed bytes.

Exact layouts, validation matrices, vectors, and stateful semantic oracles are
defined by `PHONEBOOST_C10_WIRE_ADDENDUM_V0_1_LOCKED_001.md`.
