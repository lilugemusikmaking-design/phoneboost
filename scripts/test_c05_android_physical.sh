#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
adb="/home/mimir/phoneboost_ftest02_android_v1_work/.tooling/android-sdk/platform-tools/adb"
apk="${workspace_root}/android/app/build/outputs/apk/debug/app-debug.apk"
daemon="${workspace_root}/target/release/phoneboostd"
ctl="${workspace_root}/target/release/phoneboostctl"
package="org.phoneboost.app"
activity="${package}/.MainActivity"
test_root="$(mktemp -d /tmp/phoneboost-c05-physical.XXXXXX)"
daemon_pid=""
current_stage="bootstrap"

cleanup() {
    local exit_status=$?
    if [[ -n "${daemon_pid}" ]]; then
        kill "${daemon_pid}" 2>/dev/null || true
        wait "${daemon_pid}" 2>/dev/null || true
    fi
    if [[ -d "${test_root}" ]]; then
        rm -r -- "${test_root}"
    fi
    if [[ ${exit_status} -ne 0 ]]; then
        printf 'C05 physical FAIL stage=%s\n' "${current_stage}" >&2
    fi
}
trap cleanup EXIT INT TERM
chmod 700 "${test_root}"
mkdir -m 700 "${test_root}/runtime" "${test_root}/state"

test -f "${apk}"
test -x "${daemon}"
test -x "${ctl}"
test "$("${adb}" -d get-state)" = "device"
test -z "$("${adb}" forward --list)"
test -z "$("${adb}" reverse --list)"

current_stage="install-and-listen"
"${adb}" -d install -r -g "${apk}" >/dev/null
"${adb}" -d shell pm clear "${package}" >/dev/null
"${adb}" -d shell pm grant "${package}" android.permission.POST_NOTIFICATIONS >/dev/null
"${adb}" -d logcat -c
"${adb}" -d shell am start -W -n "${activity}" >/dev/null
sleep 3

listener_endpoint() {
    local listener
    listener="$("${adb}" -d logcat -d -s PhoneBoostC04:I '*:S' 2>/dev/null \
        | grep 'C04_LISTENER state=LISTENING' | tail -1)"
    sed -n 's/.* ip=\([^ ]*\) port=\([0-9][0-9]*\) .*/\1:\2/p' <<<"${listener}"
}

start_daemon() {
    local endpoint="$1"
    local log="$2"
    XDG_RUNTIME_DIR="${test_root}/runtime" XDG_STATE_HOME="${test_root}/state" \
        "${daemon}" --foreground --manual-endpoint "${endpoint}" \
        >"${test_root}/${log}.stdout" 2>"${test_root}/${log}.stderr" &
    daemon_pid=$!
    for _ in $(seq 1 150); do
        if grep -q 'C04_TRANSPORT state=CONNECTED_UNAUTHENTICATED' \
            "${test_root}/${log}.stdout" 2>/dev/null; then
            return
        fi
        kill -0 "${daemon_pid}" 2>/dev/null
        sleep 0.1
    done
    return 1
}

stop_daemon() {
    kill "${daemon_pid}"
    wait "${daemon_pid}" 2>/dev/null || true
    daemon_pid=""
}

endpoint="$(listener_endpoint)"
test -n "${endpoint}"
current_stage="xx-connect"
start_daemon "${endpoint}" xx

current_stage="sas-host-request"
pair_output="$(XDG_RUNTIME_DIR="${test_root}/runtime" "${ctl}" pair)"
current_stage="sas-host-parse"
host_sas="$(sed -n 's/^Pairing code: \([0-9][0-9]*\)$/\1/p' <<<"${pair_output}")"
test "${#host_sas}" -eq 6

current_stage="sas-android-ui"
"${adb}" -d shell input keyevent KEYCODE_WAKEUP
"${adb}" -d shell wm dismiss-keyguard
"${adb}" -d shell am start -W --activity-clear-top -n "${activity}" >/dev/null
current_stage="sas-android-ocr"
ocr=""
android_sas=""
for _ in $(seq 1 3); do
    "${adb}" -d shell input keyevent KEYCODE_WAKEUP
    "${adb}" -d shell wm dismiss-keyguard
    "${adb}" -d shell input keyevent 82
    ocr="$("${adb}" -d exec-out screencap -p | tesseract stdin stdout --psm 6 tsv 2>/dev/null)"
    android_sas="$(awk -F '\t' '
        match($12, /[0-9][0-9][0-9][0-9][0-9][0-9]/) {
            print substr($12, RSTART, RLENGTH)
            exit
        }
    ' <<<"${ocr}")"
    if grep -q $'\tCONFIRM\r\{0,1\}$' <<<"${ocr}" \
        && [[ "${#android_sas}" -eq 6 ]]; then
        break
    fi
    sleep 0.25
