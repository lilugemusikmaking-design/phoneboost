#!/usr/bin/env python3
"""Explicit experiment via existing phoneboostctl -> C12 -> authenticated C10.

Prerequisites: isolated experimental APK paired to the test host; same regular
input file and independently computed expected digest for every run. No phone
setting, health value, affinity or contract threshold is changed by this tool.
"""
import argparse
import csv
import json
from pathlib import Path
import re
import statistics
import subprocess
import time
import uuid

RECEIVER = "org.phoneboost.app.multicore/org.phoneboost.app.MulticoreProfileReceiver"


def classify(runs, perfetto=None):
    perfetto = perfetto or {}
    if any(r.get("digest_mismatch") for r in runs):
        return "MULTICORE_NOT_CONFIRMED", {}
    measured = [r for r in runs if not r["warmup"]]
    groups = {p: [r for r in measured if r["profile"] == p and r.get("valid")]
              for p in (1, 2, 4)}
    summary = {}
    for p, rows in groups.items():
        if rows:
            times = [r["native"]["wall_ns"] / 1e6 for r in rows]
            summary[str(p)] = {"count": len(rows), "median_native_ms": statistics.median(times),
                "median_mib_s": statistics.median(r["input_bytes"] / 2**20 / (r["native"]["wall_ns"] / 1e9) for r in rows),
                "median_cpu_over_wall": statistics.median(r["native"]["cpu_ns"] / r["native"]["wall_ns"] for r in rows),
                "median_end_to_end_ms": statistics.median(r["end_to_end_ms"] for r in rows)}
    beneficial = False
    for p in (2, 4):
        rows = groups[p]
        # Scope overlap and CPU-ID samples alone do not demonstrate multicore.
        confirmed = len(rows) >= 6 and sum(
            r["native"]["executed_threads"] >= 2
            and r["native"]["peak_hash_scopes"] >= 2
            and r["native"]["cpu_mask"].bit_count() >= 2
            and r["native"]["cpu_ns"] / r["native"]["wall_ns"] > 1.10
            and not any(t["cpu_id_overflow"] for t in r["native"]["threads"])
            for r in rows) >= 5
        scheduler = perfetto.get(str(p), {})
        confirmed = confirmed and scheduler.get("qualified") is True and (
            scheduler.get("max_simultaneous_running", 0) >= 2
            and scheduler.get("time_ge_2_threads_ms", 0) > 0
            and scheduler.get("input_bytes") == rows[0]["input_bytes"])
        if rows:
            summary[str(p)]["multicore_execution_confirmed"] = confirmed
        if not confirmed:
            continue
        if len(groups[1]) < 6:
            summary[str(p)]["benefit_evidence"] = "baseline_missing"
            continue
        baseline = {r["rotation"]: r["native"]["wall_ns"] for r in groups[1]}
        ratios = [baseline[r["rotation"]] / r["native"]["wall_ns"] for r in rows if r["rotation"] in baseline]
        if len(ratios) < 6:
            continue
        speedup = statistics.median(ratios)
        summary[str(p)]["median_paired_speedup"] = speedup
        summary[str(p)]["paired_speedups"] = ratios
        # Predeclared material gain: >=10% median, >=5/6 pairs at least 5% faster.
        if speedup >= 1.10 and sum(r > 1.05 for r in ratios) >= 5:
            beneficial = True
        summary[str(p)]["multicore_confirmed"] = True
    if beneficial:
        return "MULTICORE_CONFIRMED_BENEFICIAL", summary
    if any(s.get("multicore_confirmed") for s in summary.values()):
        return "MULTICORE_CONFIRMED_NO_BENEFIT", summary
    if all(any(r["profile"] == p and r.get("native", {}).get("refusal") in (1, 2, 4)
               for r in runs) for p in (2, 4)):
        return "MULTICORE_BLOCKED_BY_RESOURCE_GUARD", summary
    return "MULTICORE_NOT_CONFIRMED", summary


def run(command, timeout=30, **kwargs):
    return subprocess.run(command, text=True, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT, timeout=timeout, **kwargs)


