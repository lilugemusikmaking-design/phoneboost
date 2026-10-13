# PHONEBOOST — AGENTIC / IDE INTEGRATION PREP HANDOFF V0.1

**Date:** 2026-10-11  
**Status:** NON-NORMATIVE PREPARATION BRIEF — DO NOT IMPLEMENT BY ITSELF  
**Target reader:** Codex / developer agent working in the PhoneBoost repository  
**Repository:** `lilugemusikmaking-design/phoneboost`  
**Current development line reviewed:** `feat/p6-c11-runtime-capacity`

## 0. Purpose

Prepare PhoneBoost for a future local integration with development agents and IDEs (first candidate: a generic MCP adapter usable by Codex/VS Code and other MCP-capable clients) **without changing PhoneBoost authority, security, transport, compute, or product claims prematurely**.

This brief exists because C11 planner work is now becoming live and C13 performance-profile persistence is still separate/deferred. The integration seams should be made explicit **before persistence and benchmarking formats become costly to change**.

This brief is not authorization to add a new provider, arbitrary code execution, generic shell execution, new trust authority, or a cloud dependency.

---

## 1. Source precedence / non-regression

Preserve the existing PhoneBoost precedence and invariants. In particular:

1. SPEC V0.7 prevails.
2. Contract Set V1.3 follows.
3. Tech Sheet V1.3 follows.
4. Pseudocode Critical Paths V1.1 follows.
5. Fixture Generation Spec V1.0 follows.
6. Locked wire/profile addenda follow.

Current implementation evidence never overrides canonical authority.

Hard invariants relevant to this brief:

- Android `ResourceGuard` remains final resource-admission authority.
- Linux planner is a placement decision engine, never admission authority.
- RemoteBuffer remains an explicit volatile remote object, never Linux RAM/swap/POSIX memory.
- Remote compute remains explicit bounded jobs, never extra scheduler CPU cores.
- No false remote success after disconnect or uncertainty.
- Pairing remains human-confirmed XX/SAS + mutual authenticated confirmation; an IDE/agent must not become pairing authority.
- No generic shell, command line, arbitrary filesystem path execution, ELF/plugin, WASM/bytecode, or caller-chosen network target in General Compute.
- An external integration must never talk directly to Android/PBMUX/provider internals and must not bypass `phoneboostd`.

---

## 2. Current repository facts that matter

### 2.1 C11 is already real code

The repository now contains a deterministic planner core with:

- modes `LOCAL_ONLY`, `CAPACITY`, `SPEED`, `BALANCED`;
- targets `LOCAL_ONLY`, `REMOTE_CAPACITY`, `REMOTE_SPEED`, `REMOTE_LOCALITY`;
- fail-closed remote gates;
- 24-hour performance-profile TTL;
- profile keys including peer/device, worker build, provider/op, protocol versions, **transport signature**, **workload signature**, thermal class and battery policy;
- `CostInput` fields for bytes up/down, remote-resident bytes, compute units, link bandwidth, RTT, local/remote score, thermal, power policy and queue depth;
- SPEED evidence gates and locality rules.

### 2.2 P6 has started live C11 runtime wiring

The current P6 increment wires `PlacementMode::Capacity` into the existing BLAKE3 auto-use path using authenticated/fresh heartbeat data. It intentionally does **not** add C13 performance-profile persistence, SPEED/BALANCED runtime behavior, benchmark production, or a C12 schema change.

**Do not mix agent/MCP work into P6.** Finish P6 full pipeline, exact-head CI and required Galaxy A15 physical proof first, then start a separate bounded increment.

### 2.3 Transport abstraction is canonical, but current implementation is still collapsed

Canonical C04 / Tech Sheet allow local transports including:

- Ethernet/IP;
- Wi-Fi LAN;
- USB-network / USB tethering.

Selection is metric/policy driven; there is no fixed Ethernet > USB > Wi-Fi rule.

The current Rust transport implementation still exposes only `TransportType::LocalIp`. It already measures fields such as RTT, sustained TX/RX, reconnects and stability, but the underlying link class is not yet represented distinctly in the current type.

This matters before C13 performance profiles are persisted: a Wi-Fi benchmark must not silently become reusable as evidence for USB-tether or Ethernet, and vice versa.

### 2.4 Current C12 compute surface is intentionally too narrow for useful IDE offload

The production C12 BLAKE3 profile currently exposes only the exact immutable `c10-abc-v1` fixture (`abc`). It deliberately rejects arbitrary input bytes, paths, raw payloads, caller-selected sizes, generic application SDK behavior and bulk local data paths.

Therefore an MCP/VS Code connector built today could be a **safe integration proof**, but it would not yet provide a meaningful developer workload acceleration path.

---

## 3. Immediate architectural decisions to prepare before C13 locks in

### D1 — Normalize physical/link transport identity separately from socket endpoint

