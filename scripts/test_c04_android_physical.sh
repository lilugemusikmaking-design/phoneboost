#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
adb="/home/mimir/phoneboost_ftest02_android_v1_work/.tooling/android-sdk/platform-tools/adb"
apk="${workspace_root}/android/app/build/outputs/apk/debug/app-debug.apk"
package="org.phoneboost.app"
activity="${package}/.MainActivity"
service="${package}/.PhoneBoostService"
probe="${workspace_root}/target/debug/c04-lan-probe"
daemon="${workspace_root}/target/release/phoneboostd"
ctl="${workspace_root}/target/debug/phoneboostctl"
daemon_runtime="$(mktemp -d /tmp/phoneboost-c04-physical.XXXXXX)"
daemon_pid=""

cleanup() {
    if [[ -n "${daemon_pid}" ]]; then
        kill "${daemon_pid}" 2>/dev/null || true
        wait "${daemon_pid}" 2>/dev/null || true
    fi
    if [[ -d "${daemon_runtime}" ]]; then
        rm -r -- "${daemon_runtime}"
    fi
}
trap cleanup EXIT INT TERM
chmod 700 "${daemon_runtime}"

test -f "${apk}"
test -x "${probe}"
test -x "${daemon}"
test -x "${ctl}"
test "$("${adb}" -d get-state)" = "device"
test -z "$("${adb}" forward --list)"
test -z "$("${adb}" reverse --list)"

"${adb}" -d install -r -g "${apk}" >/dev/null
"${adb}" -d shell am force-stop "${package}"
"${adb}" -d logcat -c
"${adb}" -d shell am start -W -n "${activity}" >/dev/null
sleep 3

c04_logs() {
    "${adb}" -d logcat -d -s PhoneBoostC04:I '*:S' 2>/dev/null
}

listener_line="$(c04_logs | grep 'C04_LISTENER state=LISTENING' | tail -1)"
endpoint="$(sed -n 's/.* ip=\([^ ]*\) port=\([0-9][0-9]*\) .*/\1:\2/p' <<<"${listener_line}")"
test -n "${endpoint}"
port="${endpoint##*:}"
device_ip="${endpoint%:*}"
linux_ip="$(ip -4 -brief address show wlp3s0 | awk '{print $3}' | cut -d/ -f1)"
test "${device_ip%.*}" = "${linux_ip%.*}"

probe_output="$("${probe}" "${endpoint}")"
grep -q 'connect=PASS' <<<"${probe_output}"
grep -q 'bidirectional=PASS' <<<"${probe_output}"
grep -q 'loss=PASS' <<<"${probe_output}"
grep -q 'reconnect=PASS' <<<"${probe_output}"
grep -q 'max_state=CONNECTED_UNAUTHENTICATED' <<<"${probe_output}"
grep -Eq 'rtt_ms=[0-9]+' <<<"${probe_output}"
grep -Eq 'tx_Bps=[1-9][0-9]*' <<<"${probe_output}"
grep -Eq 'rx_Bps=[1-9][0-9]*' <<<"${probe_output}"
grep -q 'reconnect_count=1' <<<"${probe_output}"

logs_after_probe="$(c04_logs)"
test "$(grep -c 'state=CONNECTED_UNAUTHENTICATED' <<<"${logs_after_probe}")" -ge 2
if grep -qE 'D2|D3|D4|Noise|PBMUX|params|raw request|raw response' <<<"${logs_after_probe}"; then
    echo "Forbidden material in C04 logs" >&2
    exit 1
fi

XDG_RUNTIME_DIR="${daemon_runtime}" XDG_STATE_HOME="${daemon_runtime}/state" \
    "${daemon}" --foreground \
    --manual-endpoint "${endpoint}" \
    >"${daemon_runtime}/daemon.stdout" 2>"${daemon_runtime}/daemon.stderr" &
daemon_pid=$!
for _ in $(seq 1 100); do
    if grep -q 'C04_TRANSPORT state=CONNECTED_UNAUTHENTICATED' \
        "${daemon_runtime}/daemon.stdout" 2>/dev/null; then
        break
    fi
    kill -0 "${daemon_pid}" 2>/dev/null
    sleep 0.1
done
grep -q '^READY$' "${daemon_runtime}/daemon.stdout"
grep -q 'C04_TRANSPORT state=CONNECTED_UNAUTHENTICATED.*trust=NONE' \
    "${daemon_runtime}/daemon.stdout"
expected_status=$'PhoneBoost: READY\nLocal API: ACTIVE\nAndroid worker: NOT_CONFIGURED'
test "$(XDG_RUNTIME_DIR="${daemon_runtime}" "${ctl}" status)" = "${expected_status}"

"${adb}" -d shell run-as "${package}" am stopservice --user 0 -n "${service}" >/dev/null 2>&1 || true
sleep 1
if "${adb}" -d shell ss -ltn 2>/dev/null | grep -q ":${port}"; then
    echo "C04 listener survived FGS stop" >&2
    exit 1
fi
grep -q 'C04_LISTENER state=UNAVAILABLE reason=FGS_STOP' <<<"$(c04_logs)"
for _ in $(seq 1 25); do
    if grep -q 'C04_TRANSPORT state=LOST' "${daemon_runtime}/daemon.stdout"; then
        break
    fi
    sleep 0.2
done
grep -q 'C04_TRANSPORT state=LOST.*trust=NONE' "${daemon_runtime}/daemon.stdout"
kill "${daemon_pid}"
wait "${daemon_pid}" 2>/dev/null || true
daemon_pid=""

"${adb}" -d shell am start -W --activity-clear-top -n "${activity}" >/dev/null
sleep 3
new_listener="$(c04_logs | grep 'C04_LISTENER state=LISTENING' | tail -1)"
new_endpoint="$(sed -n 's/.* ip=\([^ ]*\) port=\([0-9][0-9]*\) .*/\1:\2/p' <<<"${new_listener}")"
test -n "${new_endpoint}"
restarted_probe="$("${probe}" "${new_endpoint}")"
grep -q 'bidirectional=PASS' <<<"${restarted_probe}"

printf '%s\n' \
    "C04 physical LAN PASS" \
    "Android API: $("${adb}" -d shell getprop ro.build.version.sdk | tr -d '\r')" \
    "Transport: LOCAL_IP" \
    "Endpoint: REDACTED:${new_endpoint##*:}" \
    "ADB tunnel: NONE" \
    "Initial probe: ${probe_output}" \
    "Production phoneboostd direct connect: PASS" \
    "Local API remained ACTIVE: PASS" \
    "Production loss detection: PASS" \
    "FGS listener stop: PASS" \
    "FGS listener restart: PASS" \
    "Restarted probe: ${restarted_probe}" \
    "Trust: NONE" \
    "C05: ABSENT" \
    "PBMUX dispatch: ABSENT"
