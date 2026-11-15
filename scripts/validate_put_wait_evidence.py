#!/usr/bin/env python3
"""Recompute physical diagnostic proof from checked-in numeric local traces."""
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import sys
from analyze_put_wait import analyze
from analyze_c12_timing import validate

ROOT = Path(__file__).resolve().parents[1]


def verify(path):
    doc=json.loads(Path(path).read_text())
    assert doc['schema']==1 and len(doc['runs'])==4
    assert doc['build']['cargo_lock_sha256']=='788dd17646b9f725ca7e382e7d04819155f159038f0122c9ea86438de045e605'
    for item in doc['build']['sources']:
        source,digest=item['path'],item['sha256']
        data=subprocess.check_output(['git','show',doc['build']['source_head']+':'+source],cwd=ROOT)
        assert hashlib.sha256(data).hexdigest()==digest,source
    valid=[]
    for run in doc['runs']:
        measurement=run['measurement']
        validate(measurement,separate_control_windows=True)
        assert measurement['valid'] and measurement['digest_exact'] and measurement['execution_source']=='REMOTE_SUCCESS'
        assert measurement['experiment']['apk_sha256']==doc['build']['apk_sha256']
        assert measurement['experiment']['binary_sha256']==doc['build']['host_sha256']
        traces=[]
        for side in ('host','android'):
            item=run['traces'][side]
            file=(ROOT/item['path']).resolve();assert file.is_relative_to(ROOT/'docs/evidence/put-wait-v0-1')
            data=file.read_bytes();assert hashlib.sha256(data).hexdigest()==item['sha256']
            raw=data;assert hashlib.sha256(raw).hexdigest()==item['raw_sha256']
            traces.append(json.loads(raw))
        result=analyze(*traces)
        assert result==run['analysis'],run['label']
        if not run['calibration']:valid.append(run)
    assert len(valid)==3
    median_run=sorted(valid,key=lambda r:r['measurement']['metrics']['cli']['CliTotal']['ns'])[1]
    assert median_run['label']==doc['median_run']
    assert doc['cleanup']['battery_saver_restored'] and doc['cleanup']['experimental_profile']==1
    assert doc['cleanup']['product_unchanged'] and doc['cleanup']['experimental_app_stopped']
    assert doc['cleanup']['host_daemon_absent'] and doc['cleanup']['owned_forward_absent']
    print(json.dumps(dict(valid_runs=3,calibration=1,median_run=doc['median_run'],
        median_cli_ms=statistics.median(r['measurement']['metrics']['cli']['CliTotal']['ns']/1e6 for r in valid),
        trace_completeness='PASS',clock_domains='independent',cleanup='verified'),indent=2))
    return doc


if __name__=='__main__':verify(sys.argv[1])
