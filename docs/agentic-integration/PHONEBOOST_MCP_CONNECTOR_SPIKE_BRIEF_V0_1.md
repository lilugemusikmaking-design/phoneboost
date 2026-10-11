# PHONEBOOST — MCP CONNECTOR SPIKE BRIEF V0.1

**Status:** FUTURE SPIKE — NOT AUTHORIZATION TO IMPLEMENT  
**Prerequisite:** close P6; define transport/perf-profile and C12 bulk-data boundaries first.

## Mission

Determine whether an optional local MCP adapter can expose PhoneBoost safely and usefully to Codex/VS Code and other MCP-capable development agents without creating a second authority path.

## Architectural rule

`pb-mcp` is an adapter/requester only:

`MCP client -> pb-mcp -> C12/phoneboostd -> C11 planner -> existing C07/C08/C09/C10 path -> Android ResourceGuard`.

It must never contact Android, PBMUX or a provider directly.

## Questions the spike must answer

1. Can the adapter remain a separate optional process with no daemon authority?
2. Is direct use of the C12 UDS preferable to reusing the `pb-cli` Rust library? Compare coupling, error fidelity, timeouts and testability.
3. What is the smallest safe read-only tool set (`status`, `capabilities`, `plan`)?
4. How should a future typed execute tool express placement intent without forcing remote placement?
5. How does the adapter represent execution provenance (`LOCAL`, `REMOTE_SUCCESS`, explicit fallback) without inventing its own truth?
6. How are tool arguments strictly schema-validated before C12?
7. How are model/tool prompt-injection-style strings prevented from becoming shell/path/network execution inputs?
8. What C12 bulk data mechanism is required for real developer files and how is workspace authorization represented?
9. What extra end-to-end latency does MCP add, and should that be recorded in benchmark configuration rather than the transport signature?
10. How can the adapter be removed completely with zero effect on PhoneBoost runtime behavior?

## Initial safe tool candidates

- `phoneboost_status`: read-only sanitized state.
- `phoneboost_capabilities`: read-only allowlisted typed capabilities.
- `phoneboost_plan`: pure/read-only planner output; no reservation and no remote mutation.

`phoneboost_execute` is **deferred** until a useful typed workload and safe C12 data-ingress profile exist.

## Forbidden tools

- shell/terminal execution;
- arbitrary path read/write;
- arbitrary HTTP/network request;
- raw PBMUX send/receive;
- raw RemoteBuffer read/write;
- raw lease acquire/release IDs exposed to the model;
- pairing/SAS auto-confirmation;
- arbitrary plugin/ELF/WASM execution.

## Acceptance criteria

The spike is successful only if:

- PhoneBoost core does not depend on MCP;
- adapter absence is behaviorally neutral;
- all remote execution still passes C11 and C08 authority gates;
- disconnect/ambiguity reporting remains identical to native C12 truth;
- pairing remains human-confirmed;
- no new provider is required for the initial spike;
- no persistent secrets or payload contents are exposed to the agent;
- transport-aware benchmarks can include MCP overhead without contaminating link-class identity.

## Deliverables

- one architecture decision record;
- one exact tool schema draft;
- one threat model / negative test matrix;
- one prototype only after those documents are reviewed.
