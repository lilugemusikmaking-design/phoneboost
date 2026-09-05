#!/usr/bin/env bash
set -euo pipefail

# Bounded, production-only operator proof. It performs one direct passive C12
# status read and one loopback bridge snapshot read (whose typed backend performs
# one further passive status read). It never starts a daemon,
# mutates networking, acquires/renews a lease, probes ResourceGuard, runs
# compute, or performs remote cleanup. Run it immediately after an existing
# production authentication/readiness transition if FRESH_PASS is expected.
fail() {
    printf 'P2_PASSIVE_GATE_OBSERVABILITY_PHYSICAL_PROOF FAIL: %s\n' "$1" >&2
    exit 1
}

repository_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)"
cd "$repository_root"
phoneboostctl_bin=${PHONEBOOSTCTL_BIN:-target/release/phoneboostctl}
bridge_bin=${PHONEBOOST_WEB_BRIDGE_BIN:-target/release/phoneboost-web-bridge}

[[ -x "$phoneboostctl_bin" ]] || fail "production phoneboostctl binary is not executable"
[[ -x "$bridge_bin" ]] || fail "production web bridge binary is not executable"
command -v curl >/dev/null 2>&1 || fail "curl is unavailable"
command -v python3 >/dev/null 2>&1 || fail "python3 is unavailable"

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
trap cleanup EXIT INT TERM

status_file="$temporary_root/status.txt"
bridge_log="$temporary_root/bridge.log"
snapshot_file="$temporary_root/snapshot.json"
"$phoneboostctl_bin" status >"$status_file" || fail "passive status read failed"

"$bridge_bin" >"$bridge_log" 2>&1 &
bridge_pid="$!"
launch_url=""
for _ in $(seq 1 50); do
    launch_url="$(grep -m1 '^http://127\.0\.0\.1:[0-9][0-9]*/#token=[0-9a-f]\{64\}$' "$bridge_log" || true)"
    [[ -n "$launch_url" ]] && break
    kill -0 "$bridge_pid" 2>/dev/null || fail "bridge exited before publishing a local URL"
    sleep 0.1
done
[[ -n "$launch_url" ]] || fail "bounded wait expired before local bridge launch"

base_url="${launch_url%%/#token=*}"
capability="${launch_url##*#token=}"
[[ "$base_url" =~ ^http://127\.0\.0\.1:[0-9]+$ ]] || fail "bridge did not bind literal IPv4 loopback"
[[ "$capability" =~ ^[0-9a-f]{64}$ ]] || fail "bridge capability shape invalid"
curl --fail --silent --show-error --max-time 4 \
    -H "X-PhoneBoost-Bridge-Token: $capability" \
    "$base_url/bridge/v1/snapshot" >"$snapshot_file"

if ! python3 - "$status_file" "$snapshot_file" <<'PY'
import json
import sys

status = open(sys.argv[1], encoding="utf-8").read().splitlines()
snapshot = json.load(open(sys.argv[2], encoding="utf-8"))
expected_lines = {
    "Discovery observation: FRESH_HINT (C04_CANDIDATE_OBSERVED)",
    "Controller lease: ACTIVE (C07_ACK_FRESH)",
    "Latest admission/readiness proof: FRESH_PASS (C08_C09_C10_PROBE_PASSED)",
}
expected_snapshot = (
    snapshot.get("provenance") == "LIVE"
    and snapshot.get("discovery_observation") == {
        "state": "FRESH_HINT", "reason": "C04_CANDIDATE_OBSERVED"
    }
    and snapshot.get("controller_lease") == {
        "state": "ACTIVE", "reason": "C07_ACK_FRESH"
    }
    and snapshot.get("resource_guard_admission_proof") == {
        "state": "FRESH_PASS", "reason": "C08_C09_C10_PROBE_PASSED"
    }
)
raise SystemExit(0 if expected_lines.issubset(status) and expected_snapshot else 1)
PY
then
    fail "CLI and browser snapshot did not show the same fresh passive observations"
fi

printf '%s\n' "Open this one-time local URL in a private browser window: $launch_url"
printf '%s\n' 'Confirm LIVE/fresh rendering of the three observations, then type P2_LIVE.'
IFS= read -r confirmation
[[ "$confirmation" == "P2_LIVE" ]] || fail "operator did not confirm the fresh browser rendering"
printf '%s\n' 'P2_PASSIVE_GATE_OBSERVABILITY_PHYSICAL_PROOF PASS'
