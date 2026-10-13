# PHONEBOOST — PRE-C13 / C12 BOUNDARY DECISION DRAFT V0.1

**Status:** PROPOSED FOR REVIEW — NON-NORMATIVE — NO IMPLEMENTATION AUTHORITY
**Date:** 2026-10-11
**Branch:** `docs/agentic-integration-decision-v0-1`
**Prerequisite:** P6 software, exact-head CI, and Galaxy A15 physical closure are complete.

## 0. Scope and stop line

This document prepares the decisions requested by:

1. `PHONEBOOST_AGENTIC_INTEGRATION_PREP_HANDOFF_V0_1.md`;
2. `PHONEBOOST_MCP_CONNECTOR_SPIKE_BRIEF_V0_1.md`.

It does not change a C04-C12 contract, add C13 persistence, add an IPC data
path, add `pb-mcp`, or authorize MCP implementation. Every item marked
**DECISION REQUIRED** remains stopped until it is explicitly approved and
converted into a separately reviewed contract.

The licensing/IP hold in the preparation handoff remains untouched.

## 1. Repository findings

The current implementation establishes the following facts:

- `TransportType` has only `LocalIp`; a candidate carries an endpoint and an
  optional interface index, but no normalized physical-link class.
- `TransportManager::connect` currently records TCP connect elapsed time in
  `rtt_ms`. This is connect latency, not a steady-session round-trip sample.
- current sustained TX/RX values are cumulative bytes divided by connected
  lifetime. The manager relinquishes its `TcpStream` at the secure-runtime
  handoff, so those counters do not describe the later authenticated workload.
- the live C11 CAPACITY path deliberately supplies zero bandwidth and RTT.
- the in-memory C11 key already contains `transport_signature` and
  `workload_signature`; every key change invalidates a profile, and the current
  TTL is 24 hours.
- C12/API-1 `compute.submit` accepts only the exact daemon-owned
  `pb.native.blake3/1` / `c10-abc-v1` fixture. Arbitrary bytes, paths, sizes,
  operations and bulk-data ingress remain rejected.

Consequently, neither the current C04 metrics nor the current C12 surface is a
sound basis for live SPEED/BALANCED placement or useful IDE offload.

## 2. Proposed decision summary

| ID | Proposed decision | State |
|---|---|---|
| D1 | Add a separate fail-closed normalized link observation and encode it in a versioned transport signature. | **DECISION REQUIRED** |
| D2 | Derive live cost inputs only from fresh, session-bound measurements; keep benchmark evidence separate. | **DECISION REQUIRED** |
| D3 | Persist one canonical, privacy-bounded C13 JSON profile per complete key using atomic private storage. | **DECISION REQUIRED** |
| D4 | For future bulk input, prefer FD transfer followed by a bounded daemon-owned immutable import object. | **DECISION REQUIRED** |
| D5 | Keep any future IDE/MCP process a C12 requester only; do not start it in this increment. | Restatement; no code authorized |

No proposal establishes transport priority. All placement remains metric and
policy driven.

## 3. D1 — normalized link identity and transport signature

### 3.1 Proposed internal model

Keep the C04 byte-transport kind distinct from physical-link observation:

```text
TransportType = local_ip

NormalizedLinkClass =
  wifi_lan
  usb_network
  ethernet_ip
  local_ip_unknown

LinkClassSource =
  os_route_interface_verified
  unknown
```

`NormalizedLinkClass` is metadata only. It grants no trust, pairing, lease,
admission, readiness, or placement authority.

### 3.2 Classification rules

1. Resolve the actual route interface selected for the connected endpoint.
2. Accept a known class only when OS metadata positively identifies that
   interface as wireless LAN, USB-backed network, or physical Ethernet.
3. Do not infer class from interface-name prefixes such as `wl*`, `usb*`, or
   `eth*` alone.
4. Treat bridges, VPNs, tunnels, virtual Ethernet, ambiguous interface stacks,
   missing metadata, and conflicting signals as `local_ip_unknown`.
5. Reclassify after route/interface change. A transport switch still requires
   the existing C04/C05 secure-session behavior.
6. Never persist raw IP address, port, interface name/index, MAC, SSID/BSSID,
   USB serial, Android serial, or route identifier.

### 3.3 Proposed exact signature grammar

```text
transport_signature = "pb-link-v1:" link_class
link_class          = "wifi_lan"
                    | "usb_network"
                    | "ethernet_ip"
                    | "local_ip_unknown"
```

Examples:

```text
pb-link-v1:wifi_lan
pb-link-v1:usb_network
pb-link-v1:ethernet_ip
pb-link-v1:local_ip_unknown
```

