# Security scope

Gitleaks v8.30.0 scans exact index contents and staged patch before checkpoint, with redacted output and no exclusions. CI also scans reachable history. actionlint v1.7.7 validates workflows. Installation uses Go module checksums and version pins; installed tools live in ignored .tooling/pipeline-bin. No remote script is piped to a shell.

Dependabot covers Cargo, frontend npm/Yarn, Gradle and Actions. GitHub reads Dependabot configuration from the default branch: configuration here is ready for a later human-approved merge but automatic activation is not claimed, because this mission forbids merging master. No auto-merge or automatic finding fixes.

CodeQL uses supported build-free analysis for JavaScript/TypeScript, Rust, Python and Actions. Android Kotlin requires a separate build-aware setup; deferred rather than using Java no-build mode that omits Kotlin. Availability in a private repository depends on GitHub Code Security entitlement; do not purchase it automatically. Official support reference: https://codeql.github.com/docs/codeql-overview/supported-languages-and-frameworks/ . Results remain consultative.

No CodeRabbit/Grok automation or required AI review service. Manual independent review can be requested at a later milestone; P0 deterministic operation is independent.
