#!/usr/bin/env python3
"""Bounded real-process C12 client proof; no physical/remote-success claim."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--daemon', default='target/debug/phoneboostd')
    parser.add_argument('--client', default='target/debug/phoneboostctl')
    args = parser.parse_args()
    daemon_bin = str(Path(args.daemon).resolve(strict=True))
    client_bin = str(Path(args.client).resolve(strict=True))
    with tempfile.TemporaryDirectory(prefix='pb-cli-import-') as temporary:
        root = Path(temporary)
        env = dict(os.environ)
        for name, directory in [('XDG_RUNTIME_DIR', 'runtime'), ('XDG_STATE_HOME', 'state'),
                                ('XDG_CACHE_HOME', 'cache'), ('XDG_DATA_HOME', 'data')]:
            path = root / directory
            path.mkdir(mode=0o700)
            env[name] = str(path)
        # No inherited manual endpoint, device mutation or local daemon reuse.
        with tempfile.TemporaryFile() as daemon_log:
            daemon = subprocess.Popen([daemon_bin, '--foreground'], env=env,
                                      stdout=daemon_log, stderr=subprocess.STDOUT)
            try:
                socket = root / 'runtime/phoneboost/control.sock'
                deadline = time.monotonic() + 10
                while not socket.exists():
                    if daemon.poll() is not None or time.monotonic() >= deadline:
                        raise AssertionError('isolated daemon failed to start')
                    time.sleep(0.025)

                def compute(source):
                    return subprocess.run([client_bin, 'compute', 'blake3', '--stdin'],
                                          stdin=source, env=env, capture_output=True,
                                          text=True, timeout=15, check=False)

                def cleaned():
                    # The daemon owns disconnect cleanup; wait for confirmed
                    # removal rather than claiming cleanup from client EOF alone.
                    spool = root / 'cache/phoneboost/import/v1'
                    deadline = time.monotonic() + 5
                    while list(spool.iterdir()):
                        if time.monotonic() >= deadline:
                            raise AssertionError('private import remained after disconnect')
                        time.sleep(0.025)

                source = root / 'source.bin'
                source.write_bytes(b'abc')
                with source.open('rb') as file:
                    result = compute(file)
                assert result.returncode == 3, 'unpaired daemon must report local fallback'
                assert result.stderr == ''
                fields = dict(line.split(': ', 1) for line in result.stdout.splitlines())
                assert set(fields) == {'BLAKE3 input', 'Input bytes', 'BLAKE3 digest',
                                       'Execution source', 'Auto-use reason'}
                assert fields['BLAKE3 input'] == 'PRIVATE_IMPORT_V1'
                assert fields['Input bytes'] == '3'
                assert fields['BLAKE3 digest'] == '6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85'
                assert fields['Execution source'] == 'LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE'
                assert fields['Auto-use reason'] != 'READY'
                cleaned()
                print('PASS real import -> exact BLAKE3 -> explicit local fallback -> cleanup')

                # More than a local copy/hash chunk, with no client-side payload allocation.
                source.write_bytes(b'x' * 65537)
                with source.open('rb') as file:
                    result = compute(file)
                assert result.returncode == 3 and 'Input bytes: 65537\n' in result.stdout
                cleaned()
                print('PASS multi-chunk input and cleanup')

                for size in [0, 134217729]:
                    with source.open('wb') as file:
                        file.truncate(size)
                    with source.open('rb') as file:
                        result = compute(file)
                    assert result.returncode == 1 and result.stdout == ''
                    assert str(root) not in result.stderr and 'digest' not in result.stderr
                    cleaned()
                print('PASS empty/oversized input rejected without a compute result')

                result = subprocess.run([client_bin, 'compute', 'blake3', '--stdin'],
                                        input=b'abc', env=env, capture_output=True,
                                        timeout=15, check=False)
                assert result.returncode == 1 and result.stdout == b''
                assert b'IMPORT_SOURCE_REJECTED' in result.stderr
                cleaned()
                print('PASS pipe rejected by existing descriptor policy')

                result = subprocess.run([client_bin, 'compute', 'blake3', str(source)],
                                        env=env, capture_output=True, timeout=5, check=False)
                assert result.returncode == 2 and result.stdout == b''
                print('PASS arbitrary path argument rejected')
            finally:
                daemon.terminate()
                try:
                    daemon.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    daemon.kill()
                    daemon.wait(timeout=5)


if __name__ == '__main__':
    main()
