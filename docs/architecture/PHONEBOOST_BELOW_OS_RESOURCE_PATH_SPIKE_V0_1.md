# PHONEBOOST — BELOW-OS RESOURCE PATH SPIKE V0.1

**Status:** ARCHITECTURE SPIKE — READ / ANALYZE / MEASURE BEFORE ANY IMPLEMENTATION  
**Target:** Codex / Astra in VS Code  
**Repository:** `lilugemusikmaking-design/phoneboost`  
**Baseline reference:** current PhoneBoost repository plus the active local worktree; local state may be ahead of GitHub master.

---

## 1. Mission

Evaluate one specific hypothesis:

> **Could PhoneBoost obtain a materially larger real resource gain by moving part of its execution/data path below the normal OS/application layer, instead of continuing to rely primarily on user-space Linux/Windows ↔ Android cooperation?**

Do not assume the answer is yes.

Do not reject the idea merely because it is harder.

Treat this as a falsifiable architecture hypothesis.

The purpose is to determine whether a lower-level path can remove enough overhead in CPU, RAM copies, latency, scheduling, context switching, buffering, or transport handling to create a **meaningful net gain** for PhoneBoost.

The final criterion is not architectural elegance.

The criterion is:

> **Does the lower-level design unlock substantially more useful resource than it consumes or complicates?**

---

## 2. Original PhoneBoost goal

PhoneBoost is intended to make a constrained computer more useful by borrowing selected Android resources when the total end-to-end tradeoff is favorable.

The current architecture already solved many difficult problems:

- trust and pairing;
- authenticated sessions;
- controller lease;
- Android ResourceGuard;
- RemoteBuffer truth;
- bounded compute;
- explicit fallback;
- no false remote success;
- transport handling;
- planner foundations;
- local API and real-file ingress;
- performance evidence/profile foundations.

This work must be treated as an asset.

The question is **not** whether to throw it away.

The question is whether part of the **data/execution path** should move lower while retaining the current control/trust plane.

---

## 3. What “below the OS” may mean

Do not use the phrase vaguely.

Investigate these distinct possibilities separately.

### A. Optimized user-space path

Before going lower, establish the best practical user-space baseline:

- fewer full copies;
- zero-copy or near-zero-copy techniques where feasible;
- streaming/pipelining;
- descriptor passing;
- shared-memory / mapped buffers where safe;
- native kernels;
- direct bounded binary framing;
- reduced serialization;
- tuned USB-network/Ethernet/Wi-Fi data paths.

### B. Kernel-assisted host path

Examples to investigate conceptually:

- Linux kernel module or eBPF-assisted path where relevant;
- kernel-side buffer/network assistance;
- Windows kernel driver / WDF equivalent later;
- io_uring / splice / sendfile / vmsplice / mmap / pinned-buffer approaches where applicable.

Determine whether these actually remove dominant copies or only move them.

### C. Android privileged/native lower layer

Investigate feasibility on **stock, non-rooted Android** first.

Then separately describe what would become possible with:

- privileged/system app;
- root;
- custom ROM/kernel;
- direct USB gadget/network support;
- native daemon outside the ordinary app sandbox.

Do not recommend a path that requires root/custom ROM as the default PhoneBoost product unless the gain is extraordinary and there is no stock-device alternative.

### D. Hypervisor / VM / device passthrough approach

Investigate whether a shared lower execution layer could sit below host OSes or Android.

Evaluate feasibility, not aesthetics.

### E. Transparent distributed memory / CPU

Evaluate:

- remote memory paging;
- distributed shared memory;
- swap/block-device style paths;
- remote scheduler / process migration;
- cross-ISA execution.

Be explicit about why each is or is not viable across x86-64 Linux/Windows and ARM64 Android.

---

## 4. Core question: where is the real overhead today?

Before recommending any lower layer, map the current active path from real host input to Android execution.

For the current best real workload path, identify every step:

