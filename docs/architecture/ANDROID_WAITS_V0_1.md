# Android receive and handler waits v0.1

PR #36 was merged with a merge commit, preserving its history. Base master:
`93f6446dd795ca32ebf0a40273b68e3ec7dbd09c`.
Branch: `spike/android-waits-v0-1`.

The combined candidate is retained for review: two of three paired runs improve
end-to-end time, with a paired median gain of **11.80%**. The unfavorable pair
is retained. This is a bounded physical result on this device and power state,
not a guarantee for every run or a factorial attribution between the two changes.

## Causal audit and minimal implementation

The authenticated socket is nonblocking. `receive_frame_if_available` peeks the
two-byte length prefix. No bytes or one byte returns `None`; after two bytes,
the existing reader temporarily uses blocking reads with the unchanged 5 s
record timeout. It then restores nonblocking mode. Android previously slept
20 ms whenever no frame was ready, even if data arrived during that sleep.
The initiator already used `input_wait::wait` with rustix 1.1.6 poll. Both roles
now use that helper. It consumes no bytes and changes no socket mode.

The one-byte-prefix fallback remains a bounded 20 ms sleep: polling a readable
single byte repeatedly would spin. RDHUP/EOF/errors remain fail-closed. Partial
payload handling, heartbeat 2 s, offline 10 s, lease and cancellation boundaries
are unchanged. No generic event loop or new synchronization primitive is added.

Both RemoteBuffer and compute handlers already use scoped workers and an
existing `sync_channel(1)`. The session formerly checked the channel then slept
20 ms. It now calls `recv_timeout(20 ms)`. Liveness is checked before waiting
and after completion/disconnection notification; a concurrent SessionLost takes
priority over a result. Timeouts re-enter the existing liveness checks. The
scoped join still requires cooperative termination of a handler after peer
revocation; this does not claim a new hard cancellation guarantee.

No changes to Noise/NEON, dependencies, PBMUX, record/frame/PUT sizes,
ResourceGuard, RemoteBuffer residency, protocol timeouts, C13, CPU affinity or
experimental CPU profile rules.

## Provenance and physical controls

- Instrumented legacy baseline: `a80f66ab77cdc4c32cc9bdcac600e1577ac78d3f`.
- Candidate binary source: `572dc0e0430df6cff6733cd1c6799d422da5f25b`.
- Both APKs: `org.phoneboost.app.multicore`, native standard Cargo release,
  rustc 1.98.0, `aarch64-linux-android`, `diagnostic-timing`, pinned
  `--cfg chacha20_force_neon`. No environment profile/flags override.
- APK ZIP payloads differ only in `lib/arm64-v8a/libphoneboost_core_jni.so`
  (signature metadata excluded). Full APK/native/host/source hashes and
  identical Cargo.lock/config/manifest hashes are in the evidence.
- Same frozen release daemon/CLI, Samsung SM-A155F Android 16/API 36,
  USB/ADB, exact 64 MiB fixture and BLAKE3 digest for all eight runs.
- Battery Saver was **ON throughout**. No power-mode command was sent.
  Existing ResourceGuard allows profile **1** in Throttle; profiles >1 require
  Nominal. Both A/B therefore use profile 1, without changing policy. Older
  OFF/profile2 measurements are historical context, not this baseline.
- Before every run: thermal HAL Ready, no override, USB power, fresh real
  Health, existing nonzero budget, memory >=896 MiB across >=10 s. Health stays
  thermal 0, lowMemory false, powerSave true, Safety Throttle with no refusal.
  Measured memory spans approximately 911–1055 MiB; minimum budget >99 MiB.
- Same install/restart, recovery and host READY sequence per run, retaining
  pairing data. Two warmups followed by A/B, A/B, A/B. No heavy host build or
  stress test runs during measurement. Host environment is recorded per run.
- An initial private-script duplicate metadata key stopped setup before any
  daemon/workload. It was corrected once; zero measurements were discarded.

## Observation semantics

Optional local events 1020 WAIT_READY / 1021 WAIT_FINISHED carry only a local
thread pseudonym, start/end and enum: already-readable, data, timer, partial,
closed/error. They add no wire field or sensitive identifier.

The baseline observes readiness once, then sleeps the remainder of its original
20 ms quantum. It never wakes early or consumes input. With tracing disabled,
it keeps the original sleep. The candidate uses the same active observer and
returns on readiness. Two readable prefix bytes do **not** prove a complete
record, nor is the observation an exact kernel arrival timestamp.

Ready-to-return sums cover the entire session capture, including controls.
They are not per-PUT wall latency and must not be added to handler savings or
host timings. Handler end -> observed is separately correlated for all 17 PUTs
by existing events 1015/1016. All traces have dropped=0, one observed session
thread, and no incomplete tail in measured runs. The validator rejects in-job
missing events, duplicates, mismatches and overlapping waits. A single final
post-job snapshot boundary is supported explicitly, never silently dropped.

## Physical A/B results

All values below are medians of three measured runs per variant. Independent
medians and overlapping clock views must not be summed into an end-to-end
partition. Raw runs, ranges, per-PUT intervals and numeric traces are retained.

