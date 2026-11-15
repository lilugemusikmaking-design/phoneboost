#!/usr/bin/env python3
"""Ensure window accounting cannot hide payload loss, stale health, or spin."""
import copy
import json
from pathlib import Path
import unittest
from analyze_session_wait import summarize

class ProofTests(unittest.TestCase):
    def setUp(self):
        self.doc=json.loads((Path(__file__).resolve().parents[1]/'docs/evidence/session-input-wait-v0-1.json').read_text())
    def test_exact_recomputation(self):
        self.assertEqual(summarize(self.doc),self.doc['comparison'])
    def test_corruption_rejected(self):
        for kind in ('payload_frame','payload_bytes','wire_bytes','excess_control','fallback','stale','spin','binary','wake_count','order'):
            with self.subTest(kind=kind):
                d=copy.deepcopy(self.doc);r=d['runs'][3];m=r['metrics']
                if kind=='payload_frame':m['android']['RxFrame']['calls']-=1
                if kind=='payload_bytes':m['android']['RxFrame']['bytes']-=1
                if kind=='wire_bytes':m['android']['WireReadBody']['bytes']-=1
                if kind=='excess_control':m['android']['TxControl']['calls']+=3
                if kind=='fallback':r['execution_source']='LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE'
                if kind=='stale':r['native']['age_after_ms']=6001
                if kind=='spin':r['experiment']['idle']['host_cpu_ms']=5000
                if kind=='binary':r['experiment']['binary_sha256']['phoneboostd']='0'*64
                if kind=='wake_count':m['host']['SessionReadableWake']['calls']+=1
                if kind=='order':d['runs'][2:4]=reversed(d['runs'][2:4])
                with self.assertRaises(AssertionError):summarize(d)

if __name__=='__main__':unittest.main()