Exact byte-for-byte equality is required for profile reuse. `unknown` never
aliases a known class, and known classes never alias one another. A future
grammar revision must use a new version prefix and invalidates older keys by
ordinary key mismatch.

This uses the existing `transport_signature: String` field but changes its
canonical meaning and profile compatibility. It therefore remains a stop
condition requiring approval.

### 3.4 Required negative tests

- every known pair of link classes produces different signatures;
- ambiguous/virtual/interface-name-only evidence produces `unknown`;
- endpoint, SSID, MAC, serial, and interface identifier never appear in the
  signature or persisted profile;
- class change cannot reuse the former profile;
- class never changes trust, lease, admission, or fixed transport priority.

## 4. D2 — live measurement provenance and freshness

### 4.1 Separate two kinds of evidence

**Live link observation** supports one current C11 cost calculation. It is
process-local, session-bound, short-lived and not a C13 benchmark profile.

**Benchmark evidence** compares complete local and remote developer actions.
It is produced only by a dedicated benchmark flow and may become C13 data after
the D3 contract is approved.

Passive live observations must never silently manufacture benchmark runs or a
speed claim.

### 4.2 Proposed live snapshot

```text
LiveLinkSnapshot
  secure_session_generation: u64
  peer_id: PeerId
  transport_signature: ASCII string
  observed_at_monotonic: Instant
  rtt:
    source: authenticated_request_response
    samples: u16
    median_millis: u64
  tx_goodput:
    source: completed_authenticated_payload
    payload_bytes: u64
    active_millis: u64
    bytes_per_second: u64
  rx_goodput: same fields as tx_goodput
```

Rules:

- bind the snapshot to the exact authenticated peer, secure-session generation
  and normalized transport signature;
- measure RTT on authenticated application request/response correlation over
  the selected live stream, not from TCP connect elapsed time;
- measure goodput from successfully completed authenticated payload bytes over
  active transfer time; exclude idle lifetime and incomplete/failed transfers;
- use a monotonic clock for freshness and durations;
- never carry a snapshot across reconnect, peer change, route/class change,
  daemon restart, session replacement, or clock anomaly;
- preserve absence as `None`; never translate missing evidence into zero or an
  optimistic default.

### 4.3 Proposed runtime freshness gate

A live snapshot is usable only when all of the following are true:

1. peer, secure-session generation, and transport signature match current
   runtime truth;
2. age is strictly less than six seconds, matching the existing P6 capacity
   heartbeat freshness window rather than creating a looser authority path;
3. RTT has at least three successful samples from the current session;
4. a direction used by the cost model has a completed nonzero active window of
   at least one second in that direction;
5. no sample included an ambiguous result, failed transfer, reconnect, or
   session loss.

If any required field is absent or stale, SPEED fails closed. CAPACITY retains
its existing authenticated heartbeat gates and does not become dependent on
this draft. BALANCED must not become live until its exact missing-metric policy
is separately contracted.

The six-second and minimum-sample values are proposed contract values, not
implementation defaults.

### 4.4 Ownership

C04 owns bytes and transport observations. The secure runtime owns authenticated
session correlation. A narrow immutable snapshot may be presented to C11; C11
does not probe interfaces, own sockets, authenticate peers, or choose a
transport by class.

## 5. D3 — proposed exact C13 persistence schema

### 5.1 Storage and lifecycle

Proposed location:

```text
${XDG_STATE_HOME:-$HOME/.local/state}/phoneboost/perf/v1/<profile-id>.json
```

`<profile-id>` is lowercase hexadecimal BLAKE3-256 of the canonical UTF-8 JSON
encoding of the complete `key` object. It is an index, not a secret or an
authority token.

Proposed storage rules:

- private parent directories mode `0700` and profile files mode `0600`;
- write a private temporary file, sync it, atomically rename it, then sync the
  parent directory;
- reject symlinks, non-regular files, duplicate-key JSON, unknown fields,
  invalid UTF-8, values outside declared ranges, and files above 64 KiB;
- on malformed, unsupported, rollback, expired, or key-mismatched input: do not
  feed C11, report a bounded invalidation reason, and retain no payload content
  in logs;
- successful profile use never extends its original expiry;
- daemon restart reloads only fully valid profiles; partial temporary files are
  ignored and cleaned under the same private directory rules.

The exact canonical-JSON procedure and profile-id test vectors must be part of
the future locked C13 contract before implementation.

### 5.2 Exact proposed JSON shape

All object fields below are required. Unknown fields are rejected.

