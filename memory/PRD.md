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
