#!/usr/bin/env python3
"""Deterministic SQL math check; synthetic slices are never physical evidence."""
import csv
import io
import os
from pathlib import Path
import subprocess
import tempfile

sql = (Path(__file__).parent / "analyze.sql").read_text()
# Replace only trace extraction with known Running slices. Keep the supplied
# overlap and migration SQL, whose exact result is independently calculable.
math = sql[sql.index("CREATE PERFETTO TABLE pb_overlap AS"):]
rows = [(0, 10, 0, 10), (2, 4, 1, 11), (8, 4, 2, 11), (3, 2, 2, 12), (4, 1, 3, 13)]
fixture = "INCLUDE PERFETTO MODULE intervals.overlap;\nCREATE PERFETTO TABLE pb_running AS "
fixture += " UNION ALL ".join(
    f"SELECT {start*1000000} AS ts, {duration*1000000} AS dur, {cpu} AS cpu, "
    f"{tid} AS utid, {tid} AS tid, 'pb-cpu' AS thread_name"
    for start, duration, cpu, tid in rows) + ";\n"
with tempfile.TemporaryDirectory() as directory:
    trace = Path(directory) / "empty.pftrace"
    query = Path(directory) / "fixture.sql"
    trace.touch()
    query.write_text(fixture + math)
    result = subprocess.run([os.environ["TRACE_PROCESSOR"], "query", "--format", "csv",
        str(trace), "-f", str(query)], check=True, capture_output=True, text=True)
    row = next(csv.DictReader(io.StringIO(result.stdout)))
    for name, expected in {"observed_compute_threads": 4, "cpus_used": 4,
        "max_simultaneous_running": 4, "time_ge_2_threads_ms": 6,
        "time_ge_3_threads_ms": 2, "time_ge_4_threads_ms": 1,
        "cpu_time_ms": 21, "migration_count": 1}.items():
        assert float(row[name]) == expected, (name, row[name], expected)
print("PASS: synthetic overlap/migrations SQL fixture; no physical claim")
