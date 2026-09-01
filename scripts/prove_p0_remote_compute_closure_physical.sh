#!/bin/sh
set -u

phoneboostctl_bin=${PHONEBOOSTCTL_BIN:-target/debug/phoneboostctl}
positive_proof=${PHONEBOOST_POSITIVE_PROOF:-scripts/prove_c12_auto_use_blake3_physical.sh}
max_attempts=30

fail() {
    printf 'P0_REMOTE_COMPUTE_CLOSURE FAIL: %s\n' "$1" >&2
    exit 1
}

require_exact_line() {
    printf '%s\n' "$1" | grep -Fqx "$2"
}

report_positive_invariants() {
    printf '%s\n' 'AUTHENTICATED_RECONNECT PASS'
    printf '%s\n' 'C07_CONTROLLER_LEASE PASS (required by AVAILABLE / READY)'
    printf '%s\n' \
        'RESOURCE_GUARD_READINESS PASS (required by the C08/C10 readiness probe)'
    printf '%s\n' 'REMOTE_CLEANUP PASS (required before REMOTE_SUCCESS)'
}

wait_for_remote_unavailable() {
    attempt=1
    while [ "$attempt" -le "$max_attempts" ]; do
        if unavailable_status=$("$phoneboostctl_bin" status 2>&1) \
            && require_exact_line "$unavailable_status" 'Remote BLAKE3: UNAVAILABLE'
        then
            printf '%s\n' "$unavailable_status"
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 1
    done
    if [ "${unavailable_status+x}" = x ]; then
        printf '%s\n' "$unavailable_status" >&2
    fi
    return 1
}

prove_truthful_fallback() {
    fallback_output=$("$phoneboostctl_bin" compute blake3 c10-abc-v1 2>&1)
    fallback_exit=$?
    printf '%s\n' "$fallback_output"

    [ "$fallback_exit" -eq 3 ] || return 1
    require_exact_line "$fallback_output" 'BLAKE3 fixture: c10-abc-v1' || return 1
    require_exact_line "$fallback_output" 'Input bytes: 3' || return 1
    require_exact_line "$fallback_output" \
        'BLAKE3 digest: 6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85' \
        || return 1
    if require_exact_line "$fallback_output" 'Execution source: REMOTE_SUCCESS'; then
        return 1
    fi
    if require_exact_line "$fallback_output" \
        'Execution source: LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE'
    then
        printf '%s\n' 'DISCONNECT_SOURCE LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE'
        return 0
    fi
    if require_exact_line "$fallback_output" \
        'Execution source: LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE'
    then
        printf '%s\n' 'DISCONNECT_SOURCE LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE'
        return 0
    fi
    return 1
}

[ -x "$phoneboostctl_bin" ] || fail 'phoneboostctl binary is not executable'
[ -x "$positive_proof" ] || fail 'positive production proof script is not executable'
[ -t 0 ] || fail 'interactive terminal required for bounded physical actions'

printf '%s\n' 'P0 PHASE 1/3 — prove current production remote path'
PHONEBOOSTCTL_BIN="$phoneboostctl_bin" "$positive_proof" \
    || fail 'initial AVAILABLE / READY / REMOTE_SUCCESS proof failed'
report_positive_invariants

printf '%s\n' \
    'ACTION 1/2: stop the Android PhoneBoost worker or disconnect Android from the LAN.'
printf '%s' 'Press Enter only after the worker is no longer reachable: '
IFS= read -r confirmation || fail 'disconnect confirmation was not received'

printf '%s\n' 'P0 PHASE 2/3 — prove disconnect cannot report remote success'
wait_for_remote_unavailable \
    || fail 'remote readiness did not become unavailable within 30 seconds'
prove_truthful_fallback \
    || fail 'disconnect did not produce an explicit local fallback with exit code 3'
printf '%s\n' 'NO_FALSE_REMOTE_SUCCESS PASS'

printf '%s\n' \
    'ACTION 2/2: restart the Android PhoneBoost worker or restore its LAN connectivity.'
printf '%s' 'Press Enter only after the worker is available again: '
IFS= read -r confirmation || fail 'reconnect confirmation was not received'

printf '%s\n' 'P0 PHASE 3/3 — prove authenticated production reconnect and remote compute'
PHONEBOOSTCTL_BIN="$phoneboostctl_bin" "$positive_proof" \
    || fail 'post-disconnect AVAILABLE / READY / REMOTE_SUCCESS proof failed'
report_positive_invariants

printf '%s\n' 'P0_REMOTE_COMPUTE_CLOSURE PASS'
printf '%s\n' 'PHYSICAL_EVIDENCE may be recorded only from this completed run.'