Add/define a stable, non-secret link classification usable by C11/C13 performance evidence.

Candidate conceptual values:

- `wifi_lan`
- `usb_network`
- `ethernet_ip`
- `local_ip_unknown`

Requirements:

- Do not infer trust from link type.
- Do not encode raw IP address, interface MAC, SSID, device serial, or ephemeral endpoint in the persisted performance key.
- Do not create fixed priority by name.
- Transport switch continues to create a new SecureSession as required by C04/C05.
- If link classification cannot be proven, use `local_ip_unknown`, never optimistic guessing.
- Performance profiles from different normalized link classes must not alias.

### D2 — Populate measured link inputs before SPEED/BALANCED become live

P6 currently feeds zero for `link_bandwidth_bytes_per_second` and `link_rtt_millis` because CAPACITY mode does not require performance evidence.

Before live SPEED/BALANCED or agent workload planning:

- define the measurement source for RTT and sustained TX/RX;
- define freshness and invalidation;
- define how C11 consumes the measurement without becoming transport authority;
- define whether measurements are passive session metrics or explicit benchmark artifacts;
- keep missing/unreliable metrics fail-closed for SPEED.

### D3 — Freeze C13 performance-profile schema with integration provenance

Before implementing persistence, specify the exact JSON schema for `perf/*.json` including at minimum:

- schema version;
- complete canonical C11 key;
- created/expiry timestamps;
- local and remote sample counts;
- medians / relative gain evidence;
- bootstrap confidence evidence needed by C11;
- sustained thermal evidence;
- benchmark build/config IDs;
- normalized transport signature;
- workload signature;
- invalidation reason handling.

Agent/IDE integration should **not** become an authority dimension by default. If adapter overhead materially changes end-to-end measurements, represent it through benchmark configuration/workload provenance rather than making the IDE itself an authority.

### D4 — Define a generic local integration boundary above C12, not beside it

Future adapters (MCP, VS Code extension, CLI wrappers, other IDEs) must be **requesters only**.

Preferred topology:

```text
Codex / IDE / MCP client
        |
        v
optional pb-mcp adapter
        |
        v
C12 local API / phoneboostd
        |
        v
C11 planner -> C07/C08/C09/C10 authority path
        |
        v
SecureSession / PBMUX
        |
        v
Android worker / ResourceGuard
```

Forbidden topology:

```text
IDE/MCP -> Android directly
IDE/MCP -> PBMUX directly
IDE/MCP -> provider directly
IDE/MCP owns lease/admission/trust
```

### D5 — Do not let the agent choose remote execution directly

An external model/IDE may express intent or a policy preference, but the host planner must make the placement decision.

A future local request should describe the workload and allowed mode, not claim the destination as truth.

Example conceptual intent:

```text
operation = typed allowlisted operation
placement_mode = BALANCED | SPEED | CAPACITY | LOCAL_ONLY
workload_signature = stable bounded identifier
input_handle = bounded PhoneBoost-owned handle
```

The agent must never be able to assert `REMOTE_SUCCESS`, force a stale lease, override ResourceGuard, or bypass planner gates.

---

## 4. C12 bulk-data gap — spike before useful developer integration

A useful development connector eventually needs to pass real input data, but current C12 explicitly forbids arbitrary paths and the locked BLAKE3 profile only accepts the three-byte daemon fixture.

Do **not** solve this with a generic path parameter or shell command.

Run a separate design spike comparing safe local same-UID ingress patterns, for example:

1. bounded SCM_RIGHTS file-descriptor transfer over AF_UNIX;
2. dedicated bounded local binary stream tied to an authenticated C12 request/handle;
3. PhoneBoost-owned spool/import object with strict size, lifetime, owner and cleanup semantics.

The spike must answer:

- How does the daemon receive bytes without trusting arbitrary filesystem paths?
- What are size limits and backpressure rules?
- How is an imported object named/owned/lifetimed?
- Can it map cleanly onto RemoteBuffer without claiming local memory extension?
- What happens on client disconnect, daemon restart, worker loss and cancellation?
- How are D2/D3 contents excluded from persistent logs?
- How is same-UID residual risk bounded?
- How does the adapter avoid reading files outside a user-authorized workspace?

No implementation until this boundary is separately contracted/locked.

---

## 5. Future MCP adapter shape

### 5.1 Packaging

Prefer an optional separate process/crate (working name only: `pb-mcp`) rather than embedding MCP protocol handling into `phoneboostd`.

Properties:

- optional; absence never changes PhoneBoost behavior;
- no cloud dependency;
- no Android/PBMUX imports;
- communicates only through the supported local client boundary;
- no new trust authority;
- inherits C12 same-UID residual model;
- can be killed/restarted without affecting daemon/worker authority state.

### 5.2 Initial tool surface — read-mostly

Candidate first tools after a dedicated integration profile exists:

- `phoneboost_status` — sanitized readiness / gates / current placement capability.
- `phoneboost_capabilities` — typed allowlisted operations and modes; no raw provider internals.
- `phoneboost_plan` — read-only placement estimate where evidence exists; never reserves resources.
- `phoneboost_execute` — only typed operations accepted by C12 and the normal authority path.

Do not expose pairing confirmation, raw controller lease IDs, private peer material, arbitrary RemoteBuffer contents, raw PBMUX, shell execution, filesystem browsing or generic RPC.

### 5.3 Human-presence actions

First pairing / SAS confirmation remains a Control Center / human flow. MCP may report `PAIRING_REQUIRED`; it must not silently complete trust establishment.

---

## 6. Developer-workload benchmarking contract to prepare

The future connector is useful only if PhoneBoost provides net value for specific workloads. Create a benchmark plan before making IDE-performance claims.

Matrix dimensions:

- host class / local CPU and memory pressure;
- Android worker/build;
- operation/workload signature;
- bytes uploaded;
- bytes already resident remotely;
- bytes returned;
- normalized link class: Wi-Fi / USB-network / Ethernet / unknown;
- measured RTT and sustained TX/RX;
- thermal class;
- charging/power-save state;
- queue depth;
- local E2E time;
- remote E2E time including transfer and orchestration;
- UI responsiveness / host pressure where relevant.

SPEED claims remain evidence-gated by the existing C11 rules. A connector must never create a new claim standard.

Important: benchmark the **end-to-end developer action**, not only Android compute time.

---

## 7. Test requirements for any future adapter

Minimum tests before a connector may be called product-capable:

1. adapter absent -> PhoneBoost unchanged;
2. malformed/unexpected tool arguments fail closed;
3. unsupported operation cannot reach C10;
4. pairing-required state remains human-gated;
5. worker disconnect during request never becomes remote success;
6. ambiguous remote result preserves existing explicit ambiguous-fallback semantics;
7. stale metrics cannot authorize SPEED/CAPACITY placement;
8. USB/Wi-Fi/Ethernet profile evidence never aliases incorrectly;
9. adapter restart does not mutate trust/lease/RemoteBuffer truth;
10. no secret/raw payload leakage to logs or model-visible diagnostics;
11. no arbitrary path/shell/network target route exists;
12. IDE/MCP client cannot bypass C11/C08.

---

## 8. Recommended implementation order

**NOW — finish current P6 only**

- full repository pipeline;
- exact-head GitHub CI;
- Galaxy A15 physical validation for the new authenticated heartbeat path;
- checkpoint/close P6.

**NEXT — bounded preparation increment**

1. transport-link classification + transport-signature contract/tests;
2. C11 measured link metric provenance/freshness design;
3. exact C13 perf-profile persistence schema;
4. no MCP code yet.

**THEN — local integration boundary spike**

5. C12 bulk-data ingress design (FD/stream/spool comparison);
6. integration adapter contract;
7. read-only MCP skeleton if still useful.

**ONLY AFTER A REAL DEV WORKLOAD EXISTS**

8. expose one useful typed developer operation;
9. benchmark local vs Wi-Fi vs USB-network vs Ethernet;
10. enable planner-backed execution through MCP/IDE;
11. make no speed claim until C11 evidence qualifies it.

---

## 9. Explicit non-goals for this preparation phase

- No new Android provider.
- No generic `run_shell` or command runner.
- No arbitrary source-tree execution on the phone.
- No remote compiler daemon claim.
- No claim that Codex itself runs on Android.
- No direct VS Code extension requirement yet.
- No MCP requirement inside core PhoneBoost.
- No transport preference by name.
- No new performance claim.
- No schema break to C05-C10/PBMUX.

---

## 10. Separate commercial/IP hold discovered during repository review

The root Cargo workspace currently declares:

```toml
license = "MIT OR Apache-2.0"
```

and core crates inherit `license.workspace = true`, while the repository metadata currently has no root GitHub-detected LICENSE file and the commercial strategy has treated the global PhoneBoost licence as unresolved.

**Do not modify, remove, publish, or rely on this licensing state as part of the integration work.** It requires a separate deliberate IP/licensing review because it may affect future licensing/cession strategy. This brief makes no legal conclusion about the effect of the manifest declaration.

---

## 11. Stop conditions for the developer agent

Stop and request an explicit decision before:

- changing C12 public request schema;
- adding a bulk-data local IPC path;
- adding `pb-mcp` to the workspace;
- changing the C11 performance-profile key;
- persisting C11 profiles under C13;
- distinguishing USB/Wi-Fi/Ethernet in a way that affects compatibility/invalidation;
- adding a new provider;
- changing any licence declaration;
- exposing a filesystem path, shell command, generic RPC or arbitrary code execution route.

The intended result of this document is to preserve the correct seams while PhoneBoost is still inexpensive to change, not to expand scope during an in-flight milestone.
