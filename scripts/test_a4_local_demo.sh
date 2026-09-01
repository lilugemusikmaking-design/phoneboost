#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
demo_runtime=$(mktemp -d /tmp/phoneboost-a4-local-demo.XXXXXX)
daemon_pid=""
fake_pid=""

cleanup() {
    if [ -n "$daemon_pid" ]; then
        kill "$daemon_pid" 2>/dev/null || true
        wait "$daemon_pid" 2>/dev/null || true
    fi
    if [ -n "$fake_pid" ]; then
        kill "$fake_pid" 2>/dev/null || true
        wait "$fake_pid" 2>/dev/null || true
    fi
    if [ -d "$demo_runtime" ]; then
        rm -r -- "$demo_runtime"
    fi
}
trap cleanup EXIT INT TERM

chmod 700 "$demo_runtime"
export CARGO_HOME="$repo_root/.tooling/cargo"
export RUSTUP_HOME="$repo_root/.tooling/rustup"
export PATH="$repo_root/.tooling/rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin:/usr/bin:/bin"
export CARGO_NET_OFFLINE=true

cargo build --locked -p pb-host --bin phoneboostd -p pb-cli --bin phoneboostctl >/dev/null

XDG_RUNTIME_DIR="$demo_runtime" "$repo_root/target/debug/phoneboostd" --foreground \
    >"$demo_runtime/daemon.stdout" 2>"$demo_runtime/daemon.stderr" &
daemon_pid=$!

attempt=0
while [ ! -S "$demo_runtime/phoneboost/control.sock" ]; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 100 ] || ! kill -0 "$daemon_pid" 2>/dev/null; then
        exit 1
    fi
    sleep 0.05
done

if [ "$(sed -n '1p' "$demo_runtime/daemon.stdout")" != "READY" ]; then
    exit 1
fi

expected='PhoneBoost: READY
Local API: ACTIVE
Android worker: NOT_CONFIGURED'
first=$(XDG_RUNTIME_DIR="$demo_runtime" "$repo_root/target/debug/phoneboostctl" status)
second=$(XDG_RUNTIME_DIR="$demo_runtime" "$repo_root/target/debug/phoneboostctl" status)
if [ "$first" != "$expected" ] || [ "$second" != "$expected" ]; then
    exit 1
fi

python3 - "$demo_runtime/phoneboost/control.sock" <<'PY'
import json
import socket
import sys
import time

socket_path = sys.argv[1]
max_line = 65_536

def connect():
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.settimeout(2)
    client.connect(socket_path)
    return client

def receive_line(client):
    line = bytearray()
    while True:
        chunk = client.recv(4096)
        if not chunk:
            raise AssertionError("unexpected EOF")
        newline = chunk.find(b"\n")
        if newline >= 0:
            line.extend(chunk[:newline])
            break
        line.extend(chunk)
        if len(line) > max_line:
            raise AssertionError("oversized response")
    return json.loads(line)

def request(client, request_id, method, params):
    body = {
        "api": 1,
        "id": request_id,
        "method": method,
        "params": params,
    }
    client.sendall(json.dumps(body, separators=(",", ":")).encode() + b"\n")
    response = receive_line(client)
    assert response["api"] == 1
    assert response["id"] == request_id
    assert isinstance(response["ok"], bool)
    return response

client = connect()
opaque_id = {"opaque": [1, None]}
status = request(client, opaque_id, "system.status", {})
assert status["ok"] is True
assert set(status["result"]) == {
    "runtime_state",
    "local_api_state",
    "api",
    "local_clients_active",
    "local_clients_max",
    "max_line_bytes",
    "idle_timeout_seconds",
    "remote_worker_state",
}
assert status["result"]["runtime_state"] == "READY"
assert status["result"]["local_api_state"] == "ACTIVE"
assert status["result"]["remote_worker_state"] == "NOT_CONFIGURED"

bad_params = request(
    client,
    "bad-params",
    "system.status",
    {"secret": "RAW_PARAMS_MUST_NEVER_APPEAR"},
)
assert bad_params["ok"] is False
assert bad_params["error"]["code"] == "LOCAL_BAD_REQUEST"

deferred = request(client, "deferred", "devices.list", {})
assert deferred["ok"] is False
assert deferred["error"]["code"] == "LOCAL_UNSUPPORTED_METHOD"
assert deferred["error"]["message_safe"] == "method unavailable in current build"