```text
source data
 -> host application / UI / CLI
 -> local API / ingress
 -> daemon/import representation
 -> planner
 -> secure framing
 -> host socket/network stack
 -> physical link
 -> Android network stack
 -> secure runtime
 -> worker input representation
 -> RemoteBuffer / scratch / compute
 -> result return
```

For each stage measure or estimate:

- full copy vs view/reference;
- bytes copied;
- peak retained bytes;
- allocation lifetime;
- syscall count where relevant;
- context switches where relevant;
- serialization/framing overhead;
- encryption overhead;
- network-stack overhead;
- queueing;
- CPU time;
- memory overhead;
- latency contribution.

Build a **copy map** and a **latency map**.

Do not infer that moving below the OS is valuable until the dominant costs are identified.

---

## 5. Physical lower bound

Separate software overhead from unavoidable physics.

The following costs cannot be removed simply by moving into a kernel:

- bytes that must physically cross USB/Ethernet/Wi-Fi;
- link bandwidth;
- propagation/medium latency;
- Android memory bandwidth;
- Android CPU execution time;
- cross-device synchronization;
- encryption cost if security is retained.

Quantify the theoretical best-case improvement available if all avoidable software overhead disappeared.

If the best-case improvement is small, a lower-level rewrite is not justified.

---

## 6. Required comparison

Compare at least these architectures.

| Option | Description |
|---|---|
| U0 | Current PhoneBoost path |
| U1 | Optimized user-space PhoneBoost |
| K1 | Host kernel-assisted / driver-assisted path |
| K2 | Lower Android privileged/native path |
| K3 | Host + Android lower-level coordinated path |
| H1 | Hypervisor / VM approach |
| M1 | Transparent remote-memory approach |
| C1 | Transparent distributed-compute / scheduler approach |

For each score:

1. expected latency reduction;
2. expected CPU-overhead reduction;
3. expected RAM-copy reduction;
4. peak memory overhead;
5. stock Android compatibility;
6. Linux deployability;
7. Windows deployability;
8. root/admin requirements;
9. driver signing / distribution burden;
10. security surface;
11. crash blast radius;
12. portability;
13. maintenance;
14. implementation time;
15. reversibility;
16. compatibility with existing PhoneBoost C05-C13 work;
17. expected **net user benefit**.

---

## 7. Windows matters

PhoneBoost still needs a Windows host later.

Any proposal must explicitly answer:

- Can the same lower-level concept exist on Windows?
- Would it require a signed kernel driver?
- Would Microsoft driver policy / signing materially complicate consumer deployment?
- Can the architecture preserve a common cross-platform core?
- Would a Linux-only low-level optimization create technical debt for Windows?

A solution that makes Linux excellent but makes Windows impractical is not automatically a good PhoneBoost architecture.

---

## 8. Stock Android matters

PhoneBoost is intended to work on ordinary Android devices.

Evaluate every proposal against:

- normal app sandbox;
- Android foreground service rules;
- USB tethering;
- Ethernet adapters;
- local networking;
- NDK/JNI;
- SELinux;
- permissions;
- OEM differences;
- no root;
- no bootloader unlock.

If a lower-level path only works with root/custom firmware, classify it as **specialized mode**, not default architecture.

---

## 9. Investigate “lower without kernel driver” first

Before concluding that kernel code is necessary, explicitly test whether the desired gain can be reached with existing OS primitives.

Linux candidates may include, where applicable:

- `mmap`;
- `memfd`;
- `splice`;
- `sendfile`;
- `vmsplice`;
- `io_uring`;
- socket buffer tuning;
- direct FD handoff;
- bounded shared memory;
- incremental encryption/framing;
- pipeline overlap.

Android candidates may include:

- direct/native buffers;
- NDK sockets;
- memory mapping;
- direct ByteBuffer / owned native memory;
- reduced JNI crossings;
- incremental native compute;
- bounded producer/consumer buffers.

