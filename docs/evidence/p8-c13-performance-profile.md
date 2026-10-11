# P8 C13 private atomic performance-profile persistence

Date: 2026-10-11

Branch: `feat/p8-c13-performance-profile`

Baseline: `73a3538f6d0d793cdf28bb5a74b5eab734192209`

## Bounded scope

This increment implements the approved D3 C13 persistence boundary. It does
not produce benchmark results, enable C11 SPEED or BALANCED placement, change
C12 ingress, or add an MCP surface. The C12 resource contract and MCP stop
conditions remain in force.

The pre-existing confirmation race found by the P7 duplicate CI run is carried
as a separate prerequisite commit. The C13 implementation itself remains a
separate commit and review surface.

## Persisted contract

- The typed JSON object exactly follows
  `phoneboost-perf-profile-v1.schema.json`; unknown and duplicate fields are
  rejected.
- Its complete compatibility key includes the SHA-256 peer ID, worker build,
  provider and operation IDs, both protocol versions, normalized transport,
  workload signature, thermal class, and battery policy.
- Benchmark provenance records the exact `pb-bench-v1` and
  `pb-link-metrics-v1` contracts plus the requester path. The public
  constructor fixes those contract identifiers rather than asking callers to
  reproduce them.
- Creation fixes expiry to exactly 86,400,000 milliseconds. Load invalidates
  at `now >= expires`, on wall-clock rollback, malformed or unsupported data,
  unsafe filesystem objects, and any complete-key mismatch.
- File IDs are the lowercase BLAKE3-256 digest of the deterministically
  serialized complete key; no raw peer public key or private identity is
  persisted.

## Filesystem guarantees

- The state root is absolute, owned by the current user, and contains no
  symbolic-link component.
- `phoneboost/perf/v1` is opened component by component with `NOFOLLOW`; each
  managed directory is owner-only mode `0700`.
- Profiles are bounded to 65,536 bytes and must be owner-only regular files in
  mode `0600`.
- Writes use an exclusive temporary regular file, full write, file flush and
  `fsync`, atomic same-directory rename, then parent-directory `fsync`.
- A post-rename directory-sync failure is reported as `DurabilityUnknown`, so
  callers cannot mistake an ambiguous durable outcome for failure safe to
  retry blindly.
- Reads are bounded and descriptor-based, reject links/nonregular files and
  mode or owner drift, and compare device, inode, size and modification time
  before and after the read.

## Executed evidence before checkpoint

- C13 persistence tests: PASS, 8/8, including the approved fixture, actual
  atomic replacement, exact expiry boundary, all ten key fields, malformed and
  unsupported data, links/nonregular objects, permissions, oversize input,
  unsafe directories, root links, and environment resolution.
- C11 planner regression tests: PASS, 14/14.
- Complete `pb-host` library suite: PASS, 195/195, including the unchanged
  authenticated 64 MiB remote proof.
- `cargo check --offline --locked -p pb-host`: PASS.
- Targeted Clippy: PASS; only pre-existing warnings outside changed C13 code
  were reported.
- Cargo-deny advisories, bans, licenses and sources: PASS. The allowed duplicate
  dependency report notes `syn` 2/3 because Serde derive currently uses `syn`
  3 while the pinned Curve25519 derive still uses `syn` 2.
- Agentic boundary checker: PASS, including the valid C13 fixture, eleven
  negative profiles, and unchanged C12/MCP stop conditions.
- Native repository `check-fast`: PASS, including 12/12 pipeline tests,
  actionlint, formatting, and `git diff --check`.

No physical-device run is claimed: this increment persists and validates
performance evidence but does not yet activate performance placement.

The checkpoint, push, pull request, and exact-head CI results are recorded only
after they execute.
