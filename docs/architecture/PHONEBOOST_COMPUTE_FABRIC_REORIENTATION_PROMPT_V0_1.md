# PHONEBOOST — COMPUTE FABRIC REORIENTATION PROMPT V0.1

**Date:** 2026-10-15  
**Status:** STRATEGIC ARCHITECTURE REORIENTATION — IMPLEMENTATION HANDOFF FOR CODEX  
**Repository:** `lilugemusikmaking-design/phoneboost`  
**Baseline for this prompt:** `master` after PR #24, merge `1312cb1e7ddc40e71a9512d85663264eb63b30c6`  
**Target reader:** Codex running in VS Code on the active PhoneBoost workspace

---

# 0. READ THIS FIRST

You are resuming PhoneBoost while the product is already substantially implemented.

Do **not** treat this as a greenfield rewrite.

Do **not** discard the existing C05–C13 architecture.

Do **not** invent a new "V2" to throw away later.

The owner wants to **reorient now toward the direct long-term architecture**: a low-overhead compute fabric that can distribute useful work between heterogeneous systems without consuming so much RAM/CPU/latency that the offload defeats its own purpose.

The intended target is:

> **PhoneBoost Compute Fabric = a secure control plane plus a minimal execution plane built around NATIVE KERNELS, STREAM inputs and RESIDENT remote data.**

The project must continue to support:

- Linux x86-64 host today;
- Android ARM64 worker today;
- Windows host later;
- Wi-Fi LAN, USB-network/tethering and Ethernet/IP as local transports;
- explicit, truthful local/remote execution semantics;
- measured placement through C11;
- Android ResourceGuard as final remote resource authority.

The owner does **not** want to spend a year building intermediate architecture layers.

Your job is therefore to determine the **shortest safe migration from current P14 to the final low-overhead architecture**, then implement only bounded slices that directly belong to that final architecture.

---

# 1. CURRENT SESSION CONTEXT — DO NOT LOSE IT

The current working session, as observed immediately before this prompt, is around the post-P14 / installation path.

Latest repository truth:

- PR #23 merged P14 real desktop-file BLAKE3 support.
- PR #24 merged the Linux installer Cargo target selection fix.
- Current master merge: `1312cb1e7ddc40e71a9512d85663264eb63b30c6`.
- PR #24 pipeline was green and CodeQL was green.
- The active local workflow shown to the owner still contains installation/P14 resume state.
- The immediate prior next step was to validate the **installed** user path using a real file and a real phone, distinguishing genuine remote execution from truthful local fallback.

Before starting architecture work:

1. inspect the current worktree and local resume/checkpoint files;
2. preserve every uncommitted/local-only artifact;
3. do not overwrite `work/PHONEBOOST_INSTALL_RESUME.json`, `work/PHONEBOOST_P14_RESUME.json`, pipeline state or other local operational memory;
4. finish or explicitly checkpoint the pending installed real-file/phone validation if it is still the current owner-approved task;
5. do not mix strategic reorientation code into an installation hotfix or an unfinished physical-proof branch;
6. when clean, start a dedicated feature/architecture branch from the current validated master.

One writer per branch/worktree remains mandatory.

Never work directly on `master`.

---

# 2. ORIGINAL PRODUCT INTENT — RECENTER ON THIS

The original idea is not:

> "run BLAKE3 on a phone."

It is:

> **distribute useful compute/capacity across heterogeneous devices already available to the user, while remaining local, explicit, governed and truthful.**

The current implementation used BLAKE3 because it is an excellent deterministic proof workload.

That was correct for the POC.

The architectural mistake to avoid now is allowing:

> "one proof provider"

to become:

> "one custom mini-architecture per future workload."

BLAKE3 must now be treated as:

> **the first reference kernel / conformance workload of the general compute fabric.**

Do not plan a future where PhoneBoost accumulates dozens of unrelated provider-specific execution paths.

