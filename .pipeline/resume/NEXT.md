# Android decrypt v0.1 — closed measurement

- Branch: spike/android-decrypt-v0-1. Base master:5446240bedb630be93bdec4f619afee7a6b781c7 (#35 merged with merge commit).
- PR36 draft, no auto-merge. Source/proof checkpoint a5f01532; final checkpoint contains cleanup.
- Isolated ABAB:279.765→213.768ms; same independent fixture, exact plaintext, tamper/replay rejected.
- App:2warmups+3pairs; all REMOTE_SUCCESS64MiB/exact digest/nominal Health; unchanged ResourceGuard.
- Aggregate decrypt667.408→570.166ms, process E2E3402.680→3293.384ms;3/3 pairs favorable; CPU Android1190→1070ms.
- Standard target-scoped ChaCha20 NEON cfg; Snow/dependencies/protocol/PBMUX/polling/timeouts/CPU policy unchanged.
- Product Android pipeline, tests, APK lints, build guard/evidence tests, project-policy Clippy,22 pipeline tests/actionlint and Gitleaks passed. Extra strict Clippy failure was pre-existing and kept in evidence.
- Cleanup VERIFIED: initial SaverON restored manually after explicit OEM-command failure; Health power_save=true age2367ms; profile1/timingOFF; isolated app/owned host/ADB forward stopped; product APK unchanged. No further workload.
- Private durable archive:/var/tmp/phoneboost-android-decrypt-v0-1-evidence-20261115-3dcfdd3 (private logs/APKs; never publish wholesale).
- Historical worktrees/branches preserved; no stash or cleanup of historical work.
- Remaining this mission: await final PR36 CI/CodeQL and report.
- NEXT_ACTION after this task: traiter le polling Android résiduel mesuré (~634ms réception + retour handler), sans modifier la crypto ni le protocole. Do not start now.
