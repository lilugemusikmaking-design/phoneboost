#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf 'P3_INSTALLED_CONTROL_CENTER_PROOF_TEST FAIL: %s\n' "$1" >&2
    exit 1
}

repository_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)"
proof="$repository_root/scripts/prove_p3_installed_control_center_physical.sh"
[[ -x "$proof" ]] || fail "proof script is not executable"

test_root="$(mktemp -d)"
cleanup() {
    rm -rf -- "$test_root"
}
trap cleanup EXIT INT TERM

fake_home="$test_root/home"
prefix="$fake_home/.local"
app="$prefix/share/phoneboost"
fake_bin="$test_root/bin"
mkdir -p "$prefix/bin" "$prefix/share/applications" "$app/libexec" \
    "$app/scripts" "$app/frontend/build" "$fake_bin"

cat >"$app/libexec/phoneboostctl" <<'SH'
#!/usr/bin/env bash
cat <<'EOF'
PhoneBoost: READY
Local API: ACTIVE
Android worker: AUTHENTICATED
Auto-use: AVAILABLE
Auto-use reason: READY
Remote BLAKE3: AVAILABLE
EOF
if [[ "${PHONEBOOST_TEST_STATUS_MODE:-}" == "extra" ]]; then
    printf '%s\n' 'Android worker: NOT_CONFIGURED'
fi
SH
cat >"$app/libexec/phoneboostd" <<'SH'
#!/usr/bin/env bash
exit 99
SH
cat >"$app/libexec/phoneboost-web-bridge" <<'SH'
#!/usr/bin/env bash
exit 99
SH
cat >"$app/scripts/run_phoneboost_control_center.sh" <<'SH'
#!/usr/bin/env bash
exit 99
SH
cat >"$prefix/bin/phoneboost-control-center" <<'SH'
#!/usr/bin/env bash
touch "$PHONEBOOST_TEST_ROOT/launcher-alive"
trap 'rm -f -- "$PHONEBOOST_TEST_ROOT/launcher-alive"; exit 0' TERM INT EXIT
printf '%s\n' 'http://127.0.0.1:41991/#token=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
while :; do sleep 1; done
SH
printf '<!doctype html>\n' >"$app/frontend/build/index.html"
cat >"$prefix/share/applications/org.phoneboost.ControlCenter.desktop" <<EOF
[Desktop Entry]
Exec=$prefix/bin/phoneboost-control-center
Terminal=true
EOF
chmod +x "$prefix/bin/phoneboost-control-center" "$app/libexec/phoneboostctl" \
    "$app/libexec/phoneboostd" "$app/libexec/phoneboost-web-bridge" \
    "$app/scripts/run_phoneboost_control_center.sh"

cat >"$fake_bin/curl" <<'SH'
#!/usr/bin/env bash
args=" $* "
if [[ ! -f "$PHONEBOOST_TEST_ROOT/launcher-alive" ]]; then
    if [[ "${PHONEBOOST_TEST_CURL_MODE:-}" == "bridge-stuck" \
        && "$args" == *"/bridge/v1/snapshot "* ]]; then
        printf '{}\n'
        exit 0
    fi
    exit 7
fi
now_ms="$(python3 -c 'import time; print(time.time_ns() // 1_000_000)')"
if [[ "${PHONEBOOST_TEST_CURL_MODE:-}" == "stale" ]]; then
    now_ms=1
fi
execution_source=REMOTE_SUCCESS
auto_use_reason=READY
if [[ "${PHONEBOOST_TEST_CURL_MODE:-}" == "fallback" ]]; then
    execution_source=LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE
    auto_use_reason=RECONNECTING
fi
discovery_state=FRESH_HINT
discovery_reason=C04_CANDIDATE_OBSERVED
lease_state=ACTIVE
lease_reason=C07_ACK_FRESH
admission_state=FRESH_PASS
admission_reason=C08_C09_C10_PROBE_PASSED
if [[ "${PHONEBOOST_TEST_CURL_MODE:-}" == "passive-stale" ]]; then
    discovery_state=STALE
    discovery_reason=OBSERVATION_EXPIRED
    admission_state=STALE
    admission_reason=PROOF_EXPIRED