Determine what can realistically become zero-copy and what still necessarily copies.

---

## 10. Real gain, not benchmark theatre

The spike must distinguish:

### SPEED value
Remote end-to-end completion is materially faster.

### CAPACITY value
The host remains more responsive or avoids local memory/CPU pressure even if wall-clock time is not faster.

### LOCALITY value
Data already resident remotely avoids repeated transfer.

### FAKE gain
A microbenchmark improves while the end-to-end user task does not.

Do not recommend an architecture based on isolated syscall/network/kernel microbenchmarks alone.

---

## 11. Required test workloads

Use at least three workload shapes.

### W1 — compute-heavy / small input
Designed to expose remote-compute benefit.

### W2 — streamable large input
Designed to expose copy/memory/transport overhead.

### W3 — repeated operations on resident data
Designed to expose RemoteBuffer/locality value.

BLAKE3 may remain a reference workload but must not be the only evidence.

If no second kernel/workload is currently implemented, simulate or model the workload shape without pretending it is production proof.

---

## 12. Decision thresholds

Do not propose a lower-level rewrite for marginal gains.

Classify the result:

### NO-GO LOWER LAYER
If user-space overhead is not dominant or lower-layer savings are small relative to physical transfer/compute cost.

### SELECTIVE LOW-LEVEL OPTIMIZATION
If specific copies/syscalls/buffers are expensive and can be removed with bounded native/kernel assistance while preserving product portability.

### OPTIONAL HIGH-PERFORMANCE MODE
If substantial gain requires root/driver/custom firmware and should exist only for specialist users.

### CORE REORIENTATION
Only if the lower-level path produces a large, reproducible end-to-end improvement and remains practical on stock Android + Linux + future Windows.

---

## 13. Preserve existing PhoneBoost work

Unless evidence proves otherwise, retain:

- pairing and identity;
- SecureSession;
- PBMUX truth semantics;
- controller lease;
- ResourceGuard;
- C11 planner;
- C13 profiles;
- explicit RemoteBuffer;
- execution provenance;
- explicit fallback;
- no-false-success rules.

A lower-level data path should preferably sit **under** these abstractions rather than replace them.

Example conceptual target if justified:

```text
Application
    |
PhoneBoost API / Planner
    |
Trust / ResourceGuard / execution truth
    |
LOWER-OVERHEAD DATA PLANE
    |
USB / Ethernet / Wi-Fi
    |
Android native worker
```

---

## 14. Do not implement yet

This mission is an architecture/research spike first.

Before implementation, produce:

1. **CURRENT OVERHEAD MAP**
2. **PHYSICAL LOWER BOUND**
3. **OPTION COMPARISON**
4. **STOCK ANDROID FEASIBILITY**
5. **WINDOWS FEASIBILITY**
6. **EXPECTED NET GAIN**
7. **MIGRATION COST**
8. **VERDICT**

Then propose the smallest experiment capable of proving or falsifying the verdict.

Do not start a large driver/kernel/hypervisor implementation before that experiment.

---

## 15. Required final verdict

End with exactly one primary recommendation:

- `KEEP_USERSPACE`
- `OPTIMIZE_USERSPACE_FIRST`
- `SELECTIVE_KERNEL_ASSIST`
- `OPTIONAL_PRIVILEGED_MODE`
- `CORE_BELOW_OS_REORIENTATION`

Then provide:

- confidence level;
- strongest supporting evidence;
- strongest counterargument;
- one cheapest decisive experiment;
- what existing PhoneBoost work remains reusable;
- estimated impact on the Windows roadmap.

---

## 16. Principle

The question is not:

> "Can we build PhoneBoost below the OS?"

The question is:

> **"Can moving selected parts below the OS unlock enough additional useful resource to justify the loss of simplicity, portability and deployment ease?"**

If yes, prove where and why.

If no, prove that too.

The goal is a PhoneBoost that produces **real net resource value**, not merely a more sophisticated architecture.