---

# 3. WHAT MUST BE PRESERVED

The existing architecture contains valuable final-system infrastructure.

Preserve it unless a canonical conflict forces a reviewed change.

## 3.1 Trust and truth

Keep:

- Noise XX + six-digit SAS first pairing;
- mutual authenticated confirmation;
- Noise IK reconnect;
- pinned peer identity;
- fail-closed session handling;
- no silent trust creation by discovery/USB/IDE/MCP.

## 3.2 Control authority

Keep:

- one active controller lease;
- Android-owned controller authority;
- Android ResourceGuard as final resource-admission authority;
- Linux planner estimates as non-authoritative;
- explicit lease/session/incarnation binding.

## 3.3 Remote data semantics

Keep:

- RemoteBuffer as explicit volatile remote state;
- no transparent Linux/Windows RAM claim;
- no swap claim;
- session/worker loss invalidates volatile truth;
- no hidden memory coherence.

## 3.4 Execution truth

Keep:

- bounded typed execution;
- no false remote success;
- ambiguous/lost result remains explicit;
- no automatic replay after unknown remote outcome;
- local fallback must remain clearly labelled;
- no "extra CPU core" claim.

## 3.5 Planner

Keep C11 as the single host placement decision layer.

Existing modes/targets and evidence gates remain important:

- LOCAL_ONLY
- CAPACITY
- SPEED
- BALANCED
- REMOTE_CAPACITY
- REMOTE_SPEED
- REMOTE_LOCALITY

Existing SPEED evidence discipline must not be weakened.

## 3.6 Transport

Keep:

- Ethernet/IP;
- Wi-Fi LAN;
- USB-network/tethering;
- normalized link signatures;
- measured selection;
- no fixed name-based priority.

## 3.7 Existing P7–P14 work

Treat the following as assets, not prototypes to discard:

- normalized link classes and live-metric foundations;
- C13 performance profile persistence;
- C12 private import resource;
- SCM_RIGHTS ingress;
- imported BLAKE3 consumer;
- native CLI import path;
- durable auto-use control;
- P14 real desktop file path;
- current web/Control Center boundary.

---

# 4. THE NEW TARGET ARCHITECTURE

The architectural target is a **PhoneBoost Compute Fabric** with two planes.

## 4.1 Control plane

Already mostly present:

```text
Identity / SecureSession
        |
Controller Lease
        |
ResourceGuard
        |
C11 Planner
        |
Transport metrics / profiles / observability
```

This plane decides whether remote work is allowed and useful.

## 4.2 Execution plane

Make this minimal and stable:

```text
                 PHONEBOOST COMPUTE ABI
                         |
              +----------+----------+
              |                     |
            STREAM                RESIDENT
              |                     |
              +---------+-----------+
                        |
                      KERNEL
                        |
             native host / native worker
```

The three key concepts are:

1. **KERNEL**
2. **STREAM**
3. **RESIDENT**

These should become the durable conceptual primitives.

---

# 5. KERNEL — NATIVE FIRST

## 5.1 Definition

A PhoneBoost kernel is a **pre-approved, typed, versioned bounded computation**.

Initial implementation should be **native code compiled for each supported target**, not an interpreted VM.

Conceptual form:

```text
same logical kernel ID
        |
        +-- Linux x86-64 native implementation
        +-- Windows x86-64 native implementation later
        +-- Android ARM64 native implementation
```

When practical, prefer the same Rust source compiled for the different targets.

## 5.2 Why native first

The owner explicitly wants to avoid paying a large runtime/memory tax merely to enable portability.

Therefore:

- no mandatory VM;
- no mandatory JIT;
- no dynamic arbitrary module loader;
- no runtime-loaded untrusted code in this reorientation;
- no WASM requirement for the core design.

A future WASM/AOT backend may be evaluated later as an **optional kernel packaging/execution backend** if benchmarks justify it.

