# P13 desktop ON/OFF software closure

Approved scope: PR #21 and
`docs/protocol/PHONEBOOST_DESKTOP_AUTO_USE_CONTROL_PROPOSAL_V0_1.md`.
Native checkpoint `ef1991c` passed GitHub Project pipeline run `38448125640`.
This follow-up completes the approved bridge and frontend; it does not merge
master, install a new application or change the user's phone/pairing state.

## Product result

The desktop switch now reads and writes the daemon's durable auto-use preference.
It no longer reads or writes `phoneboost.participation-enabled` in localStorage.
Without a live authorized bridge the control is unknown/disabled, not simulated
ON/OFF. A pending command has no confirmed state. OFF allows explicit native
local fallback but disables the browser demo compute action. ON is never READY
without separate current runtime evidence.

The existing bridge token/origin/Host rules cover both typed routes. Control
and status share one bounded lane; compute has a separate bounded lane. At most
four HTTP handlers exist, with RAII slot release and no queue. Saturation is
BUSY, not automatic retry. OFF can finish while a native compute call is blocked.
Completed compute observations are shared across handlers under a short mutex.

The UI serializes its periodic preference/status reads, expires preference at
three seconds and invalidates old read generations on mutation. It reconciles
lost POST replies by GET, never POST replay. Pre/during-mutation readiness is
also invalidated: an old READY response cannot reappear after the epoch change.
Storage ERROR remains effective OFF with an explicit restart/durability warning.

## Actual local validation

- Frontend unit/integration tests: PASS 338/338, seven suites. Includes strict
  result normalization, no localStorage authority, pending/late responses,
  mutation loss without replay, cross-client read updates, freshness expiry,
  and App-level stale readiness invalidation.
- Production frontend build with CI lint rules: PASS.
- Playwright through native pipeline wrapper: PASS 11/11, one worker. Browser
  routes use controlled fixtures; these are not physical/runtime evidence.
- Web bridge Rust tests: PASS 23/23, including exact/duplicate/array/mistyped
  input rejection, authorization, closed errors, lane saturation/recovery,
  four-handler budget and OFF completion during blocked native compute.
- Native preference tests: PASS 7/7. Added rejection of JSON sequence configs
  and a deterministic wrong-UID metadata gate test (no privileged chown claim).
- Host/client/bridge Clippy, all targets, no dependencies, warnings denied: PASS.
- Native check-fast: PASS; pre-commit-fast runs again at checkpoint.
- Existing native C12 and CLI evidence remains in
  `p13-durable-auto-use-native.md`: 25 handler, 37 validation, 28 client-library,
  2 CLI-command and 3 authenticated disable tests passed.
- Extended real-process smoke: PASS for isolated daemon + CLI + authenticated
  HTTP bridge, actual ON/OFF, daemon restart, malformed/unauthorized rejection,
  exact local fallback and invalid/newer config preserved fail-closed.
  This smoke is required by the Rust CI job and builds all three executables.
- Existing real-input import regression smoke: PASS at the native checkpoint.

No new physical Android test, performance proof, generic file/browser upload,
MCP, provider, Windows implementation or licensing change. Software scope is
ready for exact-head CI, then explicit default-branch merge approval. The full
application is not declared finished by this bounded control milestone.