def current_record(logs, token):
    for encoded in reversed(re.findall(r"MC_JOB (\{[^\n]+\})", logs)):
        record = json.loads(encoded)
        if record.get("probe_token") == token and record.get("coherent") is True:
            return record
    return {}


def read_perfetto(root):
    evidence = {}
    for profile in (1, 2, 4):
        directory = root / f"profile-{profile}"
        def csv_rows(name):
            with (directory / name).open() as stream:
                return list(csv.DictReader(stream))
        metrics = csv_rows("metrics.csv")
        clocks = csv_rows("clock.csv")
        bounds = csv_rows("trace-bounds.csv")
        errors = csv_rows("trace-errors.csv")
        rows = json.loads((directory / "job/runs.json").read_text())
        assert len(metrics) == 1 and len(rows) == 1 and len(bounds) == 1
        job = rows[0]
        metric = {key: float(value) for key, value in metrics[0].items() if key != "cpu_ids"}
        metric["cpu_ids"] = metrics[0].get("cpu_ids", "")
        metric["input_bytes"] = job["input_bytes"]
        threads = job["native"].get("threads", [])
        metric["qualified"] = bool(job.get("valid") and threads and not errors and clocks
            and all(int(clock["offset_ns"]) == 0 for clock in clocks)
            and int(bounds[0]["start_ts"]) <= min(t["begin_ns"] for t in threads)
            and int(bounds[0]["end_ts"]) >= max(t["end_ns"] for t in threads)
            and metric["observed_compute_threads"] == len(threads)
            and 1 <= metric["max_simultaneous_running"] <= profile)
        evidence[str(profile)] = metric
    return evidence


