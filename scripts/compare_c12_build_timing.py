#!/usr/bin/env python3
"""Validate historical and alternating dev/release proofs without a workload.

Rows are independent medians, not an additive partition. Paired speedups are
reported separately when the explicitly recorded ABAB control is present.
For durations the ratio is dev/release; for rates it is release/dev.
"""
import argparse
import hashlib
import json
from pathlib import Path
import statistics

from analyze_c12_timing import critical_partition, validate


def values(run):
    metrics = run['metrics']
    ms = lambda source,name: metrics[source][name]['ns']/1e6
    noise = metrics['host']['Encrypt']
    return {
        'end_to_end_ms': run['cli_process_ms'],
        'noise_host_ms': ms('host','Encrypt'),
        'noise_ms_per_frame': ms('host','Encrypt')/noise['calls'],
        'noise_mib_s': noise['bytes']/2**20/(noise['ns']/1e9),
        'total_excluding_noise_ms': run['cli_process_ms']-ms('host','Encrypt'),
        'put_loop_ms': ms('host','PutLoop'),
        'put_useful_mib_s': run['input_bytes']/2**20/(ms('host','PutLoop')/1000),
        'session_wait_ms': ms('host','SessionPoll'),
        'socket_write_ms': ms('host','WireWrite'),
        'android_decrypt_ms': ms('android','Decrypt'),
        'android_session_poll_ms': ms('android','SessionPoll'),
        'android_handler_poll_ms': ms('android','BufferHandlerPoll')+ms('android','ComputeHandlerPoll'),
        'native_compute_ms': run['native']['wall_ns']/1e6,
        'residual_ms': critical_partition(run)['UnattributedLocalAndProcess'],
        'end_to_end_useful_mib_s': run['input_bytes']/2**20/(run['cli_process_ms']/1000),
    }


def compare(dev, release):
    assert release['input'] == dev['input']
    assert release['transport'] == dev['transport']
    for field in ('model', 'android', 'api', 'build'):
        assert release['device'][field] == dev['device'][field]
    assert release['build']['apk_sha256'] == dev['build']['apk_sha256']
    assert release['build']['source_files'] == dev['build']['source_files']
    assert release['build']['logic_changed'] is False
    assert release['build']['source_head'] == release['dev_evidence']['commit']
    lock = next(f['sha256'] for f in dev['build']['source_files'] if f['path'] == 'Cargo.lock')
    assert release['build']['cargo_lock_sha256'] == lock
    dev_runs = release.get('paired_dev_runs', dev['runs'])
    groups = [dev_runs, release['runs']]
    for runs in groups:
        assert len(runs) == 3
        assert len({r['label'] for r in runs}) == 3
        for run in runs:
            validate(run)
    if 'paired_dev_runs' in release:
        assert release['dev_evidence']['physical_dev_runs_repeated'] == 4  # warmup + three pairs
        warmups = release['warmups']
        assert len(warmups) == 2
        expected_order = warmups + [run for pair in zip(*groups) for run in pair]
        assert release['campaign_order'] == [run['label'] for run in expected_order]
        assert [r['utc'] for r in expected_order] == sorted(r['utc'] for r in expected_order)
        for index,run in enumerate(expected_order):
            validate(run, warmup_control_tail=run.get('warmup_control_tail',0))
            profile = 'dev' if index % 2 == 0 else 'release'
            meta = run['experiment']
            assert meta['cargo_profile'] == profile
            assert meta['warmup'] == (index < 2)
            assert meta['pair'] == (0 if index < 2 else index//2)
            identity = dev['build'] if profile == 'dev' else release['build']
            assert meta['binary_sha256'] == identity['host_sha256']
            for point in ('before','after'):
                host = run['host_environment'][point]
                assert host['cpu_model'] and host['loadavg'] and host['policies']
                assert any(p['type']=='Mains' and p['online']=='1' for p in host['power'])
    else:
        assert release['dev_evidence']['physical_dev_runs_repeated'] == 0
    groups = [[values(r) for r in runs] for runs in groups]
    rows = []
    for name in groups[0][0]:
        medians = [statistics.median(r[name] for r in g) for g in groups]
        is_rate = name.endswith('_mib_s')
        rows.append(dict(metric=name, dev_median=medians[0], release_median=medians[1],
                         ratio=medians[1]/medians[0] if is_rate else medians[0]/medians[1],
                         ratio_definition='release/dev' if is_rate else 'dev/release'))
    def share(name):
        numbers = [100*r['metrics']['host'][name]['ns']/1e6/r['cli_process_ms']
                   for r in release['runs']]
        return dict(median=statistics.median(numbers), min=min(numbers), max=max(numbers))
    dev_total,release_total = rows[0]['dev_median'],rows[0]['release_median']
    result = dict(rows=rows, noise_share_percent=share('Encrypt'),
                session_wait_share_percent=share('SessionPoll'),
                end_to_end_speedup=dev_total/release_total,
                end_to_end_reduction_percent=100*(1-release_total/dev_total),
                saved_ms=dev_total-release_total,
                valid_dev_runs=3, valid_release_runs=3,
                interpretation='Independent medians; per-run shares computed before taking the median; nested host and Android durations are never added.')
    if 'paired_dev_runs' in release:
        pairs = [dict(pair=i+1,dev_label=a['label'],release_label=b['label'],
                      end_to_end_speedup=a['cli_process_ms']/b['cli_process_ms'],
                      noise_speedup=a['metrics']['host']['Encrypt']['ns']/b['metrics']['host']['Encrypt']['ns'])
                 for i,(a,b) in enumerate(zip(dev_runs,release['runs']))]
        result['paired'] = dict(runs=pairs, median_end_to_end_speedup=statistics.median(p['end_to_end_speedup'] for p in pairs),
                                favorable_pairs=sum(p['end_to_end_speedup']>1 for p in pairs),warmups_excluded=2)
        preliminary = {k:v for k,v in release.items()
                       if k not in ('grouped_preliminary','paired_dev_runs','warmups','campaign_order')}
        preliminary['runs'] = release['grouped_preliminary']['runs']
        preliminary['dev_evidence'] = dict(release['dev_evidence'],physical_dev_runs_repeated=0)
        assert compare(dev,preliminary) == release['grouped_preliminary']['comparison']
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dev', type=Path, required=True)
    parser.add_argument('--release', type=Path, required=True)
    args = parser.parse_args()
    dev_bytes = args.dev.read_bytes()
    release = json.loads(args.release.read_text())
    assert hashlib.sha256(dev_bytes).hexdigest() == release['dev_evidence']['sha256']
    root = Path(__file__).resolve().parents[1]
    for source in release['build']['source_files']:
        assert hashlib.sha256((root/source['path']).read_bytes()).hexdigest() == source['sha256']
    result = compare(json.loads(dev_bytes),release)
    if 'comparison' in release:
        assert result == release['comparison'], 'stored comparison does not match raw evidence'
    print(json.dumps(result,indent=2))


if __name__ == '__main__':
    main()
