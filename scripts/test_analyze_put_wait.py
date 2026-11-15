#!/usr/bin/env python3
"""Causal joins, loss rejection and independence of host/Android clock epochs."""
import copy
import unittest
from analyze_put_wait import *


def fixture():
    h, a = [], []
    def row(rows, kind, key, thread, start, end=None, value=0):
        rows.append([kind, key, thread, start * 1000, (start if end is None else end) * 1000, value])
    for index in range(17):
        key, caller, base = index + 100, index + 1, index * 2000
        count = 69 if index < 16 else 1
        row(h, READY, caller, 1, base, value=4194240 if index < 16 else 1024)
        row(h, ENQUEUE, caller, 1, base + 1)
        row(h, START, key, 2, base + 2, value=caller)
        for i in range(count):
            flags = (1 if i == 0 else 0) | (2 if i == count - 1 else 0)
            t = base + 10 + i * 5
            row(h, TX, key, 2, t, t + 3, flags)
            row(h, P['Encrypt'], 0, 2, t + 1, t + 2)
            row(h, P['WireWrite'], 0, 2, t + 2, t + 3)
            t = base + 20 + i * 5
            row(a, RX, key, 3, t, t + 3, flags)
            row(a, P['WireReadBody'], 0, 3, t, t + 1)
            row(a, P['Decrypt'], 0, 3, t + 1, t + 2)
            row(a, P['FrameDecode'], 0, 3, t + 2, t + 3)
            row(a, P['Reassembly'], 0, 3, t + 3, t + 4)
        row(h, SENT, key, 2, base + 20 + count * 5, value=count)
        row(h, P['SessionPoll'], 0, 2, base + 20 + count * 5, base + 1000)
        row(h, RX, key, 2, base + 1000, base + 1002, 3)
        row(h, NOTIFY, key, 2, base + 1003)
        row(h, AWAKE, caller, 1, base + 1004, value=1)
        t = base + 30 + count * 5
        for kind, offset, thread in [(REASSEMBLED, 0, 3), (HQ, 2, 3), (HS, 3, 4),
                                      (HE, 5, 4), (HO, 22, 3), (BUILT, 23, 3), (RESP, 28, 3)]:
            row(a, kind, key, thread, t + offset)
        row(a, P['WorkerPutCopy'], 0, 4, t + 3, t + 4)
        row(a, P['BufferHandlerPoll'], 0, 3, t + 2, t + 22)
        row(a, TX, key, 3, t + 24, t + 27, 3)
    return ({'schema': 1, 'dropped': 0, 'rows': h}, {'schema': 1, 'dropped': 0, 'rows': a})


class AnalysisTests(unittest.TestCase):
    def test_clock_epochs_can_shift_independently(self):
        host, android = fixture()
        expected = analyze(host, android)
        for row in android['rows']:
            row[3] += 123_456_789_000_000
            row[4] += 123_456_789_000_000
        self.assertEqual(analyze(host, android), expected)
        self.assertAlmostEqual(expected['total']['android_ms']['android_handler_execution'], .034)
        self.assertAlmostEqual(expected['total']['android_ms']['android_handler_completion_poll_delay'], .289)
        self.assertGreater(expected['total']['outside_android_core_ms'], 0)

    def test_missing_fragment_and_failed_response_rejected(self):
        host, android = fixture()
        android['rows'].remove(next(r for r in android['rows'] if r[0] == RX))
        with self.assertRaises(AssertionError): analyze(host, android)
        host, android = fixture()
        next(r for r in host['rows'] if r[0] == AWAKE)[5] = 0
        with self.assertRaises(AssertionError): analyze(host, android)

    def test_loss_duplicate_tag_and_inverted_intervals_rejected(self):
        host, android = fixture()
        for field in ['loss', 'duplicate', 'inverted']:
            h, a = copy.deepcopy(host), copy.deepcopy(android)
            if field == 'loss': a['dropped'] = 1
            if field == 'duplicate': h['rows'].append(next(r for r in h['rows'] if r[0] == START))
            if field == 'inverted': a['rows'][0][3] = a['rows'][0][4] + 1
            with self.assertRaises(AssertionError): analyze(h, a)


if __name__ == '__main__': unittest.main()