elif [[ "${PHONEBOOST_TEST_CURL_MODE:-}" == "invalid-pair" ]]; then
    discovery_state=FRESH_HINT
    discovery_reason=OBSERVATION_EXPIRED
fi
if [[ "$args" == *" --data-binary "* ]]; then
    printf '{"provenance":"LIVE","observed_at_unix_ms":%s,"fixture":"c10-abc-v1","input_bytes":3,"digest_blake3_hex":"6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85","execution_source":"%s","auto_use_reason":"%s"}\n' \
        "$now_ms" "$execution_source" "$auto_use_reason"
elif [[ "$args" == *" /bridge/v1/snapshot "* || "$args" == *"/bridge/v1/snapshot "* ]]; then
    printf '{"provenance":"LIVE","observed_at_unix_ms":%s,"max_age_ms":3000,"local_daemon":{"state":"REACHABLE","runtime_state":"READY","local_api_state":"ACTIVE"},"authenticated_session":{"state":"AUTHENTICATED","remote_worker_state":"AUTHENTICATED"},"discovery_observation":{"state":"%s","reason":"%s"},"controller_lease":{"state":"%s","reason":"%s"},"resource_guard_admission_proof":{"state":"%s","reason":"%s"},"provider_readiness":{"provider":"pb.native.blake3/1","state":"AVAILABLE"},"auto_use":{"state":"AVAILABLE","reason":"READY"},"remote_blake3_available":true,"last_execution":{"observed_at_unix_ms":%s,"fixture":"c10-abc-v1","digest_blake3_hex":"6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85","execution_source":"%s","auto_use_reason":"%s"}}\n' \
        "$now_ms" "$discovery_state" "$discovery_reason" "$lease_state" "$lease_reason" \
        "$admission_state" "$admission_reason" "$now_ms" "$execution_source" "$auto_use_reason"
else
    exit 22
fi
SH
chmod +x "$fake_bin/curl"

output="$test_root/output.txt"
if ! printf 'P3_LIVE\nREMOTE_SUCCESS\n' | \
    HOME="$fake_home" PHONEBOOST_INSTALL_PREFIX="$prefix" \
    PHONEBOOST_TEST_ROOT="$test_root" PATH="$fake_bin:$PATH" \
    "$proof" >"$output" 2>&1; then
    cat "$output" >&2
    fail "happy-path simulation failed"
fi
grep -Fqx 'P3_INSTALLED_CONTROL_CENTER_PHYSICAL_PROOF PASS' "$output" \
    || fail "PASS marker is absent"
[[ ! -e "$test_root/launcher-alive" ]] \
    || fail "simulated installed launcher was not reaped"

passive_stale_output="$test_root/passive-stale-output.txt"
if ! printf 'P3_LIVE\nREMOTE_SUCCESS\n' | \
    HOME="$fake_home" PHONEBOOST_INSTALL_PREFIX="$prefix" \
    PHONEBOOST_TEST_ROOT="$test_root" PHONEBOOST_TEST_CURL_MODE=passive-stale \
    PATH="$fake_bin:$PATH" "$proof" >"$passive_stale_output" 2>&1; then
    cat "$passive_stale_output" >&2
    fail "truthful passive-expiry simulation failed"
fi
grep -Fqx 'Discovery observation: STALE / OBSERVATION_EXPIRED' "$passive_stale_output" \
    || fail "expired discovery observation was not rendered truthfully"
grep -Fqx 'Admission/readiness proof: STALE / PROOF_EXPIRED' "$passive_stale_output" \
    || fail "expired admission proof was not rendered truthfully"
grep -Fqx 'P3_INSTALLED_CONTROL_CENTER_PHYSICAL_PROOF PASS' "$passive_stale_output" \
    || fail "truthful passive expiry prevented a real remote-success proof"
[[ ! -e "$test_root/launcher-alive" ]] \
    || fail "launcher was not reaped after passive-expiry simulation"

