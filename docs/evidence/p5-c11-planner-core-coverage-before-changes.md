# P5 C11 planner core coverage before changes

Date: 2026-10-11

Baseline: `765dbf1b1f679c15897812f7ebc6fe2eb8c10196`

Branch: `feat/p5-planner-c11`

## Authority

This increment uses the following read-only canonical project documents:

- `PHONEBOOST_CONTRACT_SET_V1_3_QR01A_QR01B_LOCKED.docx`, SHA-256
  `205744f4ca500736c1c5b0cf013aa102e78ed49830a163a39f228eb570f76e0a`;
- `PHONEBOOST_TECH_SHEET_V1_3_QR01A_QR01B_LOCKED_IMPLEMENTATION_HANDOFF.docx`,
  SHA-256 `c055ce00ebf31d95c52b0c7e5ea584a3726fd97f7451e5402cddc6f2dfb7b6b1`;
- `PHONEBOOST_PSEUDOCODE_CRITICAL_PATHS_V1_1_FINAL.docx`, section
  `17.15 PLANNER_DECIDE()`.

The locked contract defines C11. The label P5 is used only as the repository
milestone name for this bounded C11 increment; it does not add or reinterpret a
canonical requirement.

## Baseline finding

The host implemented C07 through C10 and the C12 auto-use BLAKE3 path, but no
C11 planner or performance-profile model existed. The existing auto-use path
therefore remains unchanged during this core increment.

## Requirement matrix

| Requirement | Baseline | Increment | Test or evidence | Status |
| --- | --- | --- | --- | --- |
| Modes `LOCAL_ONLY`, `CAPACITY`, `SPEED`, `BALANCED` | Absent | Typed host model | Exact planner unit tests | Implemented |
| Safety and live authority gates precede placement | Absent | Auth, READY, lease, metrics, provider, estimate, C08, thermal and battery gates | Refusal matrix unit test | Implemented |
| C08 refusal always overrides prediction | Absent | Fail-closed `resource_guard_admitted` gate | Refusal matrix unit test | Implemented |
| SPEED without a current valid profile is local | Absent | `PROFILE_UNKNOWN` or invalid result | Missing and invalid profile tests | Implemented |
| SPEED evidence gate | Absent | Warmups, 15 plus 15 runs, 5 percent median gain, positive bootstrap lower bound, 5 minute sustained thermal check | Boundary unit tests | Implemented |
| CAPACITY does not require positive speed | Absent | Explicit aware-workload and admitted-budget path | Capacity unit tests | Implemented |
| BALANCED deterministic rule tree | Absent | Capacity, speed, locality, then local | Rule-order unit tests | Implemented |
| READY resident bytes have zero upload cost | Absent | Saturating effective-upload calculation | Locality unit test | Implemented |
| Profile key dimensions and 24 hour TTL | Absent | Versioned key and fail-closed clock and TTL assessment | Key and TTL unit tests | Implemented |
| Persistent C13 profile store | Absent | No change | Not run | Deferred |
| Benchmark runner and bootstrap calculation | C12 method deferred | Evidence is consumed but not generated | Not run | Deferred |
| Runtime integration and placement events | Existing auto-use remains direct | No behavior change | Existing regression suite | Deferred |
| Physical performance claim | No current claim | No claim added | Not run | Not required |

## Scope boundary

This increment is a pure host decision core. It does not change PBMUX, C08
admission, C09 handles, C10 execution, Android, JNI, C12 request schemas,
existing fallback behavior, or published performance claims. Runtime wiring,
profile persistence, benchmark production and observability require separate
bounded increments because their schemas and lifecycle affect C12, C13, C14
and C16.
