# PhoneBoost — Emergent Competition Control Center (PRD)

## Original problem statement
Build the PhoneBoost competition Control Center (web presentation layer) over the
imported repository `lilugemusikmaking-design/phoneboost`, tag
`competition-rc-20260824`, HEAD `51accc1a8fed188f2254f085004504735368b539`. Preserve
the native Android/Rust core. Never invent LIVE data, endpoints, READY states,
connected devices, metrics, RAM/CPU gains, or completed functionality. Every visible
fact must carry provenance LIVE / RECORDED EVIDENCE / ROADMAP; no silent fallback.

## Architecture
- FastAPI backend (`/app/backend/server.py`) acts as a Recorded-Evidence adapter
  over repo-anchored data in `/app/backend/phoneboost_data/`.
  Endpoints (all under `/api`):
  - `/system/snapshot` — five-gate ladder + endpoint panels, all UNAVAILABLE.
  - `/live/probe` — explicit `reachable=false` with reason + requirements.
  - `/evidence/index`, `/evidence/{id}` — 10 curated evidence cards, raw text
    served straight from the repo evidence files.
  - `/fixtures/manifest` — protocol fixture MANIFEST.json passthrough (31 files).
  - `/roadmap`, `/architecture`, `/release`.
- React frontend (`/app/frontend/src/App.js`, `/app/frontend/src/pages/Dashboard.js`)
  renders a single Control Center page: Computer + Phone endpoint panels, mode
  toggle (LIVE curtain when selected — never falls back), five-gate state ladder,
  secure link + remote capability panels, security callout, evidence grid + drawer,
  architecture layers, three-column roadmap.

## Preserved invariants (never violated in UI or API)
- Local vs remote never merged. RemoteBuffer / compute are ROADMAP objects.
- Five gates are independent: Paired · Authenticated · Controller lease · Resource
  admissible · Provider ready — never collapsed into one “connected” badge.
- LIVE mode fails explicit; recorded evidence is separately labelled.
- No SAS, private keys, session secrets, or unredacted diagnostics exposed.
- Native baseline `052471ed3cdbbe66a6c1f7b255f1d70580d91fcc` unchanged.

## Repo-anchored evidence surfaced
- Workspace 278/278 PASS · pb-types 2/2 · pb-pbmux 58/58 · pb-worker-core 43/43 ·
  pb-runtime-secure 15/15.
- C07 wire-addendum checker: PASS · 10 CMD/ACK vectors · 8 HEARTBEAT vectors · 5 oracles.
- Android ARM64 production core: offline release build PASS + fixture isolation + forbidden-authority-export scans.
- Android debug APK: `:app:assembleDebug` PASS, 36 gradle tasks, JNI fixture isolation PASS.
- Physical evidence files: a4-local-roundtrip, a5-android-worker, a6-lease-resourceguard, c04-local-ip-transport, c05-c06-secure-pairing.
- Protocol fixture manifest: 31 files.

## Test status
- Backend: 8/8 endpoint contracts verified (see /app/test_reports/iteration_1.json).
- Frontend: 15/15 UI expectations verified end-to-end on public preview URL.
- No fabricated LIVE data was observed anywhere.

## Truth/status/evidence refresh — 2026-06 (to GitHub master 162539c, verified)
- Refreshed the presentation layer to reflect current PhoneBoost master
  `162539c` ("Expose production auto-use BLAKE3") WITHOUT importing the native
  Rust/Android source tree into this workspace.
- Reviewed origin/master read-only (log 3f6f943..162539c): verified peer/session
  identity, authenticated C07 controller authority wiring, C08/C09 + C10 wire
  protocol locks, production RemoteBuffer + remote BLAKE3 modules, hardened
  transport resilience, Plug-and-Boost auto-use, Android lease-status fix.
- Truth model held strictly. Physical evidence set UNCHANGED (a4/a5/a6/c04/
  c05-c06). Per README@HEAD, C07 frames validate but do NOT mutate lease state;
  C05→C07 authorization seam is intentionally closed; RemoteBuffer/compute
  end-to-end remain ROADMAP and are NOT physically proven.