```json
{
  "schema_version": 1,
  "created_wall_ms": 1791744000000,
  "expires_wall_ms": 1791830400000,
  "key": {
    "peer_id_sha256_hex": "64 lowercase hexadecimal characters",
    "worker_build_id": "bounded ASCII build identifier",
    "provider_id": "bounded ASCII provider identifier",
    "operation_id": "bounded ASCII operation identifier",
    "core_protocol_version": 1,
    "provider_protocol_version": 1,
    "transport_signature": "pb-link-v1:usb_network",
    "workload_signature": "bounded versioned ASCII workload signature",
    "thermal_class": "nominal",
    "battery_policy": "external_power"
  },
  "evidence": {
    "local_warmup_complete": true,
    "remote_warmup_complete": true,
    "measured_local_runs": 15,
    "measured_remote_runs": 15,
    "local_median_duration_ns": 1000000000,
    "remote_median_duration_ns": 900000000,
    "remote_median_gain_basis_points": 1000,
    "bootstrap_95_lower_bound_basis_points": 500,
    "sustained_check_duration_ms": 300000,
    "thermal_severe_observed": false
  },
  "provenance": {
    "benchmark_contract": "pb-bench-v1",
    "benchmark_build_id": "bounded ASCII build identifier",
    "benchmark_config_id": "bounded versioned ASCII config identifier",
    "requester_path": "native_c12",
    "link_measurement_contract": "pb-link-metrics-v1",
    "median_rtt_ms": 8,
    "median_tx_goodput_bytes_per_second": 25000000,
    "median_rx_goodput_bytes_per_second": 30000000
  }
}
```

Exact enums proposed for version 1:

```text
thermal_class  = nominal | light | moderate | severe
battery_policy = external_power | battery_allowed | power_save | remote_disallowed
requester_path = native_c12 | pb_cli | mcp_adapter
```

`requester_path` records end-to-end orchestration overhead but is not an
authority dimension and is never added to `transport_signature`. To prevent
evidence gathered through materially different orchestration from aliasing,
the benchmark contract must define it as part of `benchmark_config_id` and the
versioned `workload_signature` derivation.

The persisted key mirrors the existing in-memory C11 key. `PeerId` is already a
SHA-256 digest; it is serialized as exactly 64 lowercase hexadecimal characters
without exposing the paired static public key itself.

### 5.3 Invariants and invalidation

- `expires_wall_ms` must equal `created_wall_ms + 86_400_000` exactly;
- wall-clock rollback (`now < created`) invalidates the profile;
- `now >= expires` invalidates the profile;
- any complete-key mismatch invalidates the profile;
- unsupported schema, signature, benchmark contract, or measurement contract
  invalidates the profile;
- sample counts and evidence must satisfy current C11 requirements before a
  profile can qualify SPEED; persistence never upgrades weak evidence;
- `thermal_severe_observed = true` remains disqualifying;
- profile contents contain no source path, payload bytes, payload digest,
  prompt/model text, SSID, address, serial, capability, lease ID, Noise key, or
  raw measurement trace.

Invalidation reasons are runtime diagnostics, not persisted mutable state. A
bad profile cannot be rewritten as successful evidence.

## 6. D4 — C12 bulk-data ingress spike

### 6.1 Option comparison

| Option | Strengths | Principal risks | Conclusion |
|---|---|---|---|
| `SCM_RIGHTS` FD transfer | No daemon path lookup; kernel transfers an open-file description; natural same-host AF_UNIX boundary; bounded metadata control message. | Ancillary-message truncation/FD leaks if mishandled; mutable source creates TOCTOU; Linux-specific; C12 framing change. | Preferred ingress primitive, not sufficient storage semantics alone. |
| Dedicated bounded binary stream | Portable framing concept; direct backpressure; no path parameter. | More framing/state/cancellation code; ties control and bulk lifetimes; partial stream/restart cleanup; copies are still needed for stable reuse. | Viable fallback, higher protocol surface. |
| PhoneBoost-owned spool/import object | Stable opaque handle; exact owner, TTL, quota and cleanup; clean future mapping to RemoteBuffer. | Unsafe if created from caller-supplied path; disk pressure and lifecycle complexity. | Preferred destination, only when populated from bytes/FD rather than a path. |

Linux `SCM_RIGHTS` transfers a reference to an already open file description,
not a pathname. Linux `memfd_create` plus seals can prevent later mutation;
unsealed shared memory otherwise permits time-of-check/time-of-use changes.
References:

- <https://man7.org/linux/man-pages/man7/unix.7.html>
- <https://man7.org/linux/man-pages/man2/memfd_create.2.html>
- <https://man7.org/linux/man-pages/man2/F_GET_SEALS.2const.html>

