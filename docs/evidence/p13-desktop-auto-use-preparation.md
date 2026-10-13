# P13 desktop auto-use control preparation

Baseline: master `50633fe65c8d1568cc0555d606543dace11cf718`, clean at entry.
Branch: `docs/p13-auto-use-control-v1`. Preparation only; runtime unchanged.

## Finding and bounded output

Dashboard participation is localStorage-only. It can claim DISABLED while
phoneboostd remains enabled. The native controller already has enable/disable
with epoch invalidation, but no approved public method exposes that preference.
Existing `controller.acquire/release` are lease operations, not preference APIs.
The current serial bridge can also delay a future OFF behind synchronous compute.

The exact proposal is
`docs/protocol/PHONEBOOST_DESKTOP_AUTO_USE_CONTROL_PROPOSAL_V0_1.md`.
It covers typed C12 get/set, durable daemon preference, bounded bridge control
concurrency, truthful frontend reconciliation, and the required implementation
test matrix. It deliberately does not activate these changes before approval,
as required by the user-requested agentic handoff §11 stop conditions.

## Canonical consultation

Read-only text extraction of the project mirror's Word sources was used to
resolve C07 versus local preference ownership and C12/C13 persistence. The
Documents skill guided source inspection; its bundled dependency loader was
unavailable, so read-only unzip/XML text extraction was used. No Word artifact
was edited or delivered; no visual/layout audit is claimed.

- Contract Set V1.3, C07 §3, C12 §§2,7,12 and C13 §§2–6.
  SHA256 `205744f4ca500736c1c5b0cf013aa102e78ed49830a163a39f228eb570f76e0a`.
- SPEC V0.7, targeted local API/configuration references.
  SHA256 `a2bcd508bd2432c1edfa8bbdbf0dfb259e9dce55a6c836541da1f43a0ff327bd`.

No canonical source, authority hash registration or existing locked profile was
changed. The local preference/no-lease clarification is explicit in the proposal,
not silently treated as existing implementation authority.

## Actual validation

- `python3 scripts/check_desktop_auto_use_proposal_v0_1.py`: PASS, six test groups
  for exact request/result/config shapes and rejection vectors. This is a
  proposal oracle, not production C12, storage or browser evidence.
- `CARGO_TARGET_DIR=/var/tmp/phoneboost-p0-fo4lub9x/cargo-target CARGO_BUILD_JOBS=1
  cargo test --locked --offline -p pb-host disable_ -- --nocapture`: PASS,
  3 tests, 223 library tests filtered out; 0 selected daemon tests.
  Strengthened the existing upload/compute interruption test to require exact
  local digest and reject REMOTE_SUCCESS after the controlled interruption.
  Runtime logic is unchanged. This uses authenticated software harnesses,
  not a physical phone.
- `scripts/project-pipeline check-fast`: PASS (12 pipeline tests, actionlint,
  whitespace and workspace formatting).
- `scripts/pre-commit-fast`: PASS, including the new proposal checker,
  existing agentic boundary checks, pipeline tests, actionlint and formatting.

Full CI: not run at preparation checkpoint; consult published PR checks.
New persistence/HTTP/UI runtime tests: NOT RUN (implementation not authorized).
Physical phone: NOT RUN. No remote speed or completed desktop ON/OFF claim.

## Resume

Await explicit approval of the exact proposal. Then implement daemon persistence
and typed C12 first, CLI/bridge second, truthful UI third, and execute the listed
failure/concurrency/restart matrix. No phone is needed for those software steps.
Default-branch merge is a separate action requiring authorization.
