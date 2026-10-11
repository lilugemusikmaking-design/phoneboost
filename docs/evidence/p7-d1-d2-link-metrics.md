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
