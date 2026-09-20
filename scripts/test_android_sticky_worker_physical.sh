#!/usr/bin/env bash
set -euo pipefail

# Simulates an abrupt app-process loss without force-stopping the package.
# Force-stop is a user/system stop action and must not be auto-recovered.
workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
shared_tooling="${PHONEBOOST_SHARED_TOOLING:-${workspace_root}/.tooling}"
adb="${ADB:-${shared_tooling}/android-sdk/platform-tools/adb}"
apk="${PHONEBOOST_APK:-${workspace_root}/android/app/build/outputs/apk/debug/app-debug.apk}"
package="org.phoneboost.app"
activity="${package}/.MainActivity"

fail() {
    printf 'ANDROID_STICKY_WORKER FAIL: %s\n' "$1" >&2
    exit 1
}

test -x "${adb}" || fail 'adb is unavailable'
test -f "${apk}" || fail 'debug APK is unavailable'
test "$("${adb}" -d get-state)" = device || fail 'exactly one authorized Android device is required'
test -z "$("${adb}" forward --list)" || fail 'ADB forward tunnel is active'
test -z "$("${adb}" reverse --list)" || fail 'ADB reverse tunnel is active'

"${adb}" -d install -r -g "${apk}" >/dev/null

# Start the user-enabled worker, then remove the no-history Activity so only
# the started foreground service owns the runtime.
"${adb}" -d shell am start -W -f 0x10008000 --activity-no-history -n "${activity}" >/dev/null
"${adb}" -d shell am start -W -a android.intent.action.MAIN \
    -c android.intent.category.HOME >/dev/null
sleep 2
if "${adb}" -d shell dumpsys activity activities \
    | grep -qE 'ActivityRecord\{[^}]+org\.phoneboost\.app/\.MainActivity'; then
    fail 'PhoneBoost Activity remained present'
fi

before_dump="$("${adb}" -d shell dumpsys activity services "${package}")"
grep -q 'isForeground=true' <<<"${before_dump}" || fail 'foreground service is not active'
grep -q 'startCommandResult=1' <<<"${before_dump}" || fail 'service is not START_STICKY'
"${adb}" -d shell run-as "${package}" test -f files/phoneboost/identity.key \
    || fail 'persisted Android identity is absent'
before_pid="$("${adb}" -d shell pidof "${package}" | tr -d '\r')"
test -n "${before_pid}" || fail 'app process is absent'

"${adb}" -d logcat -c
"${adb}" -d shell run-as "${package}" kill -9 "${before_pid}" >/dev/null 2>&1 || true

after_pid=""
after_dump=""
for _ in $(seq 1 60); do
    after_pid="$("${adb}" -d shell pidof "${package}" 2>/dev/null | tr -d '\r' || true)"
    after_dump="$("${adb}" -d shell dumpsys activity services "${package}" 2>/dev/null || true)"
    if [[ -n "${after_pid}" && "${after_pid}" != "${before_pid}" ]] \
        && grep -q 'isForeground=true' <<<"${after_dump}" \
        && grep -q 'startCommandResult=1' <<<"${after_dump}"; then
        break
    fi
    sleep 0.5
done

test -n "${after_pid}" || fail 'app process was not recreated'
test "${after_pid}" != "${before_pid}" || fail 'process identity did not change'
grep -q 'isForeground=true' <<<"${after_dump}" || fail 'FGS was not restored'
grep -q 'types=0x00000010' <<<"${after_dump}" || fail 'connectedDevice FGS type was not restored'
grep -q 'startCommandResult=1' <<<"${after_dump}" || fail 'sticky restart result was not retained'
if "${adb}" -d shell dumpsys activity activities \
    | grep -qE 'ActivityRecord\{[^}]+org\.phoneboost\.app/\.MainActivity'; then
    fail 'Activity was recreated with the worker'
fi
"${adb}" -d shell run-as "${package}" test -f files/phoneboost/identity.key \
    || fail 'persisted Android identity was lost'

logs="$("${adb}" -d logcat -d -s PhoneBoostA6:I PhoneBoostC04:I '*:S' 2>/dev/null)"
grep -q 'FGS_STARTED state=PAIRING_REQUIRED' <<<"${logs}" || fail 'worker restart log is absent'
grep -q 'C04_LISTENER state=LISTENING' <<<"${logs}" || fail 'listener restart log is absent'

printf '%s\n' \
    'ANDROID_STICKY_WORKER PASS' \
    'PROCESS_CHANGED=YES' \
    'ACTIVITY_AFTER_RESTART=ABSENT' \
    'FGS_RESTARTED=YES' \
    'FGS_TYPE=CONNECTED_DEVICE' \
    'PAIRING_IDENTITY=PERSISTED' \
    'LISTENER_RESTARTED=YES' \
    'ADB_TUNNEL=NONE'
