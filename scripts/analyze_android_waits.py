#!/usr/bin/env python3
"""Validate paired Android waits evidence; local clock durations, no RTT inference."""
import hashlib
import json
from pathlib import Path
import statistics
import sys

from analyze_c12_timing import validate
from analyze_put_wait import analyze, check_trace, RESP

ROOT = Path(__file__).resolve().parents[1]
WAIT_READY, WAIT_FINISHED = 1020, 1021
KINDS = {1: 'already_readable', 2: 'data', 3: 'timer', 4: 'partial', 5: 'closed_or_error'}
LABELS = ['warmupA', 'warmupB'] + [f'pair{i}{mode}' for i in range(1, 4) for mode in 'AB']


def wait_observations(trace):
    rows = check_trace(trace)
    responses = [r for r in rows if r[0] == RESP]
    assert len(responses) == 17, 'expected complete PUT set before snapshot tail'
    last_response = max(r[4] for r in responses)
    groups = {}
    for r in rows:
        if r[0] in (WAIT_READY, WAIT_FINISHED):
            assert r[1] == 0 and r[5] in KINDS
            key = r[2], r[3]  # local thread and start, never request or session ID
            assert r[0] not in groups.setdefault(key, {}), 'duplicate wait event'
            groups[key][r[0]] = r
    assert groups, 'missing readiness instrumentation'
    counts = {name: 0 for name in KINDS.values()}
    observation_ms = {name: 0.0 for name in KINDS.values()}
    tail_ms = {name: 0.0 for name in KINDS.values()}
    boundary_threads = set()
    spans = {}
    for (thread, start), pair in sorted(groups.items()):
        assert WAIT_READY in pair, 'wait finished without observation'
        ready = pair[WAIT_READY]
        assert start >= spans.get(thread, 0), 'overlapping waits on one thread'
        if WAIT_FINISHED not in pair:
            # Capture may occur between observation and return, after the job.
            # Never tolerate a missing wait end inside any PUT or two per thread.
            assert start > last_response and thread not in boundary_threads, 'in-job incomplete wait'
            assert start == max(s for t, s in groups if t == thread), 'not final wait'
            boundary_threads.add(thread)
            continue
        end = pair[WAIT_FINISHED]
        assert ready[5] == end[5] and start <= ready[4] <= end[4]
        spans[thread] = end[4]
        name = KINDS[end[5]]
        counts[name] += 1
        observation_ms[name] += (ready[4] - start) / 1e6
        tail_ms[name] += (end[4] - ready[4]) / 1e6
    return dict(observations=counts, observation_ms=observation_ms,
                post_observation_ms=tail_ms,
                data_ready_to_return_ms=tail_ms['already_readable'] + tail_ms['data'],
                observed_session_threads=len({t for t,s in groups}),
                snapshot_tail_incomplete_waits=len(boundary_threads),
                semantics='sum of all complete session waits in capture, including controls, across observed threads; not PUT/wall latency; readiness observed locally, not kernel arrival; baseline returns after legacy quantum')


def verify_traces(doc):
    for run in doc['runs']:
        traces = []
        for side in ('host', 'android'):
            entry = run['traces'][side]
            path = (ROOT / entry['path']).resolve()
            assert path.is_relative_to(ROOT / 'docs/evidence/android-waits-v0-1')
            raw = path.read_bytes()
            assert hashlib.sha256(raw).hexdigest() == entry['sha256']
            traces.append(json.loads(raw))
        assert analyze(*traces) == run['analysis']
        assert wait_observations(traces[1]) == run['waits']


def metrics(run):
    m, a, w = run['measurement'], run['analysis']['total'], run['waits']
    ms = lambda side, name: m['metrics'][side][name]['ns'] / 1e6
    ticks = m['android_process_cpu']
    assert ticks['after_ticks'] >= ticks['before_ticks'] >= 0
    put = ms('host', 'PutLoop')
    return dict(end_to_end_ms=m['cli_process_ms'], put_ms=put,
                put_mib_s=64000 / put, end_to_end_mib_s=64000 / m['cli_process_ms'],
                host_session_poll_ms=ms('host', 'SessionPoll'),
                android_receive_poll_ms=ms('android', 'SessionPoll'),
                android_receive_poll_in_put_ms=a['android_ms']['android_session_poll_in_receive'],
                android_receive_read_ms=a['android_ms']['android_receive_read'],
                android_body_read_snapshot_ms=ms('android', 'WireReadBody'),
                handler_completion_ms=a['android_ms']['android_handler_completion_poll_delay'],
                handler_execution_ms=a['android_ms']['android_handler_execution'],
                copy_ms=a['android_ms']['android_copy'],
                decrypt_ms=a['android_ms']['android_decrypt'],
                decrypt_snapshot_ms=ms('android', 'Decrypt'),
                host_queue_before_ms=a['host_ms']['host_queue_before_send'],
                outside_android_core_ms=a['outside_android_core_ms'],
                session_capture_data_ready_tail_sum_ms=w['data_ready_to_return_ms'],
                data_observations=w['observations']['data'],
                timer_observations=w['observations']['timer'],
                readiness_returns=m['metrics']['android']['SessionReadableWake']['calls'],
                timer_returns=m['metrics']['android']['SessionTimeoutWake']['calls'],
                handler_wait_calls=m['metrics']['android']['BufferHandlerPoll']['calls'],
                host_cpu_ms=m['host_process_cpu_ms'],
                android_cpu_ms=(ticks['after_ticks']-ticks['before_ticks'])*1000/ticks['clock_ticks_per_second'])


