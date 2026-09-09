#!/usr/bin/env bash
set -euo pipefail

# Bounded operator proof for the already-installed P3 user payload. It assumes
# the production daemon and authenticated Android worker are already ready. It
# never builds, starts a daemon, changes networking, or operates Android.
fail() {
    printf 'P3_INSTALLED_CONTROL_CENTER_PHYSICAL_PROOF FAIL: %s\n' "$1" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "required command unavailable: $1"
}

[[ "${HOME:-}" == /* ]] || fail "HOME must be an absolute path"
install_prefix=${PHONEBOOST_INSTALL_PREFIX:-"$HOME/.local"}
[[ "$install_prefix" == /* ]] || fail "installation prefix must be absolute"
install_prefix="$(realpath -m -- "$install_prefix")"
[[ "$install_prefix" != "/" && "$install_prefix" != "$(realpath -m -- "$HOME")" ]] \
    || fail "installation prefix is unsafe"

launcher="$install_prefix/bin/phoneboost-control-center"
application_root="$install_prefix/share/phoneboost"
phoneboostctl_bin="$application_root/libexec/phoneboostctl"
desktop_entry="$install_prefix/share/applications/org.phoneboost.ControlCenter.desktop"

[[ -x "$launcher" ]] || fail "installed Control Center command is unavailable"
[[ -x "$phoneboostctl_bin" ]] || fail "installed phoneboostctl is unavailable"
[[ -x "$application_root/libexec/phoneboostd" ]] || fail "installed phoneboostd is unavailable"
[[ -x "$application_root/libexec/phoneboost-web-bridge" ]] \
    || fail "installed web bridge is unavailable"
[[ -x "$application_root/scripts/run_phoneboost_control_center.sh" ]] \
    || fail "installed runtime launcher is unavailable"
[[ -f "$application_root/frontend/build/index.html" ]] \
    || fail "installed production frontend is unavailable"
[[ -f "$desktop_entry" ]] || fail "installed desktop entry is unavailable"
grep -Fqx "Exec=$launcher" "$desktop_entry" \
    || fail "desktop entry does not launch the installed command"
grep -Fqx 'Terminal=true' "$desktop_entry" \
    || fail "desktop entry does not keep a visible terminal"

require_command curl
require_command python3
require_command timeout

operator_confirmation_timeout_seconds=${PHONEBOOST_OPERATOR_CONFIRMATION_TIMEOUT_SECONDS:-300}
[[ "$operator_confirmation_timeout_seconds" =~ ^[0-9]+$ ]] \
    && ((operator_confirmation_timeout_seconds >= 1 \
        && operator_confirmation_timeout_seconds <= 3600)) \
    || fail "operator confirmation timeout must be an integer from 1 to 3600 seconds"

temporary_root="$(mktemp -d)"
chmod 700 "$temporary_root"
launcher_pid=""
launcher_output_fd=""
base_url=""
capability=""

process_is_reapable() {
    local pid="$1"
    local process_state

    kill -0 "$pid" 2>/dev/null || return 0
    [[ -r "/proc/$pid/stat" ]] || return 1
    read -r _ _ process_state _ <"/proc/$pid/stat" || return 0
    [[ "$process_state" == "Z" ]]
}

bridge_is_reachable() {
    [[ -n "$base_url" && -n "$capability" ]] || return 1
    curl --disable --noproxy '*' --fail --silent --show-error --max-time 1 \
        -H "X-PhoneBoost-Bridge-Token: $capability" \
        "$base_url/bridge/v1/snapshot" >/dev/null 2>&1
}

wait_for_bridge_shutdown() {
    [[ -n "$base_url" && -n "$capability" ]] || return 0
    for _ in $(seq 1 20); do
        bridge_is_reachable || return 0
        sleep 0.05
    done
    return 1
}

stop_launcher() {
    if [[ -n "$launcher_pid" ]]; then
        if ! process_is_reapable "$launcher_pid"; then
            kill "$launcher_pid" 2>/dev/null || true
            for _ in $(seq 1 20); do
                process_is_reapable "$launcher_pid" && break
                sleep 0.05
            done
        fi

        if ! process_is_reapable "$launcher_pid"; then
            kill -KILL "$launcher_pid" 2>/dev/null || true
        fi
        wait "$launcher_pid" 2>/dev/null || true
        launcher_pid=""
    fi

    if [[ -n "$launcher_output_fd" ]]; then
        exec {launcher_output_fd}<&- || true
        launcher_output_fd=""
    fi

    wait_for_bridge_shutdown
}

cleanup() {
    local status=$?
    trap - EXIT INT TERM
    if ! stop_launcher; then
        printf '%s\n' \
            'P3_INSTALLED_CONTROL_CENTER_PHYSICAL_PROOF FAIL: installed bridge remained reachable after launcher shutdown' >&2
        status=1
    fi
    rm -rf -- "$temporary_root"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

status_file="$temporary_root/status.txt"
snapshot_file="$temporary_root/snapshot.json"
compute_file="$temporary_root/compute.json"
post_compute_snapshot_file="$temporary_root/post-compute-snapshot.json"
: >"$status_file"
: >"$snapshot_file"
: >"$compute_file"
: >"$post_compute_snapshot_file"
chmod 600 "$status_file" "$snapshot_file" "$compute_file" \
    "$post_compute_snapshot_file"

timeout --signal=TERM --kill-after=1s 2s "$phoneboostctl_bin" status >"$status_file" \
    || fail "installed CLI could not read the production daemon"

python3 - "$status_file" <<'PY' || fail "production daemon is not ready for the installed proof"
import sys

lines = open(sys.argv[1], encoding="utf-8").read().splitlines()
expected = [
    "PhoneBoost: READY",
    "Local API: ACTIVE",
    "Android worker: AUTHENTICATED",
    "Auto-use: AVAILABLE",
    "Auto-use reason: READY",
    "Remote BLAKE3: AVAILABLE",
]
raise SystemExit(0 if lines == expected else 1)
PY

coproc PHONEBOOST_LAUNCHER { exec "$launcher" 2>&1; }
launcher_pid="$PHONEBOOST_LAUNCHER_PID"
launcher_output_fd="${PHONEBOOST_LAUNCHER[0]}"
launch_url=""
launch_deadline=$((SECONDS + 5))
while ((SECONDS < launch_deadline)); do
    launcher_line=""
    if IFS= read -r -t 1 -u "$launcher_output_fd" launcher_line; then
        if [[ "$launcher_line" =~ ^http://127\.0\.0\.1:[0-9]+/#token=[0-9a-f]{64}$ ]]; then
            launch_url="$launcher_line"
            break
        fi
    fi
    process_is_reapable "$launcher_pid" \
        && fail "installed launcher exited before publishing a local URL"
done
[[ -n "$launch_url" ]] || fail "bounded wait expired before installed Control Center launch"

base_url="${launch_url%%/#token=*}"
capability="${launch_url##*#token=}"
[[ "$base_url" =~ ^http://127\.0\.0\.1:[0-9]+$ ]] \
    || fail "installed bridge did not bind literal IPv4 loopback"
[[ "$capability" =~ ^[0-9a-f]{64}$ ]] || fail "bridge capability shape invalid"

bridge_curl() {
    curl --disable --noproxy '*' --fail --silent --show-error --max-time 4 \
        -H "X-PhoneBoost-Bridge-Token: $capability" "$@"
}

read_operator_confirmation() {
    local expected="$1"
    local mismatch_message="$2"
    local confirmation

    if ! IFS= read -r -t "$operator_confirmation_timeout_seconds" confirmation; then
        fail "operator confirmation timed out or input closed"
    fi
    [[ "$confirmation" == "$expected" ]] || fail "$mismatch_message"
}

bridge_curl "$base_url/bridge/v1/snapshot" >"$snapshot_file" \
    || fail "installed bridge snapshot request failed"

python3 - "$snapshot_file" <<'PY' || fail "installed browser snapshot is not fresh, ready, and truthfully observable"
import json
import sys
import time

snapshot = json.load(open(sys.argv[1], encoding="utf-8"))
observed_at = snapshot.get("observed_at_unix_ms")
max_age = snapshot.get("max_age_ms")
now = time.time_ns() // 1_000_000
fresh = (
    type(observed_at) is int
    and type(max_age) is int
    and max_age == 3_000
    and 0 <= now - observed_at <= max_age
)

allowed_gate_pairs = {
    "discovery_observation": {
        ("FRESH_HINT", "C04_CANDIDATE_OBSERVED"),
        ("NO_HINT", "C04_NO_CANDIDATE"),
        ("BACKEND_UNAVAILABLE", "DISCOVERY_BACKEND_UNAVAILABLE"),
        ("STALE", "OBSERVATION_EXPIRED"),
        ("UNKNOWN", "EPOCH_INVALIDATED"),
        ("UNKNOWN", "NOT_OBSERVED"),
        ("UNKNOWN", "NOT_EXPOSED_BY_C12"),
    },
    "controller_lease": {
        ("ACTIVE", "C07_ACK_FRESH"),
        ("EXPIRED", "ACK_TTL_ELAPSED"),
        ("UNAVAILABLE", "C07_ACQUIRE_FAILED"),
        ("UNAVAILABLE", "C07_RENEW_FAILED"),
        ("UNAVAILABLE", "SESSION_INVALIDATED"),
        ("UNAVAILABLE", "IDENTITY_OR_INCARNATION_CHANGED"),
        ("UNAVAILABLE", "AUTO_USE_DISABLED"),
        ("UNKNOWN", "NOT_OBSERVED"),
        ("UNKNOWN", "NOT_EXPOSED_BY_C12"),
    },
    "resource_guard_admission_proof": {
        ("FRESH_PASS", "C08_C09_C10_PROBE_PASSED"),
        ("FAILED", "C08_C09_C10_PROBE_FAILED"),
        ("STALE", "PROOF_EXPIRED"),
        ("UNKNOWN", "SESSION_INVALIDATED"),
        ("UNKNOWN", "LEASE_INVALIDATED"),
        ("UNKNOWN", "IDENTITY_OR_INCARNATION_CHANGED"),
        ("UNKNOWN", "AUTO_USE_DISABLED"),
        ("UNKNOWN", "NOT_OBSERVED"),
        ("UNKNOWN", "NOT_EXPOSED_BY_C12"),
    },
}

def valid_gate(name):
    gate = snapshot.get(name)
    return (
        type(gate) is dict
        and set(gate) == {"state", "reason"}
        and (gate.get("state"), gate.get("reason")) in allowed_gate_pairs[name]
    )

valid = (
    fresh
    and snapshot.get("provenance") == "LIVE"
    and snapshot.get("local_daemon") == {
        "state": "REACHABLE", "runtime_state": "READY", "local_api_state": "ACTIVE"
    }
    and snapshot.get("authenticated_session", {}).get("state") == "AUTHENTICATED"
    and snapshot.get("authenticated_session", {}).get("remote_worker_state") == "AUTHENTICATED"
    and all(valid_gate(name) for name in allowed_gate_pairs)
    and snapshot.get("provider_readiness") == {
        "provider": "pb.native.blake3/1", "state": "AVAILABLE"
    }
    and snapshot.get("auto_use") == {"state": "AVAILABLE", "reason": "READY"}
    and snapshot.get("remote_blake3_available") is True
)
raise SystemExit(0 if valid else 1)
PY

python3 - "$snapshot_file" <<'PY'
import json
import sys

snapshot = json.load(open(sys.argv[1], encoding="utf-8"))
for name, label in (
    ("discovery_observation", "Discovery observation"),
    ("controller_lease", "Controller lease"),
    ("resource_guard_admission_proof", "Admission/readiness proof"),
):
    gate = snapshot[name]
    print(f"{label}: {gate['state']} / {gate['reason']}")
PY

printf '%s\n' "Open this one-time installed Control Center URL in a private browser window: $launch_url"
printf '%s\n' 'Confirm the page says LIVE and shows exactly the three P2 gate pairs printed above; type P3_LIVE.'
read_operator_confirmation P3_LIVE \
    "operator did not confirm the installed LIVE rendering"

bridge_curl -H 'Content-Type: application/json' \
    --data-binary '{"fixture":"c10-abc-v1"}' \
    "$base_url/bridge/v1/compute/blake3" >"$compute_file" \
    || fail "installed bridge compute request failed"
bridge_curl "$base_url/bridge/v1/snapshot" >"$post_compute_snapshot_file" \
    || fail "installed bridge post-compute snapshot failed"

python3 - "$compute_file" "$post_compute_snapshot_file" <<'PY' \
    || fail "installed bridge did not prove the canonical remote execution"
import json
import sys
import time

expected_digest = "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
compute = json.load(open(sys.argv[1], encoding="utf-8"))
snapshot = json.load(open(sys.argv[2], encoding="utf-8"))
now = time.time_ns() // 1_000_000
expected = {
    "fixture": "c10-abc-v1",
    "digest_blake3_hex": expected_digest,
    "execution_source": "REMOTE_SUCCESS",
    "auto_use_reason": "READY",
}
valid_compute = (
    compute.get("provenance") == "LIVE"
    and type(compute.get("observed_at_unix_ms")) is int
    and 0 <= now - compute["observed_at_unix_ms"] <= 3_000
    and compute.get("input_bytes") == 3
    and all(compute.get(key) == value for key, value in expected.items())
)
last_execution = snapshot.get("last_execution") or {}
valid_snapshot = (
    snapshot.get("provenance") == "LIVE"
    and snapshot.get("max_age_ms") == 3_000
    and type(snapshot.get("observed_at_unix_ms")) is int
    and 0 <= now - snapshot["observed_at_unix_ms"] <= snapshot["max_age_ms"]
    and type(last_execution.get("observed_at_unix_ms")) is int
    and 0 <= now - last_execution["observed_at_unix_ms"] <= snapshot["max_age_ms"]
    and all(last_execution.get(key) == value for key, value in expected.items())
)
raise SystemExit(0 if valid_compute and valid_snapshot else 1)
PY

printf '%s\n' 'Confirm the browser shows REMOTE_SUCCESS for c10-abc-v1; type REMOTE_SUCCESS.'
read_operator_confirmation REMOTE_SUCCESS \
    "operator did not confirm the installed remote result"

shutdown_failed=0
stop_launcher || shutdown_failed=1
base_url=""
capability=""
((shutdown_failed == 0)) \
    || fail "installed bridge remained reachable after launcher shutdown"
printf '%s\n' 'P3_INSTALLED_CONTROL_CENTER_PHYSICAL_PROOF PASS'
