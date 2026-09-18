# P1 quality tooling and automated user tests

Base: UI checkpoint `239fb1ca21209ba1c2dcd60cf17315130dfbdb90`. Branch: `chore/pipeline-p1-user-tests`. This phase changes pipeline automation, stable accessibility identifiers, and the desktop READY projection needed to preserve the existing fail-closed runtime truth. It does not create physical evidence or change master.

## Tooling and local commands

- `scripts/pipeline-p1-setup core trivy pre-commit maestro` installs pinned Linux x86_64 releases from official upstream URLs after SHA-256 verification. Generated tools and reports remain under ignored `.tooling/p1`.
- `scripts/pipeline-with-sccache cargo …` uses sccache when it starts successfully and otherwise runs Cargo directly. No functional build depends on the cache.
- `scripts/pipeline-security` runs cargo-deny advisories, bans, licenses and sources, retains a HIGH/CRITICAL Trivy JSON report, and gates only fixed CRITICAL findings. Trivy has a 15-minute network timeout for the approximately 114 MiB official database.
- `.tooling/p1/bin/pre-commit run --all-files` invokes the existing fast deterministic pipeline tests, actionlint when installed, formatting, and whitespace checks.
- `scripts/pipeline-playwright test` runs ten Chromium user journeys with one CI retry, failure-only screenshots, retained failure traces, bounded timeouts, and ignored local reports.
- `scripts/pipeline-maestro` validates both flows, then executes them and writes JUnit output when `adb` reports an authorized device. With no device it reports an explicit skip. For the Galaxy A15: enable USB debugging, authorize the host, install the debug application, verify `adb devices` shows `device`, then run `scripts/pipeline-maestro`.

## Automated user coverage

Playwright covers Control Center load, preference ON without inferred READY, reconnecting/non-ready truth, the complete READY predicate, distinct Local/Remote wording, collapsed advanced details, missing data, bridge failure, implemented navigation and a narrow viewport. The UI projection now requires authenticated session, active lease, fresh admission proof, available remote provider and available/ready auto-use status before rendering READY.

Maestro flows cover Worker launch, ON/OFF, ON not implying READY, visible runtime status, advanced details, bottom navigation labels and non-ready truth. Stable Android content descriptions identify the participation switch and advanced-details control.

## CI and fallback

The workflow keeps separate deterministic, Rust, frontend, Playwright, security/dependency, Maestro structure and Android jobs. Rust runs formatting, clippy, nextest and the existing cargo-test proof. The measured 64 MiB encrypted end-to-end Rust test needs about 130 seconds in an unoptimized build; only that test receives a 180-second nextest allowance, while all other tests retain the 30-second/2-period policy. Android uses the previously locked official API 37.0 toolchain.

If pinned P1 tools are absent, run the setup command. If sccache fails, the wrapper prints a fallback message and continues. If no Android device is attached, CI and local automation validate Maestro structure; this is not a physical run. Playwright does not substitute for native Android or hardware evidence.

## Findings and limits

Finding classifications are in `P1_FINDINGS.md`. No paid service or continuous LLM loop was added. The installer currently targets Linux x86_64, matching the local and GitHub runner environments. Physical Galaxy A15 execution remains a later operator action.
