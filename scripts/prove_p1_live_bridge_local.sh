#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf 'P1_LIVE_BRIDGE_LOCAL_PROOF FAIL: %s\n' "$1" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "required command unavailable: $1"
}

confirm() {
    local prompt="$1"
    local expected="$2"
    local answer
    printf '%s\n> ' "$prompt"
    IFS= read -r answer
    [[ "$answer" == "$expected" ]] || fail "operator confirmation did not match $expected"
}

repository_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)"
cd "$repository_root"

rust_198_bin="${HOME}/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin"
[[ -x "$rust_198_bin/cargo" ]] || fail "Rust 1.98 cargo unavailable at $rust_198_bin/cargo"
PATH="$rust_198_bin:$PATH"
export PATH

for command_name in cargo curl python3 yarn; do
    require_command "$command_name"
done

temporary_root="$(mktemp -d)"
chmod 700 "$temporary_root"
bridge_pid=""
cleanup() {
    if [[ -n "$bridge_pid" ]] && kill -0 "$bridge_pid" 2>/dev/null; then
        kill "$bridge_pid" 2>/dev/null || true
        wait "$bridge_pid" 2>/dev/null || true
    fi
    rm -rf -- "$temporary_root"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

printf '%s\n' 'Building the checked-in production frontend and local bridge...'
REACT_APP_BACKEND_URL= yarn --cwd frontend build
cargo build --release -p pb-cli -p pb-web-bridge

phoneboostctl="$repository_root/target/release/phoneboostctl"
bridge_binary="$repository_root/target/release/phoneboost-web-bridge"
[[ -x "$phoneboostctl" ]] || fail "production phoneboostctl binary missing"
[[ -x "$bridge_binary" ]] || fail "production bridge binary missing"

status_file="$temporary_root/status.txt"
snapshot_file="$temporary_root/snapshot.json"
bridge_log="$temporary_root/bridge.log"

"$phoneboostctl" status >"$status_file" || fail "production daemon is not already reachable"

"$bridge_binary" >"$bridge_log" 2>&1 &
bridge_pid="$!"
launch_url=""
for _ in $(seq 1 50); do
    launch_url="$(grep -m1 '^http://127\.0\.0\.1:[0-9][0-9]*/#token=[0-9a-f]\{64\}$' "$bridge_log" || true)"
    [[ -n "$launch_url" ]] && break
    kill -0 "$bridge_pid" 2>/dev/null || fail "bridge exited before publishing its launch URL"
    sleep 0.1
done
[[ -n "$launch_url" ]] || fail "bounded wait expired before bridge launch URL"

base_url="${launch_url%%/#token=*}"
capability="${launch_url##*#token=}"
[[ "$base_url" =~ ^http://127\.0\.0\.1:[0-9]+$ ]] || fail "bridge did not publish literal IPv4 loopback"
[[ "$capability" =~ ^[0-9a-f]{64}$ ]] || fail "bridge capability shape invalid"

fetch_snapshot() {
    curl --fail --silent --show-error \
        --max-time 4 \
        -H "X-PhoneBoost-Bridge-Token: $capability" \
        "$base_url/bridge/v1/snapshot" >"$snapshot_file"
}

snapshot_is_ready() {
    python3 - "$snapshot_file" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    value = json.load(stream)
ready = (
    value.get("provenance") == "LIVE"
    and value.get("authenticated_session", {}).get("state") == "AUTHENTICATED"
    and value.get("auto_use", {}).get("state") == "AVAILABLE"
    and value.get("auto_use", {}).get("reason") == "READY"
    and value.get("remote_blake3_available") is True
    and value.get("provider_readiness", {}).get("state") == "AVAILABLE"
)
raise SystemExit(0 if ready else 1)
PY
}

snapshot_is_remote_unavailable() {
    python3 - "$snapshot_file" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    value = json.load(stream)
unavailable = (
    value.get("provenance") == "LIVE"
    and value.get("remote_blake3_available") is False
    and value.get("provider_readiness", {}).get("state") == "UNAVAILABLE"
    and not (
        value.get("auto_use", {}).get("state") == "AVAILABLE"
        and value.get("auto_use", {}).get("reason") == "READY"
    )
)
raise SystemExit(0 if unavailable else 1)
PY
}

last_execution_time() {
    python3 - "$snapshot_file" "$1" "$2" <<'PY'
import json
import sys

expected_source = sys.argv[2]
minimum = int(sys.argv[3])
with open(sys.argv[1], encoding="utf-8") as stream:
    value = json.load(stream)
execution = value.get("last_execution") or {}
valid = (
    execution.get("fixture") == "c10-abc-v1"
    and execution.get("digest_blake3_hex")
        == "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
    and execution.get("execution_source") == expected_source
    and isinstance(execution.get("observed_at_unix_ms"), int)
    and execution["observed_at_unix_ms"] > minimum
)
if not valid:
    raise SystemExit(1)
print(execution["observed_at_unix_ms"])
PY
}

wait_for_snapshot() {
    local predicate="$1"
    for _ in $(seq 1 30); do
        if fetch_snapshot && "$predicate"; then
            return 0
        fi
        sleep 2
    done
    return 1
}

status_matches_snapshot() {
    python3 - "$status_file" "$snapshot_file" <<'PY'
import json
import sys

prefixes = {
    "PhoneBoost": "runtime_state",
    "Local API": "local_api_state",
    "Android worker": "remote_worker_state",
    "Auto-use": "auto_use_state",
    "Auto-use reason": "auto_use_reason",
    "Remote BLAKE3": "remote_blake3_available",
}
observed = {}
with open(sys.argv[1], encoding="utf-8") as stream:
    for line in stream:
        key, separator, value = line.rstrip("\n").partition(": ")
        if separator and key in prefixes:
            observed[prefixes[key]] = value
with open(sys.argv[2], encoding="utf-8") as stream:
    snapshot = json.load(stream)
expected = {
    "runtime_state": snapshot.get("local_daemon", {}).get("runtime_state"),
    "local_api_state": snapshot.get("local_daemon", {}).get("local_api_state"),
    "remote_worker_state": snapshot.get("authenticated_session", {}).get("remote_worker_state"),
    "auto_use_state": snapshot.get("auto_use", {}).get("state"),
    "auto_use_reason": snapshot.get("auto_use", {}).get("reason"),
    "remote_blake3_available": "AVAILABLE" if snapshot.get("remote_blake3_available") is True else "UNAVAILABLE",
}
raise SystemExit(0 if observed == expected else 1)
PY
}

refresh_cli_and_compare() {
    "$phoneboostctl" status >"$status_file" || fail "phoneboostctl status failed"
    fetch_snapshot || fail "bridge snapshot failed"
    status_matches_snapshot || fail "bridge snapshot does not match phoneboostctl status"
    cat "$status_file"
}

printf '%s\n' 'PHASE 1 — ready production path'
wait_for_snapshot snapshot_is_ready || fail "bridge did not observe AVAILABLE / READY / true"
refresh_cli_and_compare
printf 'Open this one-time local URL in the browser:\n%s\n' "$launch_url"
confirm 'Confirm the page visibly says LIVE and matches the status above; type LIVE.' 'LIVE'
confirm 'Click “Run c10-abc-v1”, verify REMOTE_SUCCESS, then type REMOTE_SUCCESS.' 'REMOTE_SUCCESS'
fetch_snapshot
phase_one_time="$(last_execution_time REMOTE_SUCCESS 0)" || fail "fresh browser action was not REMOTE_SUCCESS"

printf '%s\n' 'PHASE 2 — operator-controlled disconnect'
confirm 'Disable Android Wi-Fi manually. This script performs no network mutation. Type WIFI_OFF.' 'WIFI_OFF'
wait_for_snapshot snapshot_is_remote_unavailable || fail "remote capability did not become unavailable"
refresh_cli_and_compare
confirm 'Verify the browser shows fresh LIVE state with remote BLAKE3 unavailable, then type UNAVAILABLE.' 'UNAVAILABLE'
confirm 'Click “Run c10-abc-v1”, verify LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE, then type FALLBACK.' 'FALLBACK'
fetch_snapshot
phase_two_time="$(last_execution_time LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE "$phase_one_time")" \
    || fail "disconnect action was missing, stale, or falsely labeled"
printf '%s\n' 'NO_FALSE_REMOTE_SUCCESS PASS'

printf '%s\n' 'PHASE 3 — operator-controlled recovery'
confirm 'Restore Android Wi-Fi manually. Type WIFI_ON.' 'WIFI_ON'
wait_for_snapshot snapshot_is_ready || fail "authenticated remote readiness did not recover"
refresh_cli_and_compare
confirm 'Verify the browser again shows fresh LIVE AVAILABLE / READY, then type RECOVERED.' 'RECOVERED'
confirm 'Click “Run c10-abc-v1”, verify REMOTE_SUCCESS, then type REMOTE_SUCCESS.' 'REMOTE_SUCCESS'
fetch_snapshot
last_execution_time REMOTE_SUCCESS "$phase_two_time" >/dev/null \
    || fail "recovered browser action was missing, stale, or not remote success"

printf '%s\n' 'P1_LIVE_BRIDGE_LOCAL_PROOF PASS'
