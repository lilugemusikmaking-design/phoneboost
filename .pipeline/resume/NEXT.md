# Android decrypt v0.1 — publication

- Branch: spike/android-decrypt-v0-1; base master5446240 (#35 merge commit).
- Current implementation source checkpoint3dcfdd32; retain target-scoped NEON config after physical proof.
- Micro ABAB: six retained samples/backend;279.765→213.768ms; exact host-generated fixture replay.
- APK8runs:2warmups+3pairs; all REMOTE_SUCCESS, exact digest, nominal Health, unchanged ResourceGuard.
- Decrypt667.408→570.166ms, process E2E3402.680→3293.384ms;3/3favorable pairs; AndroidCPU1190→1070ms.
- Same host/APK code/input/protocol/records. Config reproduces exact candidate APK685c05c1…; product installed hash unchanged.
- Local release security tests, build-guard/evidence tests, product Android pipeline, both APK lints, project-policy Clippy and fast pipeline passed. Extra -D warnings rejected a pre-existing warning, no unrelated fix.
- One writer; historical branches/worktrees preserved. No polling, ResourceGuard, C13, RAM, GPU/NPU, affinity or protocol changes.
- Phone: profile1/timingOFF, host stopped. Automatic Battery Saver restore failed explicitly; user requested to restoreON. Do not run another workload. Keep diagnostic Health alive until freshON verified, then stop isolated app/remove own forward. Journal /tmp/phoneboost-android-decrypt-20261115/power-state.json.
- Private evidence directory /tmp/phoneboost-android-decrypt-20261115 (contains credentials, never publish wholesale).
- NEXT_ACTION in this mission: verifyON cleanup, publish PR and await CI/CodeQL.
- Subsequent task only: address measured Android polling residual~634ms; do not start it now.
