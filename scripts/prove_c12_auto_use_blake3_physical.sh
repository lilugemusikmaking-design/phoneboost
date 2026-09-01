#!/bin/sh
set -u

phoneboostctl_bin=${PHONEBOOSTCTL_BIN:-target/debug/phoneboostctl}
max_attempts=30
attempt=1

if [ ! -x "$phoneboostctl_bin" ]; then
    echo "PROOF FAIL: phoneboostctl binary is not executable" >&2
    exit 1
fi

ready=0
while [ "$attempt" -le "$max_attempts" ]; do
    if status_output=$("$phoneboostctl_bin" status 2>&1); then
        if printf '%s\n' "$status_output" | grep -Fqx 'Auto-use: AVAILABLE' \
            && printf '%s\n' "$status_output" | grep -Fqx 'Auto-use reason: READY' \
            && printf '%s\n' "$status_output" | grep -Fqx 'Remote BLAKE3: AVAILABLE'
        then
            ready=1
            break
        fi
    fi
    attempt=$((attempt + 1))
    sleep 1
done

if [ "$ready" -ne 1 ]; then
    echo "PROOF FAIL: AVAILABLE / READY / true not observed within 30 seconds" >&2
    if [ "${status_output+x}" = x ]; then
        printf '%s\n' "$status_output" >&2
    fi
    exit 1
fi

printf '%s\n' "$status_output"
compute_output=$("$phoneboostctl_bin" compute blake3 c10-abc-v1 2>&1)
compute_exit=$?
printf '%s\n' "$compute_output"

expected_compute='BLAKE3 fixture: c10-abc-v1
Input bytes: 3
BLAKE3 digest: 6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85
Execution source: REMOTE_SUCCESS
Auto-use reason: READY'

if [ "$compute_exit" -ne 0 ] || [ "$compute_output" != "$expected_compute" ]; then
    echo "PROOF FAIL: exact REMOTE_SUCCESS result not observed" >&2
    exit 1
fi

echo "C12_AUTO_USE_BLAKE3_PHYSICAL_PROOF PASS"