def finish_campaign(adb, out, rows, perfetto=None):
    # Preserve measured evidence even if the device disconnects during cleanup.
    verdict, summary = classify(rows, perfetto)
    report = {"verdict": verdict, "summary": summary, "perfetto": perfetto or {}}
    path = out / "summary.json"
    def persist():
        try:
            path.write_text(json.dumps(report, indent=2) + "\n")
        except OSError as error:
            report["report_write_error"] = str(error)
    persist()
    try:
        restored = run(adb + ["shell", "am", "broadcast", "-n", RECEIVER, "--ei", "threads", "1"])
        if restored.returncode or "configured=1 result=0" not in restored.stdout:
            raise RuntimeError(restored.stdout)
        report["restored_profile_one"] = True
    except (OSError, subprocess.SubprocessError, RuntimeError) as error:
        report["restored_profile_one"] = False
        report["restoration_error"] = str(error)
    try:
        thermal = run(adb + ["shell", "dumpsys", "thermalservice"])
        (out / "thermal-after.private.txt").write_text(thermal.stdout)
        report["final_thermal_collected"] = thermal.returncode == 0
    except (OSError, subprocess.SubprocessError) as error:
        report["final_thermal_error"] = str(error)
    persist()
    try:
        print(json.dumps(report), flush=True)
    except OSError as error:
        report["report_print_error"] = str(error)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ctl", required=True)
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--expected-digest", required=True)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--profile", type=int, choices=(1, 2, 4))
    parser.add_argument("--repetitions", type=int, choices=range(7), default=6)
    parser.add_argument("--perfetto-root", type=Path)
    args = parser.parse_args()
    perfetto = read_perfetto(args.perfetto_root) if args.perfetto_root else {}
    if not re.fullmatch(r"[0-9a-f]{64}", args.expected_digest):
        parser.error("expected digest must be a canonical BLAKE3 hex digest")
    if not args.input.is_file() or not 1024 * 1024 <= args.input.stat().st_size <= 120 * 1024 * 1024:
        parser.error("regular input file must be 1..120 MiB (input + existing 8 MiB scratch <=128 MiB)")
    args.out.mkdir(parents=True, exist_ok=False)
    adb = ["adb", "-d"]
    thermal = run(adb + ["shell", "dumpsys", "thermalservice"])
    if thermal.returncode or "HAL Ready: true" not in thermal.stdout or "IsStatusOverride: false" not in thermal.stdout:
        raise SystemExit("Thermal provenance unavailable or overridden; no workload started")
    (args.out / "thermal-before.private.txt").write_text(thermal.stdout)
    rows = []
    blocked = set()
    # One warmup each, six measurements each, balanced Latin-square order.
    profiles = (args.profile,) if args.profile else (1, 2, 4)
    schedule = [(True, -1, p) for p in profiles]
    for rotation in range(args.repetitions):
        order = ((1, 2, 4), (2, 4, 1), (4, 1, 2))[rotation % 3]
        schedule.extend((False, rotation, p) for p in order if p in profiles)
    try:
        for warmup, rotation, profile in schedule:
            if profile in blocked:
                continue
            configure = run(adb + ["shell", "am", "broadcast", "-n", RECEIVER, "--ei", "threads", str(profile)])
            if configure.returncode or f"configured={profile} result=0" not in configure.stdout:
                raise RuntimeError("experimental profile configuration failed: " + configure.stdout)
            # Let normal readiness/Health processing run. No synthetic refresh.
            time.sleep(2.1)
            started = time.monotonic_ns()
            with args.input.open("rb") as source:
                result = run([args.ctl, "compute", "blake3", "--stdin"], timeout=90, stdin=source)
            elapsed_ms = (time.monotonic_ns() - started) / 1e6
            token = uuid.uuid4().hex
            dump = run(adb + ["shell", "am", "broadcast", "-n", RECEIVER, "--es", "probe_token", token])
            logs = run(adb + ["logcat", "-d", "-s", "PhoneBoostMC:I"])
            native = current_record(logs.stdout, token) if dump.returncode == 0 and logs.returncode == 0 else {}
            (args.out / f"run-{len(rows):02d}-ctl.txt").write_text(result.stdout)
            correct = f"BLAKE3 digest: {args.expected_digest}" in result.stdout
            returned_digest = re.search(r"BLAKE3 digest: ([0-9a-f]{64})", result.stdout)
            valid = (result.returncode == 0 and "Execution source: REMOTE_SUCCESS" in result.stdout
                and correct and native.get("coherent") is True and native.get("outcome") == 0
                and native.get("configured_threads") == profile
                and native.get("input_bytes") == args.input.stat().st_size and native.get("wall_ns", 0) > 0
                and 1 <= native.get("executed_threads", 0) <= profile
                and 1 <= native.get("peak_hash_scopes", 0) <= profile
                and native.get("cpu_mask", -1) >= 0
                and native.get("scratch_bytes") == 8 * 1024 * 1024
                and native.get("held_before", 0) - native.get("held_after", 0) == native.get("scratch_bytes")
                and native.get("safety_before") in (0, 1) and native.get("safety_after") in (0, 1)
                and 0 <= native.get("age_before_ms", -1) <= 6000
                and 0 <= native.get("age_after_ms", -1) <= 6000)
            row = {"profile": profile, "warmup": warmup, "rotation": rotation,
                "input_bytes": args.input.stat().st_size, "end_to_end_ms": elapsed_ms,
                "exit_code": result.returncode, "digest_exact": correct,
                "digest_mismatch": bool(returned_digest and not correct), "valid": valid, "native": native}
            rows.append(row)
            (args.out / "runs.json").write_text(json.dumps(rows, indent=2) + "\n")
            print(json.dumps(row), flush=True)
            refused = native.get("configured_threads") == profile and native.get("refusal") in (1, 2, 3, 4)
            if refused:
                blocked.add(profile)  # preserve refusal; do not change conditions to force success
            if row["digest_mismatch"] or (not correct and not refused):
                raise RuntimeError("result digest absent/mismatching; campaign stopped")
    finally:
        # Restore the experiment's one-thread configuration, no system setting.
        report = finish_campaign(adb, args.out, rows, perfetto)
    if not report["restored_profile_one"] or "report_write_error" in report or "report_print_error" in report:
        raise RuntimeError("campaign cleanup/persistence failed; inspect summary and restore profile 1")


if __name__ == "__main__":
    main()
