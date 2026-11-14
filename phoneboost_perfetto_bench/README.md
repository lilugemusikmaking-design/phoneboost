# PhoneBoost Perfetto multicore bench

Target package: `org.phoneboost.app.multicore`

Expected compute threads:
- `pb-cpu-0`
- `pb-cpu-1`
- `pb-cpu-2`
- `pb-cpu-3`

This bench is observational only:
- no CPU affinity
- no C13 change
- no ResourceGuard policy change
- product package `org.phoneboost.app` is not replaced

Files:
- `sched_pb.pbtxt`
- `analyze.sql`
- `run_campaign.sh`

One-time trace_processor setup:

```bash
curl -LO https://get.perfetto.dev/trace_processor
chmod +x trace_processor
```

Important:
- Let Perfetto finish naturally from `duration_ms`.
- Use `trace_processor query --format csv`.
- Wall-clock and digest correctness stay owned by PhoneBoost.
- The campaign harness invokes the existing authenticated C12 compute command.
- Capture duration is 40 s: the supplied 20 s window missed the actual compute
  after a measured 29.106 s transfer/command path. The incomplete trace is kept.

Integration: set `PB_CTL` to the experimental host CLI, `PB_INPUT` to the shared
regular input, `PB_EXPECTED_DIGEST` to the independent oracle, `TRACE_PROCESSOR`
to the verified tool and `OUT_DIR` to a new directory, then run
`bash run_campaign.sh 1` (also 2 and 4). The launcher uses the existing C12 command.
`analyze.sql` retains the supplied overlap/migration calculation; its input is
clipped to the PhoneBoost job's BOOTTIME windows/TIDs to exclude control traffic.
Raw traces are private local artifacts, never blanket-added to Git.
`test_analyze.py` checks the SQL arithmetic with synthetic slices only.

Expected columns:
`profile,configured_threads,observed_compute_threads,cpus_used,max_simultaneous_running,time_ge_2_threads_ms,time_ge_3_threads_ms,time_ge_4_threads_ms,cpu_time_ms,migration_count`
