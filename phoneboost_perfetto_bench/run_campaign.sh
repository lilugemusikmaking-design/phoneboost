#!/usr/bin/env bash
set -euo pipefail

PROFILE="${1:?usage: $0 1|2|4}"
case "$PROFILE" in
  1|2|4) ;;
  *) echo "profile must be 1, 2 or 4" >&2; exit 2 ;;
esac

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TRACE_PROCESSOR="${TRACE_PROCESSOR:-$ROOT/trace_processor}"
: "${PB_CTL:?path to experimental phoneboostctl}"
: "${PB_INPUT:?same regular input file for all profiles}"
: "${PB_EXPECTED_DIGEST:?independent expected BLAKE3 digest}"
OUT_DIR="${OUT_DIR:?new directory for this profile capture}"
mkdir "$OUT_DIR"
DEVICE_CONFIG="/data/local/tmp/sched_pb.pbtxt"
DEVICE_TRACE="/data/misc/perfetto-traces/pb_sched_${PROFILE}.pftrace"
OUT="$OUT_DIR/pb_${PROFILE}.pftrace"

command -v adb >/dev/null 2>&1 || { echo "adb not found" >&2; exit 1; }

if [[ ! -x "$TRACE_PROCESSOR" ]]; then
  echo "trace_processor not found/executable at: $TRACE_PROCESSOR" >&2
  echo "Download once from https://get.perfetto.dev/trace_processor" >&2
  exit 1
fi

adb -d get-state >/dev/null
adb -d push "$ROOT/sched_pb.pbtxt" "$DEVICE_CONFIG" >/dev/null

echo "=== Perfetto capture: profile ${PROFILE} ==="
adb -d shell "cat '$DEVICE_CONFIG' | perfetto --txt -c - -o '$DEVICE_TRACE'" > "$OUT_DIR/capture.log" 2>&1 &
PERFETTO_HOST_PID=$!

sleep 2

job_rc=0
python3 "$ROOT/../scripts/measure_android_multicore.py" \
  --ctl "$PB_CTL" --input "$PB_INPUT" --expected-digest "$PB_EXPECTED_DIGEST" \
  --profile "$PROFILE" --repetitions 0 --out "$OUT_DIR/job" \
  > "$OUT_DIR/job.log" 2>&1 || job_rc=$?

wait "$PERFETTO_HOST_PID"

adb -d pull "$DEVICE_TRACE" "$OUT" >/dev/null
[[ -s "$OUT" ]] || { echo "trace is empty: $OUT" >&2; exit 1; }

# Worker 0 also carries command traffic. Restrict the supplied analysis to the
# exact BOOTTIME compute intervals/TIDs emitted by PhoneBoost, never IPC traffic.
python3 - "$OUT_DIR" "$ROOT/analyze.sql" <<'PY'
import json, sys
from pathlib import Path
out = Path(sys.argv[1])
rows = json.loads((out / 'job/runs.json').read_text())
assert len(rows) == 1
row = rows[0]
windows = row['native'].get('threads', []) if row.get('valid') else []
sql = 'CREATE PERFETTO TABLE pb_compute_windows(tid INT, begin_ns LONG, end_ns LONG) AS '
if windows:
    values = []
    for t in windows:
        tid, begin, end = (int(t[k]) for k in ('tid', 'begin_ns', 'end_ns'))
        assert tid > 0 and 0 < begin < end
        values.append(f'SELECT {tid} AS tid, {begin} AS begin_ns, {end} AS end_ns')
    sql += ' UNION ALL '.join(values) + ';\n'
else:
    sql += 'SELECT 0 AS tid, 0 AS begin_ns, 0 AS end_ns WHERE 0;\n'
(out / 'analyze-job.sql').write_text(sql + Path(sys.argv[2]).read_text())
PY

"$TRACE_PROCESSOR" query --format csv "$OUT" -f "$OUT_DIR/analyze-job.sql" \
  | awk -v p="$PROFILE" '
      NR==1 {print "profile,configured_threads," $0}
      NR>1  {print p "," p "," $0}
    ' | tee "$OUT_DIR/metrics.csv"
"$TRACE_PROCESSOR" query --format csv "$OUT" \
  "SELECT name, value FROM stats WHERE value > 0 AND (severity = 'error' OR name GLOB '*overrun*' OR name GLOB '*overwrite*' OR name GLOB '*data_loss*');" > "$OUT_DIR/trace-errors.csv"
"$TRACE_PROCESSOR" query --format csv "$OUT" \
  "SELECT clock_id, clock_name, ts - clock_value AS offset_ns FROM clock_snapshot WHERE clock_id = 6 LIMIT 5;" > "$OUT_DIR/clock.csv"
"$TRACE_PROCESSOR" query --format csv "$OUT" \
  "SELECT start_ts, end_ts FROM trace_bounds;" > "$OUT_DIR/trace-bounds.csv"
exit "$job_rc"