rejection="$test_root/rejection.txt"
if printf 'WRONG\n' | HOME="$fake_home" PHONEBOOST_INSTALL_PREFIX="$prefix" \
    PHONEBOOST_TEST_ROOT="$test_root" PATH="$fake_bin:$PATH" \
    "$proof" >"$rejection" 2>&1; then
    fail "wrong operator confirmation was accepted"
fi
grep -Fq 'operator did not confirm the installed LIVE rendering' "$rejection" \
    || fail "wrong confirmation did not fail closed"
[[ ! -e "$test_root/launcher-alive" ]] \
    || fail "launcher was not reaped after a failed confirmation"

second_rejection="$test_root/second-rejection.txt"
if printf 'P3_LIVE\nWRONG\n' | HOME="$fake_home" PHONEBOOST_INSTALL_PREFIX="$prefix" \
    PHONEBOOST_TEST_ROOT="$test_root" PATH="$fake_bin:$PATH" \
    "$proof" >"$second_rejection" 2>&1; then
    fail "wrong remote-result confirmation was accepted"
fi
grep -Fq 'operator did not confirm the installed remote result' "$second_rejection" \
    || fail "wrong remote-result confirmation did not fail closed"
[[ ! -e "$test_root/launcher-alive" ]] \
    || fail "launcher was not reaped after the second failed confirmation"

first_timeout_output="$test_root/first-timeout-output.txt"
if (sleep 2) | HOME="$fake_home" PHONEBOOST_INSTALL_PREFIX="$prefix" \
    PHONEBOOST_TEST_ROOT="$test_root" \
    PHONEBOOST_OPERATOR_CONFIRMATION_TIMEOUT_SECONDS=1 \
    PATH="$fake_bin:$PATH" "$proof" >"$first_timeout_output" 2>&1; then
    fail "an absent first operator confirmation was allowed to wait indefinitely"
fi
grep -Fq 'operator confirmation timed out or input closed' "$first_timeout_output" \
    || fail "first operator confirmation timeout did not fail closed"
[[ ! -e "$test_root/launcher-alive" ]] \
    || fail "launcher was not reaped after the first confirmation timeout"

second_timeout_output="$test_root/second-timeout-output.txt"
if { printf 'P3_LIVE\n'; sleep 2; } | \
    HOME="$fake_home" PHONEBOOST_INSTALL_PREFIX="$prefix" \
    PHONEBOOST_TEST_ROOT="$test_root" \
    PHONEBOOST_OPERATOR_CONFIRMATION_TIMEOUT_SECONDS=1 \
    PATH="$fake_bin:$PATH" "$proof" >"$second_timeout_output" 2>&1; then
    fail "an absent second operator confirmation was allowed to wait indefinitely"
fi
grep -Fq 'operator confirmation timed out or input closed' "$second_timeout_output" \
    || fail "second operator confirmation timeout did not fail closed"
[[ ! -e "$test_root/launcher-alive" ]] \
    || fail "launcher was not reaped after the second confirmation timeout"

invalid_timeout_output="$test_root/invalid-timeout-output.txt"
if printf '' | HOME="$fake_home" PHONEBOOST_INSTALL_PREFIX="$prefix" \
    PHONEBOOST_TEST_ROOT="$test_root" \
    PHONEBOOST_OPERATOR_CONFIRMATION_TIMEOUT_SECONDS=0 \
    PATH="$fake_bin:$PATH" "$proof" >"$invalid_timeout_output" 2>&1; then
    fail "an unsafe operator confirmation timeout was accepted"
fi
grep -Fq 'operator confirmation timeout must be an integer from 1 to 3600 seconds' \
    "$invalid_timeout_output" \
    || fail "unsafe operator confirmation timeout did not fail closed"
[[ ! -e "$test_root/launcher-alive" ]] \
    || fail "launcher started despite an unsafe confirmation timeout"