### 6.2 Proposed two-stage boundary

```text
authorized local requester
  -> opens or creates the input itself
  -> transfers one FD over the private same-UID AF_UNIX boundary
  -> phoneboostd validates and copies with strict quota/backpressure
  -> daemon seals/finalizes a private import object
  -> requester receives an opaque import handle
  -> a later typed C12 operation may reference that handle
  -> normal C11 -> C08/C09/C10 -> ResourceGuard path remains mandatory
```

The daemon never receives or resolves a caller-provided filesystem path.
Workspace authorization remains the requester's responsibility: an IDE adapter
may open only a file granted by its client/workspace policy. PhoneBoost then
enforces object type, byte/quota/lifetime and same-UID rules without claiming it
can reconstruct the IDE's workspace policy.

### 6.3 Proposed validation and object rules

- accept exactly one FD per import request; reject missing, excess, truncated or
  unexpected ancillary data and close all rejected descriptors;
- accept only a regular file or a correctly sealed `memfd`; reject directory,
  socket, device, FIFO and other special types;
- never trust a size supplied in JSON; derive size while copying and enforce a
  contract maximum plus per-UID aggregate quota;
- for an ordinary mutable regular file, copy once into a daemon-owned private
  object before validation/use; do not reread the caller's FD later;
- use bounded chunks and explicit backpressure; no whole-input heap allocation;
- assign a random opaque handle with no path meaning and no authority outside
  the same authenticated local requester context defined by the future C12
  contract;
- import state is `RECEIVING -> READY -> CONSUMED|CANCELLED|EXPIRED` and terminal
  states never resurrect;
- client disconnect during `RECEIVING` cancels and cleans the partial object;
- daemon restart deletes unfinished and, for version 1, all ready volatile
  imports rather than claiming resumability;
- worker/session loss does not corrupt the local import; the later typed
  operation follows existing truthful ambiguity and RemoteBuffer cleanup rules;
- cancellation is idempotent and never reports success for a possibly completed
  remote operation;
- logs contain only bounded state/reason/size-class diagnostics, never paths,
  payload bytes, content digests, FD numbers, opaque handles, or model-visible
  data excerpts.

No exact byte/quota/TTL values are assigned here. Those are resource/product
decisions required in the future locked C12 ingress contract. Until then there
is no implementation authority.

### 6.4 RemoteBuffer relationship

An import object is local request input, not local memory extension and not a
RemoteBuffer. If a future typed operation uploads it, C08 reservation and C09
RemoteBuffer creation remain separate, authenticated and ResourceGuard-bound.
The local object can be consumed/released only after the existing operation
truth makes that safe; it cannot grant remote residency or bypass admission.

### 6.5 Required negative tests before implementation can ship

1. path-shaped arguments and symlink/path lookup routes are absent;
2. wrong UID, special FD type, multiple FDs, ancillary truncation and oversized
   inputs fail closed without leaks;
3. mutation during ordinary-file import cannot change the finalized object;
4. disconnect/restart/cancel/quota failure removes partial data;
5. expired or terminal handles cannot resurrect or alias a new object;
6. imported bytes and identifiers never enter logs or MCP-visible diagnostics;
7. import does not allocate C08/C09 state before normal admission;
8. worker loss preserves existing C12 ambiguity/fallback truth;
9. an adapter cannot import outside its user-authorized workspace policy;
10. adapter absence leaves PhoneBoost behavior unchanged.

## 7. Deferred MCP work

This draft deliberately creates no MCP crate, dependency, tool schema, server,
or execution route. After D1-D4 are explicitly reviewed and locked, the next
separate spike may produce:

1. one adapter ADR;
2. exact read-only schemas for status, capabilities and plan;
3. an adapter threat/negative-test matrix;
4. only then, if still useful, a read-only prototype.

`phoneboost_execute`, a generic path, shell, network target, arbitrary RPC and
arbitrary code execution remain forbidden/deferred.

## 8. Approval surface

An approving review must decide each item independently:

- **D1:** approve/reject the four link classes and `pb-link-v1` invalidation
  behavior;
- **D2:** approve/reject authenticated session-bound sampling, six-second
  freshness, and minimum sample/window rules;
- **D3:** approve/reject the JSON shape, canonicalization work, private storage,
  requester provenance, 24-hour exact expiry, and fail-closed loading;
- **D4:** approve/reject FD-plus-private-import as the direction for a later
  exact C12 ingress contract.

Approval of this document would authorize contract/test drafting only unless a
later instruction explicitly authorizes product implementation.
