#!/usr/bin/env python3
"""Validate and compare recorded same-APK polling A/B; no hardware workload."""
import argparse
import json
from pathlib import Path
from statistics import median
from analyze_c12_timing import validate


def values(run):
    m=run['metrics']; ms=lambda s,p:m[s][p]['ns']/1e6
    android_cpu=run['android_process_cpu']
    idle=run['experiment']['idle']
    return {
        'end_to_end_ms':run['cli_process_ms'],
        'put_loop_ms':ms('host','PutLoop'),
        'session_wait_host_ms':ms('host','SessionPoll'),
        'session_poll_android_ms':ms('android','SessionPoll'),
        'handlers_poll_android_ms':ms('android','BufferHandlerPoll')+ms('android','ComputeHandlerPoll'),
        'encrypt_ms':ms('host','Encrypt'), 'android_decrypt_ms':ms('android','Decrypt'),
        'wire_write_ms':ms('host','WireWrite'),
        'put_mib_s':64000/ms('host','PutLoop'), 'end_to_end_mib_s':64000/run['cli_process_ms'],
        'host_cpu_ms':run['host_process_cpu_ms'],
        'android_process_cpu_ms':(android_cpu['after_ticks']-android_cpu['before_ticks'])*1000/android_cpu['clock_ticks_per_second'],
        'host_idle_cpu_percent':100*idle['host_cpu_ms']/idle['wall_ms'],
        'android_idle_cpu_percent':100*(idle['android_after_ticks']-idle['android_before_ticks'])*10/idle['wall_ms'],
        'waits':m['host']['SessionPoll']['calls'],
        'readable_wakes':m['host'].get('SessionReadableWake',{'calls':0})['calls'],
        'timer_wakes':m['host'].get('SessionTimeoutWake',m['host']['SessionPoll'])['calls'],
        'partial_waits':m['host'].get('SessionPartialWait',{'calls':0})['calls'],
        'alloc_ms':ms('host','BufferAlloc'), 'stat_ms':ms('host','BufferStat'),
        'release_cleanup_ms':ms('host','RemoteCleanup'),
        'compute_submit_ms':ms('host','ComputeSubmit'),
        'native_compute_ms':run['native']['wall_ns']/1e6,
    }


def summarize(doc):
    assert doc['apk_unchanged'] and doc['build']['cargo_lock_unchanged']
    expected=['warmup-baseline','warmup-new']+[f'pair{i:02d}-{p}' for i in range(1,4) for p in ('baseline','new')]
    assert [r['label'] for r in doc['runs']]==expected
    assert [r['utc'] for r in doc['runs']]==sorted(r['utc'] for r in doc['runs'])
    groups={'baseline':[],'new':[]}
    for r in doc['runs']:
        assert r['valid'] and r['digest_exact'] and r['execution_source']=='REMOTE_SUCCESS'
        e=r['experiment'];p=e['cargo_profile']
        assert e['binary_sha256']==doc['build']['baseline_binary_sha256' if p=='baseline' else 'host_sha256']
        assert e['warmup']==r['label'].startswith('warmup-')
        if e['warmup']: continue
        validate(r, separate_control_windows=True)
        assert e['pair']==int(r['label'][4:6])
        if p=='new':
            m=r['metrics']['host']
            assert m['SessionPoll']['calls']==sum(m[k]['calls'] for k in ('SessionReadableWake','SessionTimeoutWake','SessionPartialWait'))
        v=values(r)
        assert v['host_idle_cpu_percent'] < 5, 'idle CPU is not acceptable'
        groups[p].append((r['label'],v))
    rows=[]
    for key in groups['baseline'][0][1]:
        a,b=[median(v[key] for _,v in groups[p]) for p in ('baseline','new')]
        ratio=(b/a if key.endswith('_mib_s') else a/b) if a and b else None
        rows.append(dict(metric=key,baseline=a,new=b,ratio=ratio))
    pairs=[dict(pair=i+1,baseline_ms=a[1]['end_to_end_ms'],new_ms=b[1]['end_to_end_ms'],speedup=a[1]['end_to_end_ms']/b[1]['end_to_end_ms']) for i,(a,b) in enumerate(zip(groups['baseline'],groups['new']))]
    return dict(rows=rows,pairs=pairs,median_paired_speedup=median(p['speedup'] for p in pairs),favorable_pairs=sum(p['speedup']>1 for p in pairs),note='Independent medians, not additive. Android CPU uses wider ADB bracket and 10ms ticks. RELEASE is existing RemoteCleanup aggregate (buffer then reservation releases), not one isolated RPC.')


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('evidence',type=Path);args=parser.parse_args()
    doc=json.loads(args.evidence.read_text());result=summarize(doc)
    if 'comparison' in doc: assert result==doc['comparison']
    print(json.dumps(result,indent=2))