It must not define the core ABI now.

## 5.3 Registry

The kernel registry must remain closed/allowlisted.

A kernel descriptor should conceptually contain only what the control/execution layer needs, for example:

```text
kernel_id
kernel_version
supported_input_modes
max_input / chunk constraints
max_output
scratch/resource class
deterministic or not
retry/side-effect class
cancellation capability
timeout class
local implementation available
remote implementation available
```

Do not accept caller-provided binaries, libraries, ELF, WASM or arbitrary code.

## 5.4 BLAKE3 role

Preserve `pb.native.blake3/1`.

Do not break current wire/API compatibility merely to rename it.

Internally, make it the first **reference kernel** behind the new abstraction where possible.

The first migration should ideally alter **internal abstraction, not external behavior**.

---

# 6. STREAM — THE KEY TO LOW RAM OVERHEAD

The current real-file path is secure and bounded, but the general architecture must not require a full materialized copy of every large input before useful remote work begins.

For stream-compatible workloads, total input size must not imply equivalent additional PhoneBoost RAM use.

## 6.1 Target property

For a stream-compatible kernel:

> **PhoneBoost-owned working memory should remain bounded independently of total input size, excluding ordinary OS page cache behavior.**

Example:

```text
regular file
   |
small bounded read window
   |
authenticated PhoneBoost stream
   |
small bounded Android window
   |
kernel incremental consume()
   |
bounded result
```

Transfer and compute should be able to pipeline where the protocol and kernel semantics allow it:

```text
transport: [chunk1][chunk2][chunk3][chunk4]
compute:          [c1]    [c2]    [c3]
```

Do not claim overlap until measured, but design for it.

## 6.2 STREAM kernel conceptual lifecycle

A useful conceptual ABI is:

```text
prepare(metadata, limits)
consume(chunk)
finish()
cancel()
```

This is only a design starting point. Reconcile it against C10/PBMUX before implementation.

Do not force every kernel into incremental semantics if it does not naturally support them.

## 6.3 Backpressure

STREAM must be bounded.

Define:

- maximum in-flight chunks/bytes;
- sender backpressure;
- receiver buffer cap;
- cancellation behavior;
- timeout behavior;
- exact terminal semantics;
- behavior when ResourceGuard changes during execution.

No unbounded queue.

No buffering the entire stream merely because the consumer is slower.

## 6.4 Replayability and fallback — critical

Streaming creates an important truth problem.

If remote execution fails after receiving part of a stream, local fallback can only restart safely if the original source is still replayable.

Therefore classify input sources.

At minimum distinguish:

- **REPLAYABLE_SEEKABLE** — e.g. an already-open stable regular file descriptor;
- **NON_REPLAYABLE** — pipe/live stream/etc., deferred until semantics are explicitly designed.

For a replayable regular file source:

- retain the local descriptor/source authority;
- local fallback may restart from offset zero using the same stable source if the execution result is known not to be successful remotely and existing truth rules allow fallback;
- do not require a second full in-memory copy merely to preserve fallback.

For a non-replayable source:

- do not silently promise local fallback;
- do not buffer the entire source just to fake replayability unless a separate bounded/spool policy explicitly chooses that tradeoff;
- ambiguous remote outcome remains ambiguous.

Current rejection of pipes/terminals/path arguments should remain unless a separate contract changes it.

---

# 7. RESIDENT — KEEP REMOTEBUFFER FOR DATA LOCALITY

RemoteBuffer remains useful.

Use RESIDENT mode when:

- the same data will be processed multiple times;
- transfer cost can be amortized;
- data locality creates value;
- the input naturally requires random access;
- the kernel cannot operate efficiently as a one-pass stream.

Concept:

```text
upload once
   |
RemoteBuffer
   |
kernel A
kernel B
kernel C
```

C11 already understands remote-resident bytes in locality decisions.

Do not replace RemoteBuffer with transparent memory.