- backend/server.py: RELEASE.head → 162539c (baseline 052471ed kept as the
  validated test baseline); added evidence cards c08-c09-checker (28 vectors),
  c10-checker (17 vectors), c12-auto-use-blake3 (locked profile); updated
  ROADMAP (working/next/future) and remote_capability note.
- frontend/src/pages/Dashboard.js: banner relabeled master/baseline/validated;
  Remote capacity + Remote compute capability chips now "Wire-locked · e2e
  roadmap" (not Available/green). Evidence grid now 13 cards.
- Verified: production `yarn build` PASS; frontend E2E 100% (11/11), see
  /app/test_reports/iteration_4.json. NOT pushed, NOT republished.

## Final product-first pass — 2026-06 (presentation-only, verified)
- Reframed as a simple product: hero now "Your phone becomes a secure compute node
  for your Linux PC. / Plug it in. PhoneBoost puts it to work." Technical clarifier
  ("not RAM extension / swap / CPU illusion") moved into the secondary "How it works".
- Added a truthful primary control: "Enable PhoneBoost" switch is DISABLED / Off ·
  Unavailable with reason "Native runtime not reachable from this hosted browser" and
  an expandable "Why can't I enable it?" (reuses /api/live/probe). It never fakes LIVE.
- Simplified default viewport: "At a glance" cards (available? phone contributing?
  trust state? evidence?), a vertical Linux PC ↕ Secure link ↕ Android phone topology
  (phone reads as a distinct REMOTE NODE, never merged), and user-value capability
  cards (Secure link / Remote capacity=ROADMAP / Remote compute=ROADMAP / Evidence).
- Protocol-heavy detail (secure-link fields, host/worker endpoints, remote capability,
  architecture layers, security mono detail) moved into collapsed "Technical details".
- Preserved: five independent gates (no progress bar, no Connected/Ready collapse),
  evidence grid+drawer, roadmap, repo anchor / native baseline / validation date.
- Verified frontend E2E 100% (see /app/test_reports/iteration_3.json). No fabricated
  LIVE/connected/ready data. Backend and native Rust/Android files untouched.
- Preview left ready for review; NOT republished. Workspace is clean for GitHub export.

## Design pass — 2026-06 (UI/UX polish, design-only)
- Dark tactical redesign: charcoal/graphite base (#0A0A0A/#111), restrained neon
  green (#39FF14) reserved for RECORDED-EVIDENCE/verified accents, amber for warnings.
- Fonts: Chivo (display), Inter (body), JetBrains Mono (technical) via Google Fonts.
- Added sticky left side-rail nav (Overview, Local↔Link↔Remote, State Ladder,
  Evidence, Roadmap) with IntersectionObserver scroll-spy active state.
- Stronger hero identity + proposition + provenance chips; "Copy repo anchor"
  buttons (sidebar + hero) — implements the P1 backlog item.
- LOCAL↔SECURE LINK↔REMOTE rebuilt as one coherent 3-column topology with
  connectors; host and remote domains kept visually distinct (never merged).
- Five gates presented as independent numbered cards (no progress bar / no implied
  sequence); all states remain honest UNAVAILABLE.
- Premium evidence grid + slide-in drawer; roadmap color-coded (green/amber/neutral).
- Truth model fully preserved: LIVE stays explicitly unavailable, no fabricated data.
- Verified frontend E2E (see /app/test_reports/iteration_2.json): 8/8 flows pass.
- Files: /app/frontend/src/pages/Dashboard.js (rewritten), src/index.css, tailwind.config.js, public/index.html. Backend unchanged.

## Backlog / next tasks
- Native browser bridge: define a local presentation adapter that opens the
  private `phoneboostd` Unix socket via a signed native helper so LIVE mode
  becomes actually reachable on a paired machine.
- Producer for `system.status` LIVE payload conforming to the same
  `PhoneBoostDataSource` shape returned by `/api/system/snapshot`.
- Add a public "Judge tour" query param (`?tour=1`) that guides through gates →
  evidence → roadmap with keyboard shortcuts.
- Diagnostics view: render redacted PBMUX/heartbeat frame samples straight from
  the checked-in fixtures (never LIVE) with per-field explanations.