def compare(doc):
    runs, build = doc['runs'], doc['apk_build']
    assert [r['label'] for r in runs] == LABELS
    assert all(a['measurement']['utc'] < b['measurement']['utc'] for a,b in zip(runs,runs[1:]))
    assert build['apk_differing_entries'] == ['lib/arm64-v8a/libphoneboost_core_jni.so']
    assert all(a['target_rustflags'] == ['--cfg', 'chacha20_force_neon'] for a in build['apks'].values())
    reference = runs[0]['measurement']['metrics']
    for run in runs:
        m, label = run['measurement'], run['label']
        validate(m, separate_control_windows=True, expected_threads=1, expected_power_save=True)
        mode = 'baseline' if label[-1] == 'A' else 'candidate'
        assert m['experiment']['mode'] == mode
        assert m['experiment']['warmup'] == label.startswith('warmup')
        assert m['experiment']['binary_sha256'] == build['host_sha256']
        assert m['experiment']['apk_sha256'] == build['apks'][mode]['apk_sha256']
        assert m['experiment']['source_sha'] == build['apks'][mode]['source_sha']
        assert len(run['analysis']['puts']) == 17
        assert sum(p['frames'] for p in run['analysis']['puts']) == 1105
        before = m['health']['before']
        assert before['hal_ready'] and before['no_thermal_override'] and before['usb_powered']
        assert before['recovery_observed'] and before['requested_profile'] == 1
        recovery = before['natural_recovery_window']
        assert len(recovery) == 7
        assert int(recovery[-1]['sample_worker_ms']) - int(recovery[0]['sample_worker_ms']) >= 10000
        assert all(int(r['memory_bytes']) >= 896*2**20 for r in recovery)
        for side in ('host', 'android'):
            for frame, control in (('TxFrame','TxControl'), ('RxFrame','RxControl')):
                for field in ('calls','bytes'):
                    actual, expected = m['metrics'][side], reference[side]
                    assert actual[frame][field]-actual[control][field] == expected[frame][field]-expected[control][field]
        android = m['metrics']['android']
        assert android['Decrypt']['calls'] == android['RxFrame']['calls'] == android['WireReadBody']['calls']
        assert android['Decrypt']['bytes'] == android['WireReadBody']['bytes']
        assert android['Decrypt']['bytes'] == android['RxFrame']['bytes'] + 16 * android['RxFrame']['calls']
    cleanup = doc['cleanup']
    assert all(cleanup[n] for n in ('battery_saver_restored', 'product_unchanged', 'experimental_app_stopped', 'host_daemon_absent', 'owned_forward_absent'))
    assert cleanup['initial_battery_saver'] is True and cleanup['restored_health']['power_save'] is True
    assert 0 <= cleanup['restored_health']['age_ms'] <= 6000
    assert cleanup['experimental_profile'] == 1 and cleanup['timing_enabled'] is False
    selected = {r['label']: metrics(r) for r in runs if r['label'].startswith('pair')}
    def distribution(mode):
        values = [selected[f'pair{i}{mode}'] for i in range(1,4)]
        return {k: dict(median=statistics.median(r[k] for r in values),
                        min=min(r[k] for r in values), max=max(r[k] for r in values)) for k in values[0]}
    pairs = []
    for i in range(1,4):
        a,b = (selected[f'pair{i}{mode}'] for mode in 'AB')
        pairs.append(dict(pair=i, baseline=a, candidate=b,
                          end_to_end_gain_pct=100*(1-b['end_to_end_ms']/a['end_to_end_ms']),
                          end_to_end_faster=b['end_to_end_ms'] < a['end_to_end_ms']))
    return dict(valid_pairs=3, warmups=2, baseline=distribution('A'), candidate=distribution('B'), pairs=pairs,
                paired_median_gain_pct=statistics.median(p['end_to_end_gain_pct'] for p in pairs),
                favorable_pairs=sum(p['end_to_end_faster'] for p in pairs),
                scope='Android CPU includes ADB capture; wait and host views overlap, never add clock domains')


if __name__ == '__main__':
    document = json.loads(Path(sys.argv[1]).read_text())
    verify_traces(document)
    summary = compare(document)
    assert summary == document['summary'], 'published summary differs from raw evidence'
    print(json.dumps(summary, indent=2))
