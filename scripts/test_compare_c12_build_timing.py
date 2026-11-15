#!/usr/bin/env python3
"""Reject corrupted recorded proofs; never supplies or runs a hardware proof."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from compare_c12_build_timing import compare

ROOT = Path(__file__).resolve().parents[1]
DEV = ROOT/'docs/evidence/c12-end-to-end-timing-v0-1.json'
RELEASE = ROOT/'docs/evidence/c12-release-vs-dev-v0-1.json'


class ComparisonProofTests(unittest.TestCase):
    def setUp(self):
        self.dev = json.loads(DEV.read_text())
        self.release = json.loads(RELEASE.read_text())

    def test_recorded_comparison_recomputes_from_raw_values(self):
        self.assertEqual(compare(self.dev,self.release), self.release['comparison'])

    def test_invalid_proofs_cannot_be_reported_as_a_successful_comparison(self):
        for kind in ('fallback','digest','stale','frames','input','apk','lockfile','device','sources',
                     'order','warmup','pair','binary','dev_fallback','ac_power'):
            with self.subTest(kind=kind):
                bad = copy.deepcopy(self.release)
                run = bad['runs'][0]
                if kind == 'fallback': run['execution_source'] = 'LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE'
                if kind == 'digest': run['digest_exact'] = False
                if kind == 'stale': run['native']['age_after_ms'] = 6001
                if kind == 'frames': run['metrics']['android']['RxFrame']['calls'] -= 1
                if kind == 'input': bad['input']['bytes'] -= 1
                if kind == 'apk': bad['build']['apk_sha256'] = '0'*64
                if kind == 'lockfile': bad['build']['cargo_lock_sha256'] = '0'*64
                if kind == 'device': bad['device']['model'] = 'different-model'
                if kind == 'sources': bad['build']['source_files'][0]['sha256'] = '0'*64
                if kind == 'order': bad['campaign_order'][2:4] = reversed(bad['campaign_order'][2:4])
                if kind == 'warmup': run['experiment']['warmup'] = True
                if kind == 'pair': run['experiment']['pair'] = 2
                if kind == 'binary': run['experiment']['binary_sha256']['phoneboostd'] = '0'*64
                if kind == 'dev_fallback': bad['paired_dev_runs'][0]['execution_source'] = 'LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE'
                if kind == 'ac_power': run['host_environment']['before']['power'] = []
                with self.assertRaises(AssertionError):
                    compare(self.dev,bad)

    def test_cli_rejects_a_tampered_gain_even_if_raw_runs_are_valid(self):
        self.release['comparison']['end_to_end_speedup'] *= 2
        with tempfile.TemporaryDirectory(prefix='pb-compare-negative-') as directory:
            path=Path(directory)/'evidence.json'
            path.write_text(json.dumps(self.release))
            result=subprocess.run([sys.executable,str(ROOT/'scripts/compare_c12_build_timing.py'),
                                   '--dev',str(DEV),'--release',str(path)],capture_output=True,text=True)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('stored comparison does not match raw evidence',result.stderr)


if __name__ == '__main__':
    unittest.main()
