INCLUDE PERFETTO MODULE intervals.overlap;

CREATE PERFETTO TABLE pb_running AS
SELECT
  MAX(s.ts, w.begin_ns) AS ts,
  MIN(s.ts + s.dur, w.end_ns) - MAX(s.ts, w.begin_ns) AS dur,
  s.cpu,
  s.utid,
  t.tid,
  t.name AS thread_name
FROM sched s
JOIN thread t USING (utid)
JOIN process p USING (upid)
JOIN pb_compute_windows w ON w.tid = t.tid
WHERE p.name = 'org.phoneboost.app.multicore'
  AND t.name GLOB 'pb-cpu-*'
  AND s.dur > 0
  AND s.ts < w.end_ns AND s.ts + s.dur > w.begin_ns;

CREATE PERFETTO TABLE pb_overlap AS
SELECT ts, value AS concurrent
FROM intervals_overlap_count!(pb_running, ts, dur);

CREATE PERFETTO TABLE pb_overlap_dur AS
SELECT
  ts,
  concurrent,
  LEAD(ts) OVER (ORDER BY ts) - ts AS dur
FROM pb_overlap;

CREATE PERFETTO TABLE pb_migrations AS
SELECT
  utid,
  tid,
  thread_name,
  ts,
  cpu,
  LAG(cpu) OVER (PARTITION BY utid ORDER BY ts) AS prev_cpu
FROM pb_running;

SELECT
  (SELECT COUNT(DISTINCT tid) FROM pb_running) AS observed_compute_threads,
  (SELECT COUNT(DISTINCT cpu) FROM pb_running) AS cpus_used,
  (SELECT COALESCE(MAX(concurrent), 0) FROM pb_overlap) AS max_simultaneous_running,
  (SELECT COALESCE(SUM(dur), 0) / 1e6 FROM pb_overlap_dur WHERE concurrent >= 2) AS time_ge_2_threads_ms,
  (SELECT COALESCE(SUM(dur), 0) / 1e6 FROM pb_overlap_dur WHERE concurrent >= 3) AS time_ge_3_threads_ms,
  (SELECT COALESCE(SUM(dur), 0) / 1e6 FROM pb_overlap_dur WHERE concurrent >= 4) AS time_ge_4_threads_ms,
  (SELECT COALESCE(SUM(dur), 0) / 1e6 FROM pb_running) AS cpu_time_ms,
  (SELECT COUNT(*) FROM pb_migrations WHERE prev_cpu IS NOT NULL AND cpu != prev_cpu) AS migration_count,
  (SELECT GROUP_CONCAT(DISTINCT cpu) FROM pb_running) AS cpu_ids;
