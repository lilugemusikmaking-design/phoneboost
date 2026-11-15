#!/usr/bin/env bash
set -euo pipefail
PROFILE="${1:?usage: $0 1|2|4}"
case "$PROFILE" in 1|2|4) ;; *) echo "profile must be 1, 2 or 4" >&2; exit 2 ;; esac
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
: "${PB_CTL:?path to experimental phoneboostctl}"
: "${PB_INPUT:?same regular input file for all profiles}"
: "${PB_EXPECTED_DIGEST:?independent expected BLAKE3 digest}"
: "${OUT_DIR:?new directory for this profile capture}"
[[ ! -e "$OUT_DIR" ]] || { echo "output already exists" >&2; exit 2; }
[[ -x "${TRACE_PROCESSOR:-$ROOT/trace_processor}" ]] || { echo "trace_processor unavailable" >&2; exit 2; }
exec python3 "$ROOT/../scripts/physical_campaign_power.py" \
  --state "${PB_POWER_STATE:-${OUT_DIR}.power-state.json}" -- \
  bash "$ROOT/capture_profile.sh" "$PROFILE"
