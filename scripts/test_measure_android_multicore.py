#!/usr/bin/env python3
import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from measure_android_multicore import classify, current_record, finish_campaign


class VerdictTests(unittest.TestCase):
    perfetto = {"2": {"qualified": True, "max_simultaneous_running": 2,
        "time_ge_2_threads_ms": 10, "input_bytes": 32 * 1024 * 1024}}

    def test_full_output_disk_still_attempts_restoration(self):
        restored = subprocess.CompletedProcess([], 0, "configured=1 result=0")
        with patch("measure_android_multicore.run", return_value=restored) as command:
            with patch.object(Path, "write_text", side_effect=OSError("disk full")):
                report = finish_campaign(["adb"], Path("/unused"), [])
        self.assertTrue(report["restored_profile_one"])
        self.assertIn("report_write_error", report)
        self.assertEqual(command.call_count, 2)

    def test_disconnect_during_cleanup_preserves_report_and_original_error(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch("measure_android_multicore.run", side_effect=subprocess.TimeoutExpired("adb", 30)):
                with self.assertRaisesRegex(ValueError, "measurement error"):
                    try:
                        raise ValueError("measurement error")
                    finally:
                        finish_campaign(["adb"], Path(directory), self.rows())
            report = json.loads((Path(directory) / "summary.json").read_text())
            self.assertFalse(report["restored_profile_one"])
            self.assertIn("restoration_error", report)
            self.assertIn("final_thermal_error", report)
            self.assertEqual(report["summary"]["1"]["count"], 6)

    def test_stale_or_incoherent_logcat_cannot_become_current_evidence(self):
        old = {"probe_token": "old", "coherent": True, "refusal": 2}
        torn = {"probe_token": "current", "coherent": False}
        logs = "MC_JOB " + json.dumps(old) + "\nMC_JOB " + json.dumps(torn)
        self.assertEqual(current_record(logs, "current"), {})
        fresh = {"probe_token": "current", "coherent": True, "refusal": 0}
        self.assertEqual(current_record(logs + "\nMC_JOB " + json.dumps(fresh), "current"), fresh)

    def rows(self, ratio=1.8, cpus=3, scopes=2):
        rows = []
        for rotation in range(6):
            for profile in (1, 2):
                wall = 100_000_000 if profile == 1 else 60_000_000
                rows.append({"profile": profile, "warmup": False, "rotation": rotation,
                    "input_bytes": 32*1024*1024, "end_to_end_ms": 1000, "valid": True,
                    "native": {"wall_ns": wall, "cpu_ns": wall*(1 if profile == 1 else ratio),
                        "executed_threads": profile, "peak_hash_scopes": scopes,
                        "cpu_mask": cpus, "threads": [{"cpu_id_overflow": False}]}})
        return rows

    def test_configuration_and_overlapping_scopes_do_not_prove_multicore(self):
        self.assertEqual(classify(self.rows(ratio=0.99))[0], "MULTICORE_NOT_CONFIRMED")
        self.assertEqual(classify(self.rows(cpus=1))[0], "MULTICORE_NOT_CONFIRMED")
        self.assertEqual(classify(self.rows(scopes=1))[0], "MULTICORE_NOT_CONFIRMED")

    def test_benefit_requires_repeated_exact_guarded_results(self):
        rows = self.rows()
        self.assertEqual(classify(rows, self.perfetto)[0], "MULTICORE_CONFIRMED_BENEFICIAL")
        self.assertEqual(classify(rows)[0], "MULTICORE_NOT_CONFIRMED")
        wrong = copy.deepcopy(rows); wrong[-1]["digest_mismatch"] = True
        self.assertEqual(classify(wrong, self.perfetto)[0], "MULTICORE_NOT_CONFIRMED")
        self.assertEqual(classify(rows[:4])[0], "MULTICORE_NOT_CONFIRMED")
        without_control = [r for r in rows if r["profile"] == 2]
        verdict, summary = classify(without_control, self.perfetto)
        self.assertTrue(summary["2"]["multicore_execution_confirmed"])
        self.assertEqual(summary["2"]["benefit_evidence"], "baseline_missing")
        self.assertEqual(verdict, "MULTICORE_NOT_CONFIRMED")
        no_benefit = self.rows()
        for r in no_benefit:
            r["native"]["wall_ns"] = 100_000_000
            r["native"]["cpu_ns"] = 180_000_000 if r["profile"] == 2 else 99_000_000
        self.assertEqual(classify(no_benefit, self.perfetto)[0], "MULTICORE_CONFIRMED_NO_BENEFIT")

    def test_guard_refusal_and_missing_evidence_are_distinct(self):
        rows = [{"profile": p, "warmup": True, "valid": False, "native": {"refusal": 2}}
                for p in (2, 4)]
        self.assertEqual(classify(rows)[0], "MULTICORE_BLOCKED_BY_RESOURCE_GUARD")
        rows[0]["native"] = {}
        self.assertEqual(classify(rows)[0], "MULTICORE_NOT_CONFIRMED")


if __name__ == "__main__":
    unittest.main()
