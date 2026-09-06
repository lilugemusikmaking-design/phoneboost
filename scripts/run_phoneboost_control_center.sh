#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf 'PHONEBOOST_CONTROL_CENTER FAIL: %s\n' "$1" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "required command unavailable: $1"
}

repository_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)"
cd "$repository_root"

build=true
if [[ "${1:-}" == "--no-build" ]]; then
    build=false
    shift
fi
[[ "$#" -eq 0 ]] || fail "usage: scripts/run_phoneboost_control_center.sh [--no-build]"

rust_198_bin="${HOME}/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin"
phoneboostctl_bin=${PHONEBOOSTCTL_BIN:-target/release/phoneboostctl}
phoneboostd_bin=${PHONEBOOSTD_BIN:-target/release/phoneboostd}
bridge_bin=${PHONEBOOST_WEB_BRIDGE_BIN:-target/release/phoneboost-web-bridge}

if [[ "$build" == true ]]; then
    [[ -x "$rust_198_bin/cargo" ]] || fail "Rust 1.98 cargo unavailable at $rust_198_bin/cargo"
    PATH="$rust_198_bin:$PATH"
    export PATH
    require_command cargo
    require_command yarn

    printf '%s\n' 'Building the checked-in production frontend and PhoneBoost binaries...'
    REACT_APP_BACKEND_URL= yarn --cwd frontend build
    cargo build --release \
        -p pb-host --bin phoneboostd \
        -p pb-cli --bin phoneboostctl \
        -p pb-web-bridge --bin phoneboost-web-bridge
fi

[[ -x "$phoneboostctl_bin" ]] || fail "production phoneboostctl binary is not executable"
[[ -x "$phoneboostd_bin" ]] || fail "production phoneboostd binary is not executable"
[[ -x "$bridge_bin" ]] || fail "production web bridge binary is not executable"
[[ -f frontend/build/index.html ]] || fail "production frontend build is unavailable"
require_command timeout

query_status() {
    timeout --signal=TERM --kill-after=1s 1s "$phoneboostctl_bin" status
}

temporary_root="$(mktemp -d)"
chmod 700 "$temporary_root"
daemon_pid=""
bridge_pid=""

process_is_reapable() {
    local pid="$1"
    local process_state

    kill -0 "$pid" 2>/dev/null || return 0
    [[ -r "/proc/$pid/stat" ]] || return 1
    read -r _ _ process_state _ <"/proc/$pid/stat" || return 0
    [[ "$process_state" == "Z" ]]
}

stop_owned_process() {
    local pid="$1"
    [[ -n "$pid" ]] || return 0
    process_is_reapable "$pid" && {
        wait "$pid" 2>/dev/null || true
        return 0
    }

    kill "$pid" 2>/dev/null || true
    for _ in $(seq 1 20); do
        process_is_reapable "$pid" && {
            wait "$pid" 2>/dev/null || true
            return 0
        }
        sleep 0.05
    done

    kill -KILL "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
}

cleanup() {
    stop_owned_process "$bridge_pid"
    stop_owned_process "$daemon_pid"
    rm -rf -- "$temporary_root"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

status_file="$temporary_root/status.txt"
daemon_log="$temporary_root/daemon.log"

if ! query_status >"$status_file" 2>/dev/null; then
    printf '%s\n' 'Starting the production PhoneBoost daemon...'
    "$phoneboostd_bin" --foreground >"$daemon_log" 2>&1 &
    daemon_pid="$!"

    daemon_ready=false
    daemon_deadline=$((SECONDS + 5))
    while (( SECONDS < daemon_deadline )); do
        if query_status >"$status_file" 2>/dev/null; then
            daemon_ready=true
            break
        fi
        kill -0 "$daemon_pid" 2>/dev/null \
            || fail "production daemon exited before its local API became reachable"
        sleep 0.1
    done
    [[ "$daemon_ready" == true ]] \
        || fail "bounded wait expired before the production daemon became reachable"
else
    printf '%s\n' 'Using the production PhoneBoost daemon that is already running.'
fi

cat "$status_file"
printf '%s\n' \
    'Starting the loopback-only Control Center...' \
    'Open the one-time local URL printed below. Keep this terminal open; press Ctrl+C to stop.'

"$bridge_bin" &
bridge_pid="$!"
wait "$bridge_pid"
bridge_pid=""