Do not invent cache coherence.

RESIDENT is explicit remote ownership/lifetime.

---

# 8. DO NOT "PASS BELOW THE OS" BY DEFAULT — BUT AUDIT IT PROPERLY

The owner specifically asked whether going below the OS could remove enough overhead to make the fabric simpler/faster.

Do not dismiss this with hand-waving.

Perform a concise technical comparison.

Compare at least:

### A. current user-space local-IP architecture

### B. optimized native user-space Compute Fabric
- kernel ABI
- bounded streaming
- resident remote buffers
- minimized copies
- direct native execution

### C. custom kernel/driver path
- Linux kernel module / Windows driver / Android privileged path

### D. hypervisor / distributed-memory / transparent CPU-RAM approaches

For each, evaluate:

- bytes still required to cross the physical link;
- copy count;
- context-switch/syscall cost;
- RAM overhead;
- latency;
- portability;
- Android stock-device feasibility;
- Windows signing/deployment burden;
- security surface;
- crash blast radius;
- development time;
- maintainability;
- whether it actually removes a dominant measured bottleneck.

**Decision rule:**

Do not move below the OS unless measured/current-path analysis shows that user-space/software overhead is a dominant cost and a lower-layer path removes a material fraction of that cost **without destroying stock Android + Windows/Linux deployment viability**.

Physical transport latency/bandwidth cannot be optimized away by moving code into the kernel.

Document this comparison in the architecture decision.

---

# 9. CURRENT P14 DATA PATH — AUDIT THE COPY/MEMORY BUDGET

The reorientation must begin from measured reality.

Audit the exact path for a real selected file today.

At minimum map:

```text
browser/file or CLI FD
 -> bridge/native adapter
 -> memfd/import representation
 -> daemon private import
 -> local/remote decision
 -> RemoteBuffer / PBMUX
 -> Android input
 -> kernel scratch
 -> result
```

For every step record:

- owning process;
- bytes held;
- whether full input or window;
- whether copy, mapping or descriptor pass;
- lifetime;
- peak additional RAM;
- whether disk-backed/spooled;
- whether required for security correctness;
- whether removable;
- whether streaming can replace it.

Create a **copy/memory residency map**.

No optimization claim without this map.

For P14 specifically, pay attention to the current selected-file path where a sealed anonymous memfd may temporarily coexist with the daemon import representation.

That was a valid safety design.

It must now be evaluated against the final fabric goal.

---

# 10. PERFORMANCE GOVERNANCE

The fabric exists to add value, not merely to execute remotely.

Use end-to-end cost.

Conceptually:

```text
T_remote =
  source_ingress
+ queue
+ transfer_up
+ remote_compute
+ transfer_down
+ orchestration
```

versus:

```text
T_local = local_compute
```

For SPEED, keep existing C11 evidence rules.

Do not invent universal speed claims.

Also measure **host responsiveness/resource relief** separately from wall-clock speed.

A remote job can be useful even when it is not faster if CAPACITY mode intentionally keeps a constrained host usable.

Track at least:

- local wall time;
- remote wall time;
- upload bytes;
- download bytes;
- bytes already resident;
- measured link class;
- measured link throughput/RTT;
- host CPU pressure;
- host peak incremental RSS attributable to PhoneBoost;
- worker peak incremental memory where measurable;
- worker thermal state;
- charging/power policy;
- queue wait;
- orchestration overhead;
- execution source.

Do not merge these into a single invented score unless canonical C11 explicitly authorizes it.

---

# 11. MEMORY EFFICIENCY INVARIANTS FOR THE FINAL FABRIC

Propose and review exact numerical caps later, but the architecture should follow these qualitative invariants now:

1. STREAM mode must not require PhoneBoost to hold the complete input in RAM at both endpoints.
2. Input size and PhoneBoost working-RAM overhead must be decoupled for streamable kernels.
3. No hidden duplicate whole-file copy merely for convenience.
4. Full materialization is allowed only when the selected execution mode genuinely requires RESIDENT/random access or a documented security/lifecycle requirement.
5. ResourceGuard remains authoritative for remote buffers and scratch.
6. Planner-visible estimates must include execution-plane scratch/window costs.
7. Queueing may not create unbounded aggregate input retention.
8. Failure cleanup must reclaim buffers/windows exactly once.
9. No result may become visible before authoritative kernel completion.
10. Optimization must not weaken provenance or unknown-outcome semantics.

---

# 12. WINDOWS MUST BENEFIT FROM THIS REORIENTATION

Do not design the new abstraction around Linux-specific behavior.

The future Windows port should reuse:

- kernel IDs/descriptors;
- planner logic where platform-neutral;
- performance-profile concepts;
- execution-source semantics;
- streaming protocol;
- resident remote semantics;
- PBMUX;
- ResourceGuard contract.

OS-specific adapters should be concentrated around:

- local IPC;
- file/descriptor ingress;
- service lifecycle;
- discovery/network adapters;
- packaging/install;
- UI integration.

Do not make Linux filesystem descriptors part of the portable PhoneBoost kernel ABI.

They are a host-side ingress mechanism, not the distributed compute abstraction.

---

# 13. MCP / CODEX / IDE INTEGRATION — DOWNSTREAM, NOT CORE

Keep the previously defined rule:

```text
Codex/IDE
  -> optional adapter
  -> PhoneBoost local API
  -> Planner
  -> Compute Fabric
```

MCP must not become the compute architecture.

Do not prioritize MCP implementation before the final KERNEL/STREAM/RESIDENT boundary is clear.

Once the Compute Fabric API is stable, MCP can expose typed operations cheaply.

---

# 14. WASM / PORTABLE BYTECODE — OPTIONAL SPIKE, NOT FOUNDATION

A future portable-code backend can still be valuable.

But do not require it for this architecture.

Evaluate later:

- native compiled kernel;
- AOT WASM kernel;
- interpreted WASM kernel only if there is a compelling reason.

Benchmark:

- startup latency;
- steady-state execution;
- memory footprint;
- code size;
- cancellation;
- sandbox value;
- Android compatibility;
- Windows compatibility;
- ability to enforce kernel capability limits.

The native kernel ABI should remain usable even if the WASM spike is rejected.

No arbitrary user-supplied WASM in the General product without a separately approved security contract.

---

# 15. MIGRATION STRATEGY — NO BIG REWRITE

The preferred migration is additive and compatibility-preserving.

## Stage A — close current operational work

- preserve local resume state;
- finish current installed P14 physical journey if pending/approved;
- checkpoint/record truth;
- get back to a clean branch baseline.

## Stage B — architecture audit, no runtime change

Create:

1. `docs/architecture/PHONEBOOST_COMPUTE_FABRIC_DIRECTION_V0_1.md`
2. `docs/architecture/PHONEBOOST_DATA_PATH_COPY_BUDGET_V0_1.md`
3. `docs/architecture/PHONEBOOST_KERNEL_ABI_PROPOSAL_V0_1.md`
4. `docs/architecture/PHONEBOOST_STREAM_EXECUTION_PROPOSAL_V0_1.md`

These documents must be grounded in the actual current code.

## Stage C — internal kernel abstraction

If the audit shows no canonical conflict:

- introduce a small internal kernel abstraction/registry;
- place existing BLAKE3 behind it;
- preserve `pb.native.blake3/1` and all current external behavior;
- preserve existing test vectors and physical evidence meaning;
- no new wire message required merely for refactoring;
- no new public capability claim.

This is the preferred first code slice because it converts BLAKE3 from a special case into the first final-architecture kernel without adding scope.

## Stage D — stream design/protocol

Determine whether existing C06/PBMUX fragmentation and C10 machinery can support a true bounded incremental input path cleanly.

