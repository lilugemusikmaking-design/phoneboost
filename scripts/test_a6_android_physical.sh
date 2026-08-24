#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
shared_tooling="${PHONEBOOST_SHARED_TOOLING:-${workspace_root}/.tooling}"
adb="${shared_tooling}/android-sdk/platform-tools/adb"
apk="${workspace_root}/android/app/build/outputs/apk/debug/app-debug.apk"
package="org.phoneboost.app"
activity="${package}/.MainActivity"

test -f "${apk}"
test "$("${adb}" -d get-state)" = "device"
test "$("${adb}" -d shell getprop ro.product.cpu.abi | tr -d '\r')" = "arm64-v8a"

"${adb}" -d install -r -g "${apk}" >/dev/null
"${adb}" -d shell am force-stop "${package}"
"${adb}" -d logcat -c
"${adb}" -d shell am start -W -n "${activity}" >/dev/null
sleep 13

logs="$("${adb}" -d logcat -d -s PhoneBoostA6:I '*:S' 2>/dev/null)"
samples="$(sed -n 's/.*HEALTH_SAMPLE count=\([0-9][0-9]*\) at_ms=\([0-9][0-9]*\).*/\1 \2/p' <<<"${logs}")"
sample_count="$(wc -l <<<"${samples}" | tr -d ' ')"
test "${sample_count}" -ge 3

first_count="$(awk 'NR == 1 { print $1 }' <<<"${samples}")"
last_count="$(awk 'END { print $1 }' <<<"${samples}")"
test "${last_count}" -gt "${first_count}"
awk '
    NR > 1 {
        interval = $2 - previous
        if (interval < 1500 || interval > 3000) exit 1
    }
    { previous = $2 }
' <<<"${samples}"

grep -q 'result=0' <<<"${logs}"
grep -q 'resource_guard=ACTIVE' <<<"${logs}"
grep -q 'lease=NONE' <<<"${logs}"
grep -q 'remote=INACTIVE_FOR_REMOTE_CONTROL' <<<"${logs}"
grep -qE 'transport=(LISTENING|CONNECTED_UNAUTHENTICATED)' <<<"${logs}"

service_dump="$("${adb}" -d shell dumpsys activity services "${package}")"
grep -q 'isForeground=true' <<<"${service_dump}"
grep -q 'types=0x00000010' <<<"${service_dump}"

package_dump="$("${adb}" -d shell dumpsys package "${package}")"
grep -q 'primaryCpuAbi=arm64-v8a' <<<"${package_dump}"
grep -q 'targetSdk=37' <<<"${package_dump}"
latest_health="$(grep 'HEALTH_SAMPLE' <<<"${logs}" | tail -1 | sed 's/^.*HEALTH_SAMPLE/HEALTH_SAMPLE/')"
if grep -qE 'D2|D3|D4|params|raw request|raw response' <<<"${logs}"; then
    echo "Forbidden material in PhoneBoost log" >&2
    exit 1
fi

printf '%s\n' \
    "A6 physical health PASS" \
    "API: $("${adb}" -d shell getprop ro.build.version.sdk | tr -d '\r')" \
    "ABI: arm64-v8a" \
    "FGS connectedDevice: PASS" \
    "Sampler cadence: PASS (${sample_count} samples)" \
    "ResourceGuard local ingestion: PASS" \
    "Controller lease: NONE" \
    "Remote control: INACTIVE_FOR_REMOTE_CONTROL" \
    "Transport: C04 LISTENER INTEGRATED" \
    "${latest_health}"