status_rejection="$test_root/status-rejection.txt"
if printf '' | HOME="$fake_home" PHONEBOOST_INSTALL_PREFIX="$prefix" \
    PHONEBOOST_TEST_ROOT="$test_root" PHONEBOOST_TEST_STATUS_MODE=extra \
    PATH="$fake_bin:$PATH" "$proof" >"$status_rejection" 2>&1; then
    fail "contradictory installed status was accepted"
fi
grep -Fq 'production daemon is not ready for the installed proof' "$status_rejection" \
    || fail "contradictory installed status did not fail closed"
[[ ! -e "$test_root/launcher-alive" ]] \
    || fail "launcher started despite contradictory installed status"

stale_rejection="$test_root/stale-rejection.txt"
if printf '' | HOME="$fake_home" PHONEBOOST_INSTALL_PREFIX="$prefix" \
    PHONEBOOST_TEST_ROOT="$test_root" PHONEBOOST_TEST_CURL_MODE=stale \
    PATH="$fake_bin:$PATH" "$proof" >"$stale_rejection" 2>&1; then
    fail "stale installed snapshot was accepted"
fi
grep -Fq 'installed browser snapshot is not fresh, ready, and truthfully observable' \
    "$stale_rejection" \
    || fail "stale installed snapshot did not fail closed"
[[ ! -e "$test_root/launcher-alive" ]] \
    || fail "launcher was not reaped after a stale snapshot"

pair_rejection="$test_root/pair-rejection.txt"
if printf '' | HOME="$fake_home" PHONEBOOST_INSTALL_PREFIX="$prefix" \
    PHONEBOOST_TEST_ROOT="$test_root" PHONEBOOST_TEST_CURL_MODE=invalid-pair \
    PATH="$fake_bin:$PATH" "$proof" >"$pair_rejection" 2>&1; then
    fail "invalid passive gate state/reason pair was accepted"
fi
grep -Fq 'installed browser snapshot is not fresh, ready, and truthfully observable' \
    "$pair_rejection" || fail "invalid passive gate pair did not fail closed"
[[ ! -e "$test_root/launcher-alive" ]] \
    || fail "launcher was not reaped after an invalid passive gate pair"

fallback_rejection="$test_root/fallback-rejection.txt"
if printf 'P3_LIVE\n' | HOME="$fake_home" PHONEBOOST_INSTALL_PREFIX="$prefix" \
    PHONEBOOST_TEST_ROOT="$test_root" PHONEBOOST_TEST_CURL_MODE=fallback \
    PATH="$fake_bin:$PATH" "$proof" >"$fallback_rejection" 2>&1; then
    fail "local fallback was accepted as installed remote success"
fi
grep -Fq 'installed bridge did not prove the canonical remote execution' "$fallback_rejection" \
    || fail "local fallback did not fail closed"
[[ ! -e "$test_root/launcher-alive" ]] \
    || fail "launcher was not reaped after a fallback result"

stuck_bridge_rejection="$test_root/stuck-bridge-rejection.txt"
if printf 'P3_LIVE\nREMOTE_SUCCESS\n' | \
    HOME="$fake_home" PHONEBOOST_INSTALL_PREFIX="$prefix" \
    PHONEBOOST_TEST_ROOT="$test_root" PHONEBOOST_TEST_CURL_MODE=bridge-stuck \
    PATH="$fake_bin:$PATH" "$proof" >"$stuck_bridge_rejection" 2>&1; then
    fail "a bridge still reachable after launcher shutdown was accepted"
fi
grep -Fq 'installed bridge remained reachable after launcher shutdown' \
    "$stuck_bridge_rejection" \
    || fail "stuck bridge shutdown did not fail closed"
if grep -Fq 'P3_INSTALLED_CONTROL_CENTER_PHYSICAL_PROOF PASS' \
    "$stuck_bridge_rejection"; then
    fail "PASS was emitted before bridge shutdown was verified"
fi
[[ ! -e "$test_root/launcher-alive" ]] \
    || fail "launcher was not reaped after a stuck-bridge rejection"

printf '%s\n' 'P3_INSTALLED_CONTROL_CENTER_PROOF_TEST PASS'
