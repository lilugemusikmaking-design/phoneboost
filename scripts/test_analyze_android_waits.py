#!/usr/bin/env python3
"""Reject incomplete waits in jobs; allow only a documented snapshot tail."""
import copy
import unittest
from analyze_android_waits import wait_observations, compare, ROOT, WAIT_READY, WAIT_FINISHED, RESP
import json


def fixture():
    rows = [[RESP, i, 1, i*1000000, i*1000000, 0] for i in range(1,18)]
    rows += [[WAIT_READY,0,1,100,1000,2], [WAIT_FINISHED,0,1,100,20000,2],
             [WAIT_READY,0,1,30000,50000,3], [WAIT_FINISHED,0,1,30000,50100,3]]
    return dict(schema=1, dropped=0, rows=rows)


class WaitTests(unittest.TestCase):
    def test_local_observation_is_not_total_poll(self):
        result = wait_observations(fixture())
        self.assertEqual(result['observations']['data'], 1)
        self.assertEqual(result['observations']['timer'], 1)
        self.assertAlmostEqual(result['data_ready_to_return_ms'], .019)
        shifted = fixture()
        for r in shifted['rows']: r[3] += 10**15; r[4] += 10**15
        self.assertEqual(result, wait_observations(shifted))

    def test_only_final_post_job_incomplete_wait_is_allowed(self):
        trace = fixture()
        trace['rows'].append([WAIT_READY,0,1,18000000,18010000,2])
        self.assertEqual(wait_observations(trace)['snapshot_tail_incomplete_waits'], 1)
        trace['rows'].append([WAIT_READY,0,1,18020000,18021000,2])
        with self.assertRaises(AssertionError): wait_observations(trace)
        trace = fixture()
        trace['rows'].append([WAIT_READY,0,1,100000,110000,2])
        with self.assertRaises(AssertionError): wait_observations(trace)
        trace = fixture()
        trace['rows'] += [[WAIT_READY,0,1,17500000,17600000,2],
                          [WAIT_FINISHED,0,1,17500000,19000000,2],
                          [WAIT_READY,0,1,18000000,18010000,2]]
        with self.assertRaises(AssertionError): wait_observations(trace)

    def test_snapshot_controls_and_threads_are_explicit_aggregate(self):
        trace = fixture()
        trace['rows'] += [[WAIT_READY,0,2,18000000,18001000,2],
                          [WAIT_FINISHED,0,2,18000000,18020000,2]]
        result = wait_observations(trace)
        self.assertEqual(result['observed_session_threads'], 2)
        self.assertAlmostEqual(result['data_ready_to_return_ms'], .038)
        self.assertIn('not PUT/wall latency', result['semantics'])

    def test_loss_duplicate_mismatch_and_overlap_are_rejected(self):
        for change in ('loss', 'duplicate', 'mismatch', 'overlap', 'missing_ready'):
            trace = fixture()
            if change == 'loss': trace['dropped'] = 1
            if change == 'duplicate': trace['rows'].append(copy.deepcopy(trace['rows'][-1]))
            if change == 'mismatch': trace['rows'][-1][5] = 2
            if change == 'overlap':
                trace['rows'][-1][3] = trace['rows'][-2][3] = 15000
            if change == 'missing_ready': trace['rows'].pop(-2)
            with self.assertRaises(AssertionError): wait_observations(trace)


class CampaignTests(unittest.TestCase):
    def test_reject_wrong_health_profile_and_recovery_even_with_success_flag(self):
        source = json.loads((ROOT/'docs/evidence/android-waits-v0-1.json').read_text())
        for change in ('power', 'profile', 'memory', 'recovery'):
            doc = copy.deepcopy(source)
            m = doc['runs'][2]['measurement']
            if change == 'power': m['native']['power_save'] = 0
            if change == 'profile': m['native']['configured_threads'] = 2
            if change == 'memory': m['health']['before']['natural_recovery_window'][0]['memory_bytes'] = '1'
            if change == 'recovery': m['health']['before']['recovery_observed'] = False
            with self.assertRaises(AssertionError): compare(doc)

    def test_reject_mode_order_provenance_or_unrestored_state(self):
        source = json.loads((ROOT/'docs/evidence/android-waits-v0-1.json').read_text())
        for change in ('order', 'apk', 'host', 'cleanup'):
            doc = copy.deepcopy(source)
            if change == 'order': doc['runs'][2:4] = doc['runs'][2:4][::-1]
            if change == 'apk': doc['runs'][2]['measurement']['experiment']['apk_sha256'] = 'wrong'
            if change == 'host': doc['runs'][2]['measurement']['experiment']['binary_sha256'] = {}
            if change == 'cleanup': doc['cleanup']['battery_saver_restored'] = False
            with self.assertRaises(AssertionError): compare(doc)


if __name__ == '__main__': unittest.main()
