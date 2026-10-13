#!/usr/bin/env python3
"""Isolated real daemon/CLI restart proof. No phone or user state is touched."""
import argparse
import json
import os
from pathlib import Path
import socket
import selectors
import subprocess
import tempfile
import time
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import Request, urlopen


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--daemon', default='target/debug/phoneboostd')
    parser.add_argument('--client', default='target/debug/phoneboostctl')
    parser.add_argument('--bridge', default='target/debug/phoneboost-web-bridge')
    args = parser.parse_args()
    daemon_bin = str(Path(args.daemon).resolve(strict=True))
    client_bin = str(Path(args.client).resolve(strict=True))
    bridge_bin = str(Path(args.bridge).resolve(strict=True))
    with tempfile.TemporaryDirectory(prefix='pb-auto-use-') as temporary, tempfile.TemporaryFile() as log:
        root = Path(temporary)
        env = dict(os.environ)
        for variable, name in [('XDG_RUNTIME_DIR', 'runtime'), ('XDG_STATE_HOME', 'state'),
                               ('XDG_CACHE_HOME', 'cache'), ('XDG_DATA_HOME', 'data')]:
            directory = root / name
            directory.mkdir(mode=0o700)
            env[variable] = str(directory)
        config = root / 'state/phoneboost/config.json'
        control_socket = root / 'runtime/phoneboost/control.sock'
        daemon = None

        def command(*arguments):
            return subprocess.run([client_bin, *arguments], env=env, text=True,
                                  capture_output=True, timeout=8, check=False)

        def start():
            nonlocal daemon
            daemon = subprocess.Popen([daemon_bin, '--foreground'], env=env,
                                      stdout=log, stderr=subprocess.STDOUT)
            deadline = time.monotonic() + 10
            while True:
                if daemon.poll() is not None:
                    raise AssertionError('isolated daemon failed to start')
                probe = command('auto-use', 'status')
                if probe.returncode in (0, 1) and 'Persistence:' in probe.stdout:
                    return probe
                if time.monotonic() >= deadline:
                    raise AssertionError('control was not reachable within startup deadline')
                time.sleep(0.025)

        def stop():
            if daemon is not None and daemon.poll() is None:
                daemon.terminate()
                try:
                    daemon.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    daemon.kill()
                    daemon.wait(timeout=5)

        def read_wire(method, params):
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
                connection.settimeout(5)
                connection.connect(str(control_socket))
                connection.sendall(json.dumps(dict(api=1, id='smoke', method=method, params=params)).encode() + b'\n')
                with connection.makefile('rb') as reader:
                    line = reader.readline(65537)
                assert line.endswith(b'\n') and len(line) <= 65536
                result = json.loads(line)
                assert result['id'] == 'smoke' and result['api'] == 1
                return result

        try:
            first = start()
            assert first.returncode == 0 and first.stdout == 'Auto-use: ON\nPersistence: DEFAULT\n'
            assert not config.exists(), 'read-only status must not persist default'
            result = command('auto-use', 'off')
            assert result.returncode == 0 and result.stdout == 'Auto-use: OFF\nPersistence: SAVED\n'
            assert json.loads(config.read_text()) == dict(schema_version=1, auto_use_enabled=False)
            assert config.stat().st_mode & 0o7777 == 0o600
            assert read_wire('system.status', {})['result']['auto_use_state'] == 'OFF'
            compute = command('compute', 'blake3', 'c10-abc-v1')
            assert compute.returncode == 3
            assert 'LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE' in compute.stdout
            assert '6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85' in compute.stdout
            print('PASS real ON default -> durable OFF -> actual controller OFF -> exact local fallback')

            stop()
            restarted = start()
            assert restarted.returncode == 0 and 'Auto-use: OFF\nPersistence: SAVED' in restarted.stdout
            assert read_wire('system.status', {})['result']['auto_use_state'] == 'OFF'
            assert command('auto-use', 'on').returncode == 0
            stop()
            restarted = start()
            assert restarted.returncode == 0 and 'Auto-use: ON\nPersistence: SAVED' in restarted.stdout
            print('PASS OFF and ON survive real process restart, not browser state')

            for params in ({}, {'enabled': 0}, {'enabled': 'false'}, {'enabled': False, 'path': '/tmp/x'}):
                assert read_wire('auto_use.set', params)['error']['code'] == 'LOCAL_BAD_REQUEST'
            assert read_wire('auto_use.get', {})['result'] == dict(enabled=True, persistence='SAVED')
            assert command('auto-use', 'off', 'extra').returncode == 2
            print('PASS invalid requests and CLI arguments do not mutate preference')

            # Real authenticated HTTP -> typed client -> daemon. The bridge's
            # static root is isolated too; no production frontend/server reuse.
            frontend = root / 'frontend/build'
            frontend.mkdir(parents=True)
            (frontend / 'index.html').write_text('<!doctype html><title>Isolated proof</title>')
            bridge = subprocess.Popen([bridge_bin], cwd=root, env=env, stdout=subprocess.PIPE,
                                      stderr=log, text=True, bufsize=1)
            try:
                deadline = time.monotonic() + 5
                launch = ''
                # Read bytes directly: no TextIO buffering hiding ready lines.
                with selectors.DefaultSelector() as selector:
                    selector.register(bridge.stdout, selectors.EVENT_READ)
                    while '#token=' not in launch:
                        remaining = deadline - time.monotonic()
                        assert remaining > 0 and selector.select(remaining), 'bridge startup deadline'
                        chunk = os.read(bridge.stdout.fileno(), 4096)
                        assert chunk, 'bridge exited before launch'
                        launch += chunk.decode()
                url = next(line for line in launch.splitlines() if line.startswith('http://'))
                parsed = urlsplit(url)
                origin = f'{parsed.scheme}://{parsed.netloc}'
                token = parsed.fragment.removeprefix('token=')

                def http(method, data=None, authorized=True):
                    headers = {'Origin': origin, 'Content-Type': 'application/json'}
                    if authorized:
                        headers['X-PhoneBoost-Bridge-Token'] = token
                    request = Request(origin + '/bridge/v1/auto-use', method=method,
                                      headers=headers, data=None if data is None else json.dumps(data).encode())
                    try:
                        response = urlopen(request, timeout=8)
                    except HTTPError as error:
                        response = error
                    with response:
                        return response.status, json.loads(response.read(65537))

                assert http('GET') == (200, dict(enabled=True, persistence='SAVED'))
                assert http('POST', {'enabled': False}) == (200, dict(enabled=False, persistence='SAVED'))
                assert read_wire('system.status', {})['result']['auto_use_state'] == 'OFF'
                assert http('POST', {'enabled': 1}) == (400, dict(error='BAD_REQUEST'))
                assert http('POST', {'enabled': True}, authorized=False)[0] == 401
                assert http('GET') == (200, dict(enabled=False, persistence='SAVED'))
                assert http('POST', {'enabled': True}) == (200, dict(enabled=True, persistence='SAVED'))
                print('PASS real authenticated HTTP ON/OFF controls daemon; malformed/unauthorized requests do not mutate')
            finally:
                bridge.terminate()
                try:
                    bridge.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    bridge.kill()
                    bridge.wait(timeout=5)
                bridge.stdout.close()

            stop()
            corrupt = '{"schema_version":99,"auto_use_enabled":true}'
            config.write_text(corrupt)
            restarted = start()
            assert restarted.returncode == 1 and 'Auto-use: OFF\nPersistence: ERROR' in restarted.stdout
            assert read_wire('system.status', {})['result']['auto_use_state'] == 'OFF'
            result = command('auto-use', 'on')
            assert result.returncode == 1 and result.stdout == ''
            assert str(root) not in result.stderr
            assert config.read_text() == corrupt
            assert command('status').returncode == 0, 'daemon remains observable'
            print('PASS newer config fails closed without repair; daemon stays observable')
        finally:
            stop()


if __name__ == '__main__':
    main()