unknown = request(client, "unknown", "raw.pbmux.rpc", {})
assert unknown["ok"] is False
assert unknown["error"]["code"] == "LOCAL_UNSUPPORTED_METHOD"

client.sendall(b"{\n")
malformed = receive_line(client)
assert malformed["id"] is None
assert malformed["ok"] is False
assert malformed["error"]["code"] == "LOCAL_BAD_REQUEST"

second_status = request(client, "same-connection-2", "system.status", {})
assert second_status["ok"] is True
client.close()

held = [connect() for _ in range(8)]
for attempt in range(100):
    cap = request(held[0], f"cap-{attempt}", "system.status", {})
    if cap["result"]["local_clients_active"] == 8:
        break
    time.sleep(0.02)
else:
    raise AssertionError("eight-client cap was not observed")

ninth = connect()
ninth.sendall(
    b'{"api":1,"id":"ninth","method":"system.status","params":{}}\n'
)
try:
    ninth_result = ninth.recv(1)
except (ConnectionResetError, BrokenPipeError):
    ninth_result = b""
assert ninth_result == b""
ninth.close()
for held_client in held:
    held_client.close()
PY

third=$(XDG_RUNTIME_DIR="$demo_runtime" "$repo_root/target/debug/phoneboostctl" status)
if [ "$third" != "$expected" ]; then
    exit 1
fi
if grep -F 'RAW_PARAMS_MUST_NEVER_APPEAR' \
    "$demo_runtime/daemon.stdout" "$demo_runtime/daemon.stderr" >/dev/null; then
    exit 1
fi

kill "$daemon_pid"
wait "$daemon_pid" 2>/dev/null || true
daemon_pid=""
rm -f -- "$demo_runtime/phoneboost/control.sock"

python3 - "$demo_runtime/phoneboost/control.sock" mismatch <<'PY' &
import json
import os
import socket
import sys

path, mode = sys.argv[1:]
try:
    os.unlink(path)
except FileNotFoundError:
    pass
server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
server.bind(path)
server.listen(1)
connection, _ = server.accept()
request = bytearray()
while not request.endswith(b"\n"):
    request.extend(connection.recv(4096))
if mode == "mismatch":
    response = {
        "api": 1,
        "id": "wrong-id",
        "ok": True,
        "result": {
            "runtime_state": "READY",
            "local_api_state": "ACTIVE",
            "remote_worker_state": "NOT_CONFIGURED",
        },
    }
    connection.sendall(json.dumps(response, separators=(",", ":")).encode() + b"\n")
connection.close()
server.close()
os.unlink(path)
PY
fake_pid=$!
attempt=0
while [ ! -S "$demo_runtime/phoneboost/control.sock" ]; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 100 ]; then
        exit 1
    fi
    sleep 0.01
done
if XDG_RUNTIME_DIR="$demo_runtime" "$repo_root/target/debug/phoneboostctl" status \
    >"$demo_runtime/mismatch.stdout" 2>"$demo_runtime/mismatch.stderr"; then
    exit 1
fi
wait "$fake_pid"
fake_pid=""

python3 - "$demo_runtime/phoneboost/control.sock" malformed <<'PY' &
import os
import socket
import sys

path, mode = sys.argv[1:]
assert mode == "malformed"
server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
server.bind(path)
server.listen(1)
connection, _ = server.accept()
request = bytearray()
while not request.endswith(b"\n"):
    request.extend(connection.recv(4096))
connection.sendall(b"{\n")
connection.close()
server.close()
os.unlink(path)
PY
fake_pid=$!
attempt=0
while [ ! -S "$demo_runtime/phoneboost/control.sock" ]; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 100 ]; then
        exit 1
    fi
    sleep 0.01
done
if XDG_RUNTIME_DIR="$demo_runtime" "$repo_root/target/debug/phoneboostctl" status \
    >"$demo_runtime/malformed.stdout" 2>"$demo_runtime/malformed.stderr"; then
    exit 1
fi
wait "$fake_pid"
fake_pid=""

if XDG_RUNTIME_DIR="$demo_runtime" "$repo_root/target/debug/phoneboostctl" status \
    >"$demo_runtime/absent.stdout" 2>"$demo_runtime/absent.stderr"; then
    exit 1
fi

printf '%s\n' 'Terminal 1:' '$ phoneboostd --foreground' 'READY' '' \
    'Terminal 2:' '$ phoneboostctl status' "$first"
