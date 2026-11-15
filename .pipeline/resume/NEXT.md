# Android waits physical checkpoint

Branch `spike/android-waits-v0-1`, base master `93f6446dd795ca32ebf0a40273b68e3ec7dbd09c` after merge commit #36. One writer; historical branches/worktrees intact.

Software and physical A/B complete. Baseline source a80f66a; candidate binary source572dc0e. Two warmups + three alternating pairs, all exact REMOTE_SUCCESS. End-to-end paired median gain11.80%,2/3 favorable; unfavorablepair retained. Both runs use Battery SaverON/profile1 under unchanged ResourceGuard. Combined two-wait change retained for review, not factorial attribution.

Report `docs/architecture/ANDROID_WAITS_V0_1.md`; public numeric evidence `docs/evidence/android-waits-v0-1.json`. Private campaign `/tmp/phoneboost-android-waits-20261115` contains credentials and MUST NOT be published. No more physical workloads. Cleanup verified: SaverON unchanged, profile1, diagnosticsOFF, experimentalapp stopped, ownedhost/forward removed, product APK unchanged.

Actual checks: runtime36+4doc, worker60+2doc, Clippy, both APK builds/lint, sixnewanalyzer tests, existing timing/power/evidencevalidators, fmt/actionlint. Publish evidence via project-pipeline/Gitleaks, open dedicated draft PR, wait CI/CodeQL. Do not auto-merge. No crypto/protocol/threshold/timeouts/C13/RAM/GPU/NPU/affinity change.

NEXT_ACTION = Quantify socket-wait, scheduling and local-read contributions inside Android WireReadBody (~726 ms), before selecting one reception optimization.
