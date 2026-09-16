# P0 validation and exact restart

Clean clone: /home/mimir/phoneboost-dev-pipeline-v1. Official base acacc0418a3ed8c8b06cec42365f1d6fe8e56c60; branch chore/dev-pipeline-v1. Master and old clones untouched. No existing product source/config file modified.

## Passed

- All seven requested VS Code extensions installed; Ollama inventory recorded without inference/benchmark.
- Python standard-library CLI doctor/status/check-fast/check-full/checkpoint/resume operational.
- 12 pipeline tests: protected/detached/wrong branches, index preservation, canonical hash changes, atomic state, missing scanner, staged synthetic-secret rejection, failed push preservation, real temporary remote cycle, read-only resume.
- Real GitHub cycle check-full --component pipeline -> checkpoint -> push -> resume at ad78f1a754115ef39ba9f1b8f624aa77982d9f01; resume confirmed remote HEAD equality and clean worktree.
- Local check-fast passed (including cargo fmt). Gitleaks scanned staged patches and full staged tree with no findings before each operational checkpoint. CI history scan passed. actionlint passed.
- CI frontend locked install, tests, build and existing CRACO lint passed. Rust formatting/clippy passed. Four CodeQL languages completed successfully on the first security run 35131237186. Final run results and exact commits are in ci-results.json.

## Failed / limitations

- Existing Rust pb-host startup tests hs_t20_unchanged_stale_socket_is_recovered, hs_t21_replaced_stale_socket_is_refused_and_survives and hs_t22_inode_change_is_refused fail on the GitHub runner. The first observes identical old/new inode; the other two observe unexpected READY. Evidence: https://github.com/lilugemusikmaking-design/phoneboost/actions/runs/35131236818/job/104912848220 . Files are unchanged from official checkpoint. Inode reuse/environment sensitivity is a hypothesis, not an accepted root cause. No skipping, weakening, or runtime fix applied. Cargo stops after failing target; subsequent workspace tests are not claimed passed.
- Android setup initially requested removed package tools; pipeline now explicitly requests platform-tools. Next SDK installation fails because platforms;android-37 is unavailable to the configured sdkmanager. Existing compileSdk 37, AGP 9.3.0, Gradle 9.5.0, NDK 29.0.14206865 retained. APK/JNI Android build and Android tests are configured but NOT validated by this run. No downgrade to make CI green. Local Java/Gradle/SDK are absent from PATH; local check-full reports missing prerequisites.
- Dependabot config is committed, but GitHub activates it from the default branch. Activation deferred to an independently authorized future merge; no merge performed.
- CodeQL Android Kotlin analysis deferred (requires build-aware setup); no false claim from Java no-build analysis. CodeRabbit/Grok review remains advisory and unexecuted; no required AI service.
- Canonical original documents are external/not-local. CI is SOFTWARE/CI PROOF only. PHYSICAL HARDWARE PROOF unchanged.

## Exact next action

Run scripts/project-pipeline resume in this clone. Stop new work. A separate authorized product investigation should reproduce pb-host hs_t20/21/22 and determine the intended trusted Android SDK 37 source before seeking green full CI. Do not start P1, apply reviewer findings automatically, or merge master. P1 candidates are in ROADMAP.md.