Prefer reuse.

Do not force reuse if it creates ambiguous ownership/cancellation semantics.

If a new wire/profile is necessary:

- isolate it in a separate locked addendum/profile;
- do not mutate existing BLAKE3 compatibility silently.

## Stage E — BLAKE3 as first STREAM reference kernel

Use BLAKE3 because it naturally supports incremental hashing.

Target:

- regular replayable file descriptor;
- bounded chunk/window memory;
- no full remote materialization needed;
- exact digest;
- truthful source;
- clean disconnect/cancel behavior;
- local fallback from original replayable source where allowed;
- no speed claim yet.

This becomes the conformance proof for STREAM.

## Stage F — compare STREAM vs current imported/resident path

Measure both paths on the same devices/transports.

Do not delete the existing path until evidence shows what each is best for.

It may be correct to retain both:

- STREAM for one-pass input;
- RESIDENT for repeated/random-access work.

## Stage G — only then add another kernel

Do not add a second operation merely for feature count.

Choose the next kernel specifically to prove that the ABI is genuinely general.

The second kernel should exercise a meaningfully different workload shape.

No implementation until the kernel ABI has proven that adding it is small and does not require another architecture.

---

# 16. QUESTIONS THE ARCHITECTURE AUDIT MUST ANSWER

Answer explicitly, with code references.

1. Which parts of current C10 are generic enough to become a kernel execution contract?
2. Which parts are hardcoded specifically to BLAKE3?
3. Can the existing provider ID/version model become the kernel registry without wire breakage?
4. Can the same Rust kernel implementation be compiled for Linux local fallback and Android remote execution?
5. Where exactly are full-input copies made today?
6. What is current peak extra RAM for 1 MiB, 64 MiB and 128 MiB inputs in each path?
7. Which copies are required for safety versus implementation convenience?
8. Can an open regular file FD remain the replay authority while chunks are streamed?
9. Can STREAM be implemented with constant/bounded memory while preserving current unknown-outcome semantics?
10. Can C11 estimate STREAM transfer cost before full input transfer?
11. How should kernel scratch be declared to ResourceGuard?
12. How does cancellation reach an incremental kernel?
13. Does C10's current 30-second model need a distinct stream timing profile, or can it remain compatible?
14. What is the smallest wire change, if any?
15. How do current queue limits map to stream jobs?
16. How is backpressure enforced across host/PBMUX/worker?
17. Can a slow phone begin consuming before upload finishes?
18. How do we guarantee no result publication before complete input + successful finalization?
19. How does RemoteBuffer remain useful for RESIDENT mode?
20. What architecture can Windows reuse unchanged?
21. What would a below-OS design actually save in measured/copy terms?
22. Which current components become unnecessary after the Compute Fabric abstraction, if any?
23. What is the minimum migration that yields the final architecture rather than an intermediate throwaway design?

---

# 17. SECURITY / SAFETY NON-GOALS

This reorientation does **not** authorize:

- arbitrary shell execution;
- arbitrary commands;
- caller-provided ELF/shared library;
- dynamic plugin execution;
- arbitrary bytecode/WASM;
- filesystem path RPC;
- arbitrary network destination;
- raw PBMUX exposure;
- direct Android access by local clients;
- bypassing C11;
- bypassing C08;
- transparent distributed RAM;
- transparent CPU migration;
- kernel module / root / custom ROM dependency;
- Windows kernel driver dependency;
- dynamic code download;
- cloud control plane;
- multi-phone scheduling;
- new AI provider work.

If any proposal requires one of these, stop and justify it separately.

---

# 18. TEST PHILOSOPHY

For every implementation slice:

## Software

- deterministic unit tests;
- negative/fail-closed cases;
- exact schema tests;
- bounded memory/queue tests;
- cancellation tests;
- malformed/duplicate/replay tests where relevant;
- loss/unknown-result tests;
- cargo fmt/clippy/tests;
- full repository pipeline proportional to changed scope;
- CodeQL/security gates.

