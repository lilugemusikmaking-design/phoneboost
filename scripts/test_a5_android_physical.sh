#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
adb="/home/mimir/phoneboost_ftest02_android_v1_work/.tooling/android-sdk/platform-tools/adb"
apk="${workspace_root}/android/app/build/outputs/apk/debug/app-debug.apk"
package="org.phoneboost.app"
activity="${package}/.MainActivity"
service="${package}/.PhoneBoostService"

test -f "${apk}"
test "$("${adb}" -d get-state)" = "device"
test "$("${adb}" -d shell getprop ro.product.cpu.abi | tr -d '\r')" = "arm64-v8a"

"${adb}" -d install -r -g "${apk}" >/dev/null
"${adb}" -d shell am force-stop "${package}"
"${adb}" -d shell am start -W -n "${activity}" >/dev/null
sleep 1

status_lines() {
    "${adb}" -d logcat -d -s PhoneBoostA5:I '*:S' 2>/dev/null
}

latest_incarnation() {
    status_lines | sed -n 's/.*UI_STATUS state=PAIRING_REQUIRED incarnation=\([0-9a-f][0-9a-f]*\) fgs=ACTIVE.*/\1/p' | tail -1
}

first="$(latest_incarnation)"
test "${#first}" -eq 8

"${adb}" -d shell am start -W --activity-clear-top -n "${activity}" >/dev/null
sleep 1
after_ui_recreation="$(latest_incarnation)"
test "${after_ui_recreation}" = "${first}"

"${adb}" -d shell run-as "${package}" am stopservice --user 0 -n "${service}" >/dev/null 2>&1 || true
sleep 1
if "${adb}" -d shell dumpsys activity services "${package}" | grep -q 'ServiceRecord'; then
    echo "PhoneBoost service survived graceful stop" >&2
    exit 1
fi
status_lines | tail -20 | grep -q 'WORKER_STOP'

"${adb}" -d shell am start -W --activity-clear-top -n "${activity}" >/dev/null
sleep 1
after_graceful_restart="$(latest_incarnation)"
test "${#after_graceful_restart}" -eq 8
test "${after_graceful_restart}" != "${first}"

"${adb}" -d shell am force-stop "${package}"
if "${adb}" -d shell pidof "${package}" >/dev/null 2>&1; then
    echo "PhoneBoost process survived force-stop" >&2
    exit 1
fi
"${adb}" -d shell am start -W -n "${activity}" >/dev/null
sleep 1
after_process_restart="$(latest_incarnation)"
test "${#after_process_restart}" -eq 8
test "${after_process_restart}" != "${after_graceful_restart}"

service_dump="$("${adb}" -d shell dumpsys activity services "${package}")"
grep -q 'isForeground=true' <<<"${service_dump}"
grep -q 'types=0x00000010' <<<"${service_dump}"

notification_dump="$("${adb}" -d shell dumpsys notification --noredact)"
grep -q 'pkg=org.phoneboost.app' <<<"${notification_dump}"
grep -q 'android.text=String (Worker core: PAIRING_REQUIRED)' <<<"${notification_dump}"

package_dump="$("${adb}" -d shell dumpsys package "${package}")"
grep -q 'primaryCpuAbi=arm64-v8a' <<<"${package_dump}"
grep -q 'targetSdk=37' <<<"${package_dump}"
if grep -qE 'android.permission.(INTERNET|ACCESS_LOCAL_NETWORK)' <<<"${package_dump}"; then
    echo "Unexpected network permission" >&2
    exit 1
fi

pid="$("${adb}" -d shell pidof "${package}" | tr -d '\r')"
if "${adb}" -d shell run-as "${package}" sh -c "ls -l /proc/${pid}/fd 2>/dev/null" | grep -q 'socket:'; then
    echo "Unexpected PhoneBoost socket descriptor" >&2
    exit 1
fi

redacted_log="$(status_lines | tail -40)"
grep -q 'state=PAIRING_REQUIRED' <<<"${redacted_log}"
if grep -qE 'D2|D3|D4|params|raw request|raw response' <<<"${redacted_log}"; then
    echo "Forbidden material in PhoneBoost log" >&2
    exit 1
fi

printf '%s\n' \
    "A5 physical device PASS" \
    "API: $("${adb}" -d shell getprop ro.build.version.sdk | tr -d '\r')" \
    "ABI: arm64-v8a" \
    "FGS connectedDevice: PASS" \
    "JNI worker: PASS" \
    "Activity recreation preserves incarnation: PASS" \
    "Graceful core restart changes incarnation: PASS" \
    "Full process restart changes incarnation: PASS" \
    "State: PAIRING_REQUIRED" \
    "Transport: NOT_CONFIGURED"
