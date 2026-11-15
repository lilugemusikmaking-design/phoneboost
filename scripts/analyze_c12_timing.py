#!/usr/bin/env python3
"""Validate recorded diagnostic durations and print non-overlapping C12 partitions.

This does not execute workloads, synchronize clocks, or add Android durations to
Linux intervals. The representative run is the median end-to-end run, so its
partition sums exactly; a sum of independent per-phase medians would not.
"""
import argparse
import json
from pathlib import Path
import statistics

HOST_PHASES = ('HostLock', 'HostPlacement', 'BufferReserveCommit', 'BufferAlloc',
               'PutLoop', 'BufferStat', 'ScratchReserveCommit', 'ComputeSubmit',
               'ComputeStatus', 'ComputePollSleep', 'RemoteCleanup')
CLI_PHASES = ('CliConnect', 'CliCreate', 'CliReadyWait', 'CliSubmit', 'CliResult')
SESSION_PHASES = ('RequestBuild', 'FrameEncode', 'Encrypt', 'WireWrite',
                  'WireReadPrefix', 'WireReadBody', 'Decrypt', 'FrameDecode',
                  'Reassembly', 'SessionPoll')


def validate(run, *, warmup_control_tail=0):
    assert run['valid'] and run['returncode'] == 0 and run['digest_exact']
    assert run['execution_source'] == 'REMOTE_SUCCESS' and run['input_bytes'] == 64 * 2**20
    metrics = run['metrics']
    for source in ('cli', 'host', 'android'):
        assert 0 < len(metrics[source]) <= 128
        assert all(set(m) == {'calls', 'ns', 'bytes'}
                   and all(type(v) is int and v >= 0 for v in m.values())
                   for m in metrics[source].values())
    host, android, cli = (metrics[s] for s in ('host', 'android', 'cli'))
    assert host['PutFull']['calls'] == 16 and host['PutTail']['calls'] == 1
    assert host['PutFull']['bytes'] == 16 * (4 * 2**20 - 64)
    assert host['PutTail']['bytes'] == 1024
    for name in ('PutLoop', 'PutPrepare'):
        assert host[name]['bytes'] == run['input_bytes']
    assert android['WorkerPutCopy']['bytes'] == run['input_bytes']
    assert android['WorkerPutCopy']['calls'] == host['PutPrepare']['calls'] == 17
    assert host['TxFrame']['calls'] == android['RxFrame']['calls']
    # A separately recorded, excluded warmup has one 40-byte control frame
    # in the larger Android snapshot window. Measured runs stay strict.
    assert warmup_control_tail in (0, 1)
    if warmup_control_tail:
        assert run['experiment']['warmup'] is True
    assert host['RxFrame']['calls'] + warmup_control_tail == android['TxFrame']['calls']
    assert host['RxFrame']['bytes'] + 40*warmup_control_tail == android['TxFrame']['bytes']
    assert host['RxControl']['calls'] + warmup_control_tail == android['TxControl']['calls']
    assert host['RxControl']['bytes'] + 40*warmup_control_tail == android['TxControl']['bytes']
    assert host['TxFrame']['bytes'] == android['RxFrame']['bytes']
    assert host['TxFrame']['calls'] - host['TxControl']['calls'] == 1113
    assert host['RxFrame']['calls'] - host['RxControl']['calls'] == 25
    assert host['WireWrite']['bytes'] == (android['WireReadPrefix']['bytes']
                                          + android['WireReadBody']['bytes'])
    assert host['LeaseAcquire']['calls'] == 0
    assert host['ComputeStatus']['calls'] == host['ComputePollSleep']['calls'] == 0
    # READY may be observed on a different poll under another build/host load.
    # Successful imports have one terminal poll and one sleep per RECEIVING.
    assert cli['CliPoll']['calls'] >= 1
    assert cli['CliPollSleep']['calls'] + 1 == cli['CliPoll']['calls']
    assert host['HostTotal']['ns'] >= sum(host[p]['ns'] for p in HOST_PHASES)
    assert cli['CliTotal']['ns'] >= sum(cli[p]['ns'] for p in CLI_PHASES)
    assert host['PutLoop']['ns'] >= sum(host[p]['ns'] for p in ('PutPrepare', 'PutFull', 'PutTail'))
    assert run['cli_process_ms'] * 1e6 >= cli['CliTotal']['ns']
    native = run['native']
    assert native['coherent'] and native['outcome'] == native['refusal'] == 0
    assert native['configured_threads'] == native['executed_threads'] == 2
    assert native['input_bytes'] == run['input_bytes']
    assert native['safety_before'] == native['safety_after'] == native['power_save'] == 0
    assert 0 <= native['age_before_ms'] <= native['age_after_ms'] <= 6000
    for point in ('before', 'after'):
        assert run['health'][point]['ready']
        assert 0 <= run['health'][point]['effective_sample_age_ms'] <= 6000
    samples = run['health']['samples_during']
    assert samples and all(r['result'] == r['safety'] == r['thermal'] == '0'
                           and r['low_memory'] == r['power_save'] == 'false'
                           and int(r['budget_bytes']) >= 72 * 2**20 for r in samples)
    assert all(0 < int(b['at_ms'])-int(a['at_ms']) <= 6000 for a,b in zip(samples,samples[1:]))


def critical_partition(run):
    ms = lambda s,p: run['metrics'][s][p]['ns']/1e6
    total = run['cli_process_ms']
    rows = [(p, ms('cli',p)) for p in ('CliConnect','CliCreate','CliReadyWait')]
    rows += [('ImportConsume', ms('host','ImportConsume'))]
    rows += [(p, ms('host',p)) for p in HOST_PHASES]
    rows += [('CliResult', ms('cli','CliResult'))]
    residual = total - sum(duration for _,duration in rows)
    assert 0 <= residual < .01 * total
    rows.append(('UnattributedLocalAndProcess', residual))
    return dict(rows)


def summarize(document):
    runs = document['runs']
    assert len(runs) == 3 and len({r['label'] for r in runs}) == len(runs)
    for run in runs:
        validate(run)
    representative = sorted(runs, key=lambda r: r['cli_process_ms'])[len(runs)//2]
    ms = lambda s,p: representative['metrics'][s][p]['ns']/1e6
    total = representative['cli_process_ms']
    partition = critical_partition(representative)
    residual = partition['UnattributedLocalAndProcess']
    return dict(valid_runs=len(runs), representative=representative['label'], total_ms=total,
                total_range_ms=[min(r['cli_process_ms'] for r in runs), max(r['cli_process_ms'] for r in runs)],
                useful_mib_s=64000/total, critical_partition_ms=partition, residual_ms=residual,
                host_session_residual_ms=ms('cli','CliTotal')-sum(ms('host',p) for p in SESSION_PHASES),
                encrypt_share_percent=ms('host','Encrypt')/total*100,
                encrypt_share_range_percent=[min(r['metrics']['host']['Encrypt']['ns']/1e4/r['cli_process_ms'] for r in runs),max(r['metrics']['host']['Encrypt']['ns']/1e4/r['cli_process_ms'] for r in runs)],
                native_median_ms=statistics.median(r['native']['wall_ns']/1e6 for r in runs))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('evidence',type=Path)
    args=parser.parse_args()
    print(json.dumps(summarize(json.loads(args.evidence.read_text())),indent=2))


if __name__ == '__main__':
    main()