## Physical

When execution/transport behavior changes:

- Galaxy A15 physical proof;
- genuine remote success;
- deliberate transport loss;
- no false remote success;
- authenticated recovery;
- memory observation;
- thermal observation;
- at least Wi-Fi and USB-network when both are practically available;
- Ethernet where hardware is available.

Do not convert CI into physical evidence.

---

# 19. REQUIRED OUTPUT BEFORE LARGE IMPLEMENTATION

Before writing a major stream/runtime patch, produce an architecture report with this exact structure:

## A. Current-state map

- current P14 execution path;
- current import path;
- current C09/C10/C11/C12/C13 roles;
- current copy/memory path.

## B. Final target

- control plane;
- execution plane;
- KERNEL;
- STREAM;
- RESIDENT;
- Windows reuse boundary.

## C. Options compared

- current provider-per-operation;
- native kernel ABI;
- WASM/AOT;
- below-OS/kernel-driver;
- hypervisor/transparent distributed resource.

Score each on:

- performance overhead;
- RAM overhead;
- security;
- Android feasibility;
- Windows feasibility;
- development time;
- maintainability;
- compatibility with existing PhoneBoost.

## D. Chosen direction

State why.

## E. Compatibility plan

List what remains unchanged.

## F. Required contract changes

List exact contracts/addenda affected.

## G. Migration commits

Propose the smallest ordered commits.

## H. Stop points

Identify decisions requiring owner approval.

---

# 20. AUTONOMY RULE FOR CODEX

You may proceed autonomously through **read-only audit and document drafting**.

You may also implement a purely internal kernel abstraction around existing BLAKE3 **only if all of the following are true**:

- no public/local API schema change;
- no wire change;
- no behavior change;
- no trust/authority change;
- current BLAKE3 IDs and results remain exact;
- current tests remain valid;
- the change clearly belongs to the final target architecture.

Stop for owner approval before:

- introducing STREAM on the wire;
- changing PBMUX/C10 layouts;
- changing C12 public/local methods;
- changing persisted C13 schemas;
- adding a new kernel/provider;
- adding WASM;
- adding any below-OS component;
- changing any performance claim;
- deleting the current import/RemoteBuffer path;
- changing licensing.

---

# 21. SUCCESS CRITERIA FOR THIS REORIENTATION

This mission is successful when PhoneBoost has a credible direct path to:

```text
APPLICATION / IDE / CLI
          |
        C12
          |
      C11 Planner
          |
    +-----+------+
    |            |
  STREAM       RESIDENT
    |            |
    +---- KERNEL-+
           |
    native execution
           |
    ResourceGuard
           |
 Wi-Fi / USB / Ethernet
```

with these properties:

- no mandatory VM;
- no full-input RAM duplication for streamable workloads;
- no provider-specific architecture explosion;
- native execution on heterogeneous ISAs;
- existing trust/safety preserved;
- future Windows host fits naturally;
- measurable overhead;
- truthful fallback and unknown-result semantics;
- BLAKE3 remains a reference proof, not the whole product;
- adding the second kernel should be a small extension of the ABI, not a new subsystem.

---

# 22. FINAL PRODUCT PRINCIPLE

When choosing between two designs, prefer the one that best satisfies:

> **PhoneBoost must spend materially less resource managing remote work than the useful resource value it unlocks.**

Security overhead that protects truth/authority is not optional.

Architectural overhead that exists only because of accidental layering should be removed.

The target is not "distributed computing at any cost."

The target is:

> **the smallest trustworthy mechanism that lets a weak host use available Android compute/capacity when the measured end-to-end tradeoff is favorable.**

Do not optimize for architectural novelty.

Do not optimize for number of features.

Optimize for useful net resource gain, portability, and proof.

