#!/usr/bin/env bash
set -euo pipefail

# Physical development proof only. ADB observes and controls the test device;
# it is not part of the PhoneBoost product transport.
workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
shared_tooling="${PHONEBOOST_SHARED_TOOLING:-${workspace_root}/.tooling}"
adb="${ADB:-${shared_tooling}/android-sdk/platform-tools/adb}"
ctl="${PHONEBOOSTCTL_BIN:-${workspace_root}/target/release/phoneboostctl}"
package="org.phoneboost.app"
activity="${package}/.MainActivity"
wait_seconds="${PHONEBOOST_SCREEN_OFF_WAIT_SECONDS:-300}"
poll_seconds="${PHONEBOOST_SCREEN_OFF_POLL_SECONDS:-30}"

fail() {
    printf 'ANDROID_DURABLE_WORKER FAIL: %s\n' "$1" >&2
    exit 1
}

case "${wait_seconds}" in
    ''|*[!0-9]*) fail 'PHONEBOOST_SCREEN_OFF_WAIT_SECONDS must be an integer' ;;
esac
case "${poll_seconds}" in
    ''|*[!0-9]*) fail 'PHONEBOOST_SCREEN_OFF_POLL_SECONDS must be an integer' ;;
esac
(( wait_seconds >= 1 )) || fail 'wait duration must be positive'
(( poll_seconds >= 1 && poll_seconds <= 60 )) || fail 'poll duration must be between 1 and 60 seconds'

test -x "${adb}" || fail 'adb is unavailable'
test -x "${ctl}" || fail 'phoneboostctl is unavailable'
test "$("${adb}" -d get-state)" = device || fail 'exactly one authorized Android device is required'
test -z "$("${adb}" forward --list)" || fail 'ADB forward tunnel is active'
test -z "$("${adb}" reverse --list)" || fail 'ADB reverse tunnel is active'

status="$("${ctl}" status)" || fail 'phoneboostd is unavailable'
grep -Fqx 'Android worker: AUTHENTICATED' <<<"${status}" || fail 'worker is not authenticated'
grep -Fqx 'Auto-use: AVAILABLE' <<<"${status}" || fail 'auto-use is unavailable'
grep -Fqx 'Remote BLAKE3: AVAILABLE' <<<"${status}" || fail 'remote compute is unavailable'

# Recreate the Activity as no-history, then leave it. This starts an enabled
# worker when needed while ensuring the proof itself runs without an Activity.
"${adb}" -d shell am start -W -f 0x10008000 --activity-no-history -n "${activity}" >/dev/null
"${adb}" -d shell am start -W -a android.intent.action.MAIN \
    -c android.intent.category.HOME >/dev/null
sleep 2
if "${adb}" -d shell dumpsys activity activities \
    | grep -qE 'ActivityRecord\{[^}]+org\.phoneboost\.app/\.MainActivity'; then
    fail 'PhoneBoost Activity remained present'
fi

service_dump="$("${adb}" -d shell dumpsys activity services "${package}")"
grep -q 'isForeground=true' <<<"${service_dump}" || fail 'foreground service is not active'
grep -q 'types=0x00000010' <<<"${service_dump}" || fail 'connectedDevice FGS type is absent'

"${adb}" -d shell input keyevent KEYCODE_SLEEP
sleep 3

elapsed=0
while (( elapsed < wait_seconds )); do
    wakefulness="$("${adb}" -d shell dumpsys power \
        | sed -n 's/^  mWakefulness=//p' | head -1 | tr -d '\r')"
    case "${wakefulness}" in
        Asleep|Dozing) ;;
        *) fail "display woke before job at ${elapsed}s (wakefulness=${wakefulness:-UNKNOWN})" ;;
    esac
    printf 'SCREEN_STATE elapsed_s=%s wakefulness=%s\n' "${elapsed}" "${wakefulness}"
    remaining=$((wait_seconds - elapsed))
    interval="${poll_seconds}"
    if (( remaining < interval )); then interval="${remaining}"; fi
    sleep "${interval}"
    elapsed=$((elapsed + interval))
done

wakefulness="$("${adb}" -d shell dumpsys power \
    | sed -n 's/^  mWakefulness=//p' | head -1 | tr -d '\r')"
case "${wakefulness}" in
    Asleep|Dozing) ;;
    *) fail "display woke before job submission (wakefulness=${wakefulness:-UNKNOWN})" ;;
esac
if "${adb}" -d shell dumpsys activity activities \
    | grep -qE 'ActivityRecord\{[^}]+org\.phoneboost\.app/\.MainActivity'; then
    fail 'PhoneBoost Activity reappeared before job submission'
fi

compute_output="$("${ctl}" compute blake3 c10-abc-v1)" || {
    printf '%s\n' "${compute_output}" >&2
    fail 'remote compute command failed'
}
expected_compute='BLAKE3 fixture: c10-abc-v1
Input bytes: 3
BLAKE3 digest: 6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85
Execution source: REMOTE_SUCCESS
Auto-use reason: READY'
test "${compute_output}" = "${expected_compute}" || {
    printf '%s\n' "${compute_output}" >&2
    fail 'exact REMOTE_SUCCESS result was not observed'
}

final_wakefulness="$("${adb}" -d shell dumpsys power \
    | sed -n 's/^  mWakefulness=//p' | head -1 | tr -d '\r')"
case "${final_wakefulness}" in
    Asleep|Dozing) ;;
    *) fail "display woke during remote compute (wakefulness=${final_wakefulness:-UNKNOWN})" ;;
esac

printf '%s\n' \
    'ANDROID_DURABLE_WORKER PASS' \
    "SCREEN_OFF_WAIT_SECONDS=${wait_seconds}" \
    'ACTIVITY=ABSENT' \
    'FGS=CONNECTED_DEVICE' \
    'WORKER=AUTHENTICATED' \
    'JOB_SUBMITTED_AFTER_SCREEN_OFF=YES' \
    'EXECUTION_SOURCE=REMOTE_SUCCESS' \
    "SCREEN_AFTER_JOB=${final_wakefulness}" \
    'ADB_TUNNEL=NONE'
