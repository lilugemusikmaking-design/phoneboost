#!/usr/bin/env python3
"""Validate and summarize the bounded decrypt experiment; never sum clock domains."""
import hashlib
import json
from pathlib import Path
import statistics
import sys

from analyze_c12_timing import validate
from analyze_put_wait import analyze

ROOT = Path(__file__).resolve().parents[1]


def verify_traces(doc):
    for run in doc['apk_runs']:
        raw = []
        for side in ('host', 'android'):
            entry = run['traces'][side]
            path = (ROOT / entry['path']).resolve()
            assert path.is_relative_to(ROOT / 'docs/evidence/android-decrypt-v0-1')
            data = path.read_bytes()
            assert hashlib.sha256(data).hexdigest() == entry['sha256']
            raw.append(json.loads(data))
        assert analyze(*raw) == run['analysis']


def compare(doc):
    runs = doc['apk_runs']
    assert [r['label'] for r in runs] == [
        'warmupA', 'warmupB', 'pair1A', 'pair1B', 'pair2B', 'pair2A', 'pair3A', 'pair3B']
    host = doc['apk_build']['host_sha256']
    assert doc['apk_build']['apks']['baseline']['apk_sha256'] == 'efc70d8adb6e00302ae6ca8edfd5f6fee69f4466d30cad08f6c3b59c09791a6e'
    assert doc['apk_build']['apks']['neon']['apk_sha256'] == '685c05c10504f165556abcd201ea87ed655e46414d47d0554bf90ef0f2334bbb'
    assert doc['apk_build']['apks']['baseline']['target_rustflags'] == []
    assert doc['apk_build']['apks']['neon']['target_rustflags'] == ['--cfg', 'chacha20_force_neon']
    assert doc['apk_build']['apk_differing_entries'] == ['lib/arm64-v8a/libphoneboost_core_jni.so']
    assert all(a['measurement']['utc'] < b['measurement']['utc'] for a, b in zip(runs, runs[1:]))
    for run in runs:
        m = run['measurement']
        validate(m, separate_control_windows=True)
        assert m['valid'] and m['digest_exact'] and m['execution_source'] == 'REMOTE_SUCCESS'
        assert m['input_bytes'] == 64 * 2**20
        assert m['experiment']['binary_sha256'] == host
        mode = 'baseline' if run['label'][-1] == 'A' else 'neon'
        assert m['experiment']['mode'] == mode
        assert m['experiment']['warmup'] == run['label'].startswith('warmup')
        assert m['experiment']['apk_sha256'] == doc['apk_build']['apks'][mode]['apk_sha256']
        assert m['native']['configured_threads'] == 2 and m['native']['refusal'] == 0
        assert m['native']['safety_before'] == m['native']['safety_after'] == 0
        assert m['native']['power_save'] == 0
    selected = {r['label']: r['measurement'] for r in runs if r['label'].startswith('pair')}
    analyses = {r['label']: r['analysis'] for r in runs}
    reference = selected['pair1A']
    # Application record counts/bytes must match; background/control snapshots
    # remain independently bracketed, as in the existing timing validator.
    for m in selected.values():
        for name in ('Encrypt', 'WireWrite', 'PutLoop'):
            for field in ('calls', 'bytes'):
                assert m['metrics']['host'][name][field] == reference['metrics']['host'][name][field]
        android = m['metrics']['android']
        assert android['Decrypt']['calls'] == android['RxFrame']['calls'] == android['WireReadBody']['calls']
        assert android['Decrypt']['bytes'] == android['WireReadBody']['bytes']
        assert android['Decrypt']['bytes'] == android['RxFrame']['bytes'] + 16 * android['RxFrame']['calls']
        # Wider diagnostic windows can contain a different number of heartbeat
        # controls. Keep their raw counters; require exact application records.
        for field in ('calls', 'bytes'):
            assert (android['RxFrame'][field] - android['RxControl'][field]
                    == reference['metrics']['android']['RxFrame'][field] - reference['metrics']['android']['RxControl'][field])
    for label in selected:
        puts = analyses[label]['puts']
        assert len(puts) == 17 and sum(p['frames'] for p in puts) == 1105
        assert [(p['bytes'], p['frames']) for p in puts] == [(p['bytes'], p['frames']) for p in analyses['pair1A']['puts']]

    def values(m, analysis):
        metrics = m['metrics']
        ms = lambda side, name: metrics[side][name]['ns'] / 1e6
        put = ms('host', 'PutLoop')
        ticks = m['android_process_cpu']
        return {
            'end_to_end_ms': m['cli_process_ms'],
            'cli_timed_ms': ms('cli', 'CliTotal'),
            'decrypt_ms': analysis['total']['android_ms']['android_decrypt'],
            'decrypt_snapshot_ms': ms('android', 'Decrypt'),
            'session_poll_ms': ms('host', 'SessionPoll'),
            'put_ms': put,
            'put_mib_s': 64 / (put / 1000),
            'host_cpu_ms': m['host_process_cpu_ms'],
            'android_cpu_ms': (ticks['after_ticks'] - ticks['before_ticks']) * 1000 / ticks['clock_ticks_per_second'],
        }
    a = [values(selected[f'pair{i}A'], analyses[f'pair{i}A']) for i in range(1, 4)]
    b = [values(selected[f'pair{i}B'], analyses[f'pair{i}B']) for i in range(1, 4)]
    med = lambda rows: {k: statistics.median(r[k] for r in rows) for k in rows[0]}
    pairs = [{
        'pair': i + 1,
        'decrypt_ratio': left['decrypt_ms'] / right['decrypt_ms'],
        'end_to_end_ratio': left['end_to_end_ms'] / right['end_to_end_ms'],
        'decrypt_faster': right['decrypt_ms'] < left['decrypt_ms'],
        'end_to_end_faster': right['end_to_end_ms'] < left['end_to_end_ms'],
    } for i, (left, right) in enumerate(zip(a, b))]
    return {'baseline_median': med(a), 'neon_median': med(b), 'pairs': pairs,
            'valid_pairs': 3, 'warmups': 2,
            'cpu_scope': 'ADB-bracketed Android process ticks include capture overhead; host process ticks bracket CLI',
            'clock_domains': 'independent; overlapping views must not be added'}


if __name__ == '__main__':
    document = json.loads(Path(sys.argv[1]).read_text())
    verify_traces(document)
    print(json.dumps(compare(document), indent=2))
