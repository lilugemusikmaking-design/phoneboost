#!/usr/bin/env python3
"""Same real C12 outcome with diagnostic collection OFF/ON; no hardware claim."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time


def verify(daemon_bin, client_bin):
    results = []
    for enabled in ('0', '1'):
        with tempfile.TemporaryDirectory(prefix='pb-timing-test-') as directory:
            root = Path(directory)
            env = dict(os.environ, PHONEBOOST_TIMING_DIAGNOSTIC=enabled)
            for variable in ('XDG_RUNTIME_DIR', 'XDG_STATE_HOME', 'XDG_CACHE_HOME'):
                path = root / variable
                path.mkdir(mode=0o700)
                env[variable] = str(path)
            with tempfile.TemporaryFile() as log:
                daemon = subprocess.Popen([daemon_bin, '--foreground'], env=env, stdout=log, stderr=log)
                try:
                    deadline = time.monotonic() + 10
                    while not (Path(env['XDG_RUNTIME_DIR']) / 'phoneboost/control.sock').exists():
                        assert daemon.poll() is None and time.monotonic() < deadline
                        time.sleep(.025)
                    source = root / 'private-input'
                    pairs = []
                    for payload in (b'x' * 65537, b''):
                        source.write_bytes(payload)
                        with source.open('rb') as stream:
                            result = subprocess.run([client_bin, 'compute', 'blake3', '--stdin'],
                                                    stdin=stream, capture_output=True, text=True,
                                                    env=env, timeout=15)
                        assert result.returncode == (3 if payload else 1)
                        assert 'Execution source: REMOTE_SUCCESS' not in result.stdout
                        diagnostic = [line for line in result.stderr.splitlines() if line.startswith('PB_TIMING ')]
                        assert len(diagnostic) == int(enabled)
                        if diagnostic:
                            report = json.loads(diagnostic[0].removeprefix('PB_TIMING '))
                            assert set(report) == {'schema', 'source', 'metrics'}
                            assert report['source'] == 'cli' and report['schema'] == 1
                            assert 0 < len(report['metrics']) <= 128
                            assert all(len(row) == 4 and row[0].isalpha()
                                       and all(isinstance(n, int) and n >= 0 for n in row[1:])
                                       for row in report['metrics'])
                            assert str(root) not in diagnostic[0] and 'digest' not in diagnostic[0]
                            metrics = {row[0]: row[1:] for row in report['metrics']}
                            assert metrics['CliTotal'][0] == 1 and metrics['CliTotal'][1] > 0
                        errors = '\n'.join(line for line in result.stderr.splitlines()
                                           if not line.startswith('PB_TIMING '))
                        pairs.append((result.returncode, result.stdout, errors))
                    results.append(pairs)
                finally:
                    daemon.terminate()
                    daemon.wait(timeout=5)
    assert results[0] == results[1], 'diagnostic changed a response or exit status'
    print('PASS: OFF/ON C12 responses, fallback and input rejection identical; bounded numeric diagnostics only.')
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--daemon', required=True)
    parser.add_argument('--client', required=True)
    parser.add_argument('--reference-daemon')
    parser.add_argument('--reference-client')
    args = parser.parse_args()
    if bool(args.reference_daemon) != bool(args.reference_client):
        parser.error('both reference binaries are required together')
    actual = verify(args.daemon, args.client)
    if args.reference_daemon:
        expected = verify(args.reference_daemon, args.reference_client)
        assert actual == expected, 'build profile changed a response, digest or exit status'
        print('PASS: build profiles have identical stdout, error, digest, exit and fallback behavior.')


if __name__ == '__main__':
    main()
