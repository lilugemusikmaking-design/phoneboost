#!/usr/bin/env python3
import copy
import json
from pathlib import Path
import unittest

from analyze_android_decrypt import compare, verify_traces

ROOT = Path(__file__).resolve().parents[1]


class DecryptEvidence(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.doc = json.loads((ROOT / 'docs/evidence/android-decrypt-v0-1.json').read_text())

    def test_physical_evidence_recomputes_without_cross_clock_sum(self):
        verify_traces(self.doc)
        result = compare(self.doc)
        self.assertEqual(result['valid_pairs'], 3)
        self.assertTrue(all(p['decrypt_faster'] and p['end_to_end_faster'] for p in result['pairs']))

    def test_substituted_artifact_or_changed_workload_refused(self):
        for mutation in ('artifact', 'bytes', 'fallback', 'order', 'unrestored'):
            d = copy.deepcopy(self.doc)
            if mutation == 'artifact':
                d['apk_build']['apks']['neon']['apk_sha256'] = '0' * 64
            elif mutation == 'bytes':
                d['apk_runs'][3]['measurement']['metrics']['android']['Decrypt']['bytes'] += 1
            elif mutation == 'fallback':
                d['apk_runs'][3]['measurement']['execution_source'] = 'LOCAL_FALLBACK'
            elif mutation == 'order':
                d['apk_runs'][3]['measurement']['utc'] = d['apk_runs'][2]['measurement']['utc']
            else:
                d['cleanup']['battery_saver_restored'] = False
            with self.subTest(mutation=mutation), self.assertRaises(AssertionError):
                compare(d)

    def test_missing_run_or_trace_hash_refused(self):
        d = copy.deepcopy(self.doc)
        d['apk_runs'].pop()
        with self.assertRaises(AssertionError):
            compare(d)
        d = copy.deepcopy(self.doc)
        d['apk_runs'][0]['traces']['android']['sha256'] = '0' * 64
        with self.assertRaises(AssertionError):
            verify_traces(d)


if __name__ == '__main__':
    unittest.main()
