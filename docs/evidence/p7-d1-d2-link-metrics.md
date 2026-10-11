# P7 D1/D2 normalized link and live-metric foundations

Date: 2026-10-11

Branch: `feat/p7-d1-d2-link-metrics`

Baseline: `c7b267a889ef3545a90ed580f6191d4c54ee1f51`

## Bounded scope

This increment implements the approved D1 transport-signature boundary and
the fail-closed D2 live-metric value types. It does not implement C13
persistence, enable C11 SPEED/BALANCED runtime placement, change C12 ingress,
or add an MCP surface.

D2 in this increment is deliberately a validated consumption seam, not a
runtime sampling claim. A future producer must supply authenticated
request/response and completed-payload observations from the secure session;
raw C04 transport counters cannot construct a live snapshot automatically.

## D1 implementation

- The exact classes are `wifi_lan`, `usb_network`, `ethernet_ip`, and
  `local_ip_unknown`, with exact `pb-link-v1:<class>` signatures.
- Discovery interface metadata is not treated as route proof. Classification
  occurs only after TCP connect, using the local address selected by the
  kernel to resolve one unambiguous interface.
- Linux sysfs metadata positively identifies wireless LAN, USB-backed network,
  or a physical PCI Ethernet controller. Missing, conflicting, loopback,
  bridge, virtual, tunnel, name-only, or unsupported evidence fails closed to
  `local_ip_unknown`.
- The route is re-resolved on every connection attempt. Link class remains
  metadata only and grants no trust, lease, admission, readiness, or fixed
  priority.
- Signatures never contain an IP address, port, interface name/index, MAC,
  SSID/BSSID, or device serial.

## D2 foundations

- TCP connect latency and raw pre-authentication byte rates are explicitly
  named as such and cannot masquerade as authenticated RTT or goodput.
- Authenticated RTT requires at least three successful samples and a nonzero
  median.
- Directional goodput requires a nonzero completed authenticated payload and
  at least one second of active transfer.
- A live snapshot is bound to peer ID, secure-session generation, normalized
  transport signature, and monotonic observation time.
- Validated consumption rejects age greater than or equal to six seconds,
  clock rollback, peer/session/link mismatch, and every missing required
  direction. Absence remains `None`; it is never converted to zero.

## Executed evidence before checkpoint

- D1/D2 transport tests: PASS, 11/11.
- Discovery regression tests: PASS, 3/3.
- Complete `pb-host` library suite: PASS, 187/187, including the unchanged
  authenticated 64 MiB remote proof.
- `cargo check --offline -p pb-host`: PASS.
- `cargo clippy --locked -p pb-host --all-targets`: PASS; only pre-existing
  warnings in dependency crates were reported.
- Native repository `check-fast`: PASS, including 12/12 pipeline tests,
  actionlint, formatting, and diff checks.
- Agentic boundary contract checker: PASS, including all D1/D2 and C13
  positive/negative contract fixtures.
- `git diff --check`: PASS.

The checkpoint, push, and exact-head GitHub CI results are recorded only after
they execute.

## First exact-head CI and security correction

GitHub Project pipeline run `38165468466` passed all 7/7 jobs at checkpoint
`044a1a1209dc96dac6f3b8b11c161dc183054cd9`. All four CodeQL language analysis
jobs also completed successfully, but the aggregate pull-request security gate
correctly reported one new high-severity alert: the direct traversal of the C
`getifaddrs` linked list could dereference an invalid pointer.

The raw `libc` traversal was removed rather than suppressed. Interface address
enumeration now uses the safe API from pinned `nix` `0.31.3`; interface-index
lookup remains in safe `rustix`. The route-selection and fail-closed behavior
are unchanged, and no `unsafe` block remains in the PhoneBoost D1 path.

Correction evidence before the follow-up checkpoint:

- D1/D2 transport tests: PASS, 11/11.
- Discovery regression tests: PASS, 3/3.
- Targeted Clippy: PASS with no warning in changed `pb-host` code.
- Cargo-deny advisories, bans, licenses and sources: PASS for the pinned
  dependency graph.
- `git diff --check`: PASS.

The corrected primary exact-head Project pipeline run `38166504245` passed all
7/7 jobs at checkpoint `73a3538f6d0d793cdf28bb5a74b5eab734192209`.
CodeQL run `38166509049` passed all four language analyses and its aggregate
gate; the pointer alert is absent.

An intentionally cancelled duplicate Project pipeline run was restarted to
restore its GitHub status. That duplicate exposed a pre-existing pairing-test
race: the secure-session thread could consume `confirm_requested` between two
immediate local confirmations, so the second call could be accepted rather
than classified as a duplicate. Confirmation acceptance is now retained for
the lifetime of the session while a separate one-shot dispatch bit prevents a
second wire confirmation. This changes neither the pairing protocol nor its
user-visible success semantics; it makes duplicate classification independent
of thread scheduling.

Pairing-race correction evidence before the follow-up checkpoint:

- Exact formerly failing test: PASS, 1/1.
- Complete `pb-runtime-secure` library suite: PASS, 24/24.
- Formatting and `git diff --check`: PASS.
- Targeted Clippy reaches only the existing `pb-pbmux` manual-range warning
  when warnings are promoted globally; no warning is emitted for the changed
  runtime code.

The final follow-up exact-head CI result is recorded only after it executes.
