# F1/F2 final validation

Rust: inode reuse reproduced with unchanged hs_t20/21/22 on ext4, not tmpfs. Minimal O_PATH reference retained through recovery/fresh bind preserves all supplied C02/Tech Sheet/HS checks. All 32 startup tests pass on both filesystems. No test source changed. Scope/excerpt/evidence: F1.md and decisions/003-host-startup-excerpt.md.

Android: official Google package is platforms;android-37.0 revision 2. The old identifier platforms;android-37 was absent. Locked official tools, archive hashes, SDK revisions and shared local/CI installer repair the environment. compileSdk/targetSdk 37, AGP 9.3.0, Gradle 9.5.0, NDK 29.0.14206865 retained. JDK execution 21; product Java target 17 unchanged. The existing standalone Kotlin test task runs its five assertions; no empty JUnit task or suppressed test-discovery failure. Evidence: F2.md, android-toolchain.json and final-ci-results.json.

Mandatory local check-full: 10/10 commands PASS; 0 failed. Exact commands, timestamps and source HEAD: final-full-check.json. Local log: /tmp/pb-final-full.log. Latest GitHub pipeline and CodeQL are success / success at 6044c061fa3270e2ff0160374551721a4b880044. Final metadata-only checkpoint does not change tested source.

Failed commands: none.

Known reliability issue, outside these two fixes: one earlier CI run reported TransportLost in remote_blake3_64_mib_is_exact_when_practical; no unrelated runtime/test patch applied and no claim that flakiness was repaired. See F2.md for original evidence.

Local space issue was solved by relocating only generated tooling/Cargo caches to /var/tmp with ignored symlinks and an isolated Gradle cache. Old clones and personal data unchanged. APK JNI inclusion and SHA-256 are recorded in android-artifact.json if local APK exists. SOFTWARE/CI PROOF only; physical evidence unchanged. Master unchanged; P1 not started.

Exact next action: STOP. Run scripts/project-pipeline resume on next instruction; no P1 or merge.