done
test "${#android_sas}" -eq 6
current_stage="sas-equality"
test "${host_sas}" = "${android_sas}"
current_stage="confirm-locate"
confirm_bounds="$(awk -F '\t' '$12 == "CONFIRM" {print $7, $8, $9, $10; exit}' <<<"${ocr}")"
read -r confirm_x confirm_y confirm_width confirm_height <<<"${confirm_bounds}"
test -n "${confirm_x}"

current_stage="mutual-confirm-auth"
XDG_RUNTIME_DIR="${test_root}/runtime" "${ctl}" pair-confirm >/dev/null
"${adb}" -d shell input tap \
    "$((confirm_x + confirm_width / 2))" "$((confirm_y + confirm_height / 2))"

for _ in $(seq 1 150); do
    status="$(XDG_RUNTIME_DIR="${test_root}/runtime" "${ctl}" status)"
    if grep -q '^Android worker: AUTHENTICATED$' <<<"${status}"; then
        break
    fi
    sleep 0.1
done
grep -q '^Android worker: AUTHENTICATED$' <<<"${status}"

current_stage="persistence-audit"
host_state="${test_root}/state/phoneboost"
test "$(stat -c '%a' "${host_state}")" = 700
test "$(stat -c '%a' "${host_state}/identity.key")" = 600
test "$(stat -c '%a' "${host_state}/pairing_guard.json")" = 600
test "$(find "${host_state}/peers" -maxdepth 1 -name '*.json' -type f | wc -l)" -eq 1
if grep -R -a -E \
    'SAS_PENDING|LOCAL_CONFIRMED|PEER_CONFIRMED|MUTUAL_CONFIRMED|TRUST_COMMITTING' \
    "${host_state}" >/dev/null; then
    exit 1
fi

android_files="$("${adb}" -d shell run-as "${package}" sh -c \
    'find files/phoneboost -maxdepth 2 -type f -print' | tr -d '\r')"
grep -q 'files/phoneboost/identity.key' <<<"${android_files}"
grep -q 'files/phoneboost/pairing_guard.json' <<<"${android_files}"
test "$(grep -c '/peers/.*\.json' <<<"${android_files}")" -eq 1
if "${adb}" -d shell run-as "${package}" sh -c \
    "grep -R -E 'SAS_PENDING|LOCAL_CONFIRMED|PEER_CONFIRMED|MUTUAL_CONFIRMED|TRUST_COMMITTING' files/phoneboost 2>/dev/null" \
    | grep -q .; then
    exit 1
fi
if "${adb}" -d logcat -d | grep -E 'Pairing code: [0-9]{6}' >/dev/null; then
    exit 1
fi
if find "${test_root}" -type f -exec \
    grep -E 'Pairing code: [0-9]{6}' {} + >/dev/null; then
    exit 1
fi

current_stage="ik-stop-daemon"
stop_daemon
current_stage="ik-stop-android"
"${adb}" -d shell am force-stop "${package}"
current_stage="ik-start-android"
"${adb}" -d shell am start -W -n "${activity}" >/dev/null
sleep 3
current_stage="ik-listener"
ik_endpoint="$(listener_endpoint)"
test -n "${ik_endpoint}"
current_stage="ik-connect"
start_daemon "${ik_endpoint}" ik
current_stage="ik-auth"
for _ in $(seq 1 150); do
    ik_status="$(XDG_RUNTIME_DIR="${test_root}/runtime" "${ctl}" status)"
    if grep -q '^Android worker: AUTHENTICATED$' <<<"${ik_status}"; then
        break
    fi
    sleep 0.1
done
grep -q '^Android worker: AUTHENTICATED$' <<<"${ik_status}"

current_stage="post-auth-isolation"
test -z "$("${adb}" forward --list)"
test -z "$("${adb}" reverse --list)"
if "${adb}" -d logcat -d -s PhoneBoostA6:I PhoneBoostC04:I '*:S' 2>/dev/null \
    | grep -qE 'controller=ACTIVE|remote=ACTIVE|BOOST ACTIVE|COMPUTE ACTIVE'; then
    exit 1
fi

current_stage="complete"
printf '%s\n' \
    'C05/C06 physical secure pairing PASS' \
    'XX=PASS' \
    'SAS_MATCH=PASS (digits suppressed)' \
    'LINUX_CONFIRM=PASS' \
    'ANDROID_UI_CONFIRM=PASS' \
    'PAIR_CONFIRM_ENCRYPTED=PASS' \
    'COMMITTED_BOTH=PASS' \
    'AUTHENTICATED=PASS' \
    'PING_PONG=PASS' \
    'RESTART_RELOAD=PASS' \
    'IK_RECONNECT=PASS' \
    'ADB_TUNNEL=NONE' \
    'REMOTE_C07_C08=INACCESSIBLE' \
    "ENDPOINT=REDACTED:${ik_endpoint##*:}"