| Metric | Legacy A | Candidate B |
| --- | ---: | ---: |
| End-to-end, ms | 3288.138 | 2938.770 |
| PUT loop, ms | 2716.759 | 2423.170 |
| Useful PUT throughput, MiB/s | 23.557 | 26.412 |
| End-to-end throughput, MiB/s | 19.464 | 21.778 |
| Host SessionPoll, ms | 3050.319 | 2623.927 |
| Android receive SessionPoll, whole capture, ms | 1486.228 | 1291.731 |
| Android receive poll inside PUT reassembly windows, ms | 347.217 | 388.000 |
| PUT handler end -> observed, ms | 310.589 | 5.259 |
| PUT handler execution / included copy, ms | 27.127 / 26.107 | 26.197 / 25.439 |
| Android decrypt inside PUT windows, ms | 647.629 | 616.502 |
| Android record reads inside PUT windows, ms | 530.817 | 730.746 |
| Android WireReadBody, capture counter, ms | 524.661 | 725.583 |
| Host queue before send, ms | 305.112 | 290.262 |
| Outside Android causal core, duration residual, ms | 388.371 | 150.212 |
| Host process CPU, ms | 320 | 320 |
| Android process CPU, ms | 1220 | 1180 |
| Capture sum, data observed -> wait return, ms | 478.031 | 0.334 |
| Data / timer readiness observations, count | 42 / 30 | 42 / 50 |

The baseline returns on its legacy quantum even when it observed data earlier;
its `SessionReadableWake`/`SessionTimeoutWake` counters are consequently zero.
Candidate counter medians are 42 readiness returns and 50 timer returns.
Observation/counter windows can differ by the final in-progress wait; raw
counts are retained. No partial-prefix or closed/error observation occurred.
The candidate does not eliminate waits for input that has not arrived: receive
poll within PUT windows and body-read time can increase as deferred sleeps
vanish. These redistributions are not additional latency regressions to add
to the total. Their cause within socket/transport/scheduling is not yet isolated.

Android CPU is measured with process ticks in an ADB-bracketed interval that
includes capture overhead; host CPU brackets the CLI. Neither is precise
per-handler CPU. There is no median CPU increase, and deterministic empty/
partial-input and timeout tests verify blocking rather than busy-waiting.

| Pair | A end-to-end ms | B end-to-end ms | Gain |
| --- | ---: | ---: | ---: |
| 1 | 3278.230 | 3344.816 | -2.03% |
| 2 | 3501.755 | 2938.770 | +16.08% |
| 3 | 3288.138 | 2899.992 | +11.80% |

Paired median gain: **11.80%**, 2/3 favorable. Ratio of independent medians gives
10.63%, a different statistic. Ranges: A 3278–3502 ms, B 2900–3345 ms.
Pair 1 includes a longer observed CLI import READY wait (50.6 ->190.3 ms),
body reads and host CPU. Those facts explain the variability in the recorded
partition, but do not prove a specific governor or transport cause. No outlier
was removed and no additional favorable runs were selected.

All eight results are REMOTE_SUCCESS with exact digest. Each has 17 PUTs,
1105 PUT records and identical application frame counts/bytes. Android capture
contains 1114 decrypt records including control traffic; control-window
accounting remains explicit. Warmups are excluded from the paired statistics.

## Verification and cleanup

Local validation passed: runtime 36 tests +4 doctests, worker 60 tests +2
doctests, repository-policy Clippy, both APK builds and Android lint, formatting,
new analyzer refusal tests and existing timing/power/evidence validators.
Completion tests cover notification before/during wait, repeated timeouts,
shutdown/disconnection race, offline deadline and both real scoped handlers.
Existing partial prefix/payload, cancellation and heartbeat coverage remains.

Independent read-only reviews covered the input path, completion race and
measurement validator. Accepted corrections: post-wake liveness priority,
real-wrapper tests, incomplete-tail overlap rejection, explicit aggregate
readiness semantics, immutable campaign schedule and per-install APK hashes.

Cleanup physically verified: Battery Saver ON with fresh Health, profile 1,
timing disabled, experimental app stopped, owned host/ADB forward removed,
product APK hash unchanged. Historical branches/worktrees and earlier evidence
are preserved. No new physical workload is needed for this publication.

Evidence: [android-waits-v0-1.json](../evidence/android-waits-v0-1.json).
Reproduce analysis without a device:

```sh
python3 scripts/test_analyze_android_waits.py
python3 scripts/analyze_android_waits.py docs/evidence/android-waits-v0-1.json
```

## Remaining bottleneck and next action

Largest directly isolated remaining Android PUT component: record reading,
**730.746 ms**, mostly body reads (capture median **725.583 ms**), followed by
decrypt **616.502 ms**. Host SessionPoll is a containing wait, not a USB RTT.
The 150.212 ms residual outside the Android causal core is not a proven
transport-only cost. Current evidence cannot divide body-read time into data
arrival, USB/ADB, kernel copying and scheduling, so no transport-capacity claim
or further optimization is made here.

NEXT_ACTION = Quantify the socket-wait, scheduling and local-read contributions
inside Android WireReadBody (~726 ms), before selecting one reception optimization.
