//! Explicit experimental exception, never enabled by a product build.
//! One C10 job, borrowed input, bounded scoped threads, unchanged BLAKE3 result.
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use blake3::hazmat::{
    HasherExt, Mode, left_subtree_len, merge_subtrees_non_root, merge_subtrees_root,
};
use pb_pbmux::ComputeReason;

use crate::{HealthStatus, ResourceGuard, SafetyBand};

pub const THREAD_STACK_BYTES: usize = 512 * 1024;
const QUANTUM_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CpuProfile {
    #[default]
    One,
    Two,
    Four,
}

impl CpuProfile {
    pub fn from_threads(threads: usize) -> Option<Self> {
        match threads {
            1 => Some(Self::One),
            2 => Some(Self::Two),
            4 => Some(Self::Four),
            _ => None,
        }
    }
    pub const fn threads(self) -> usize {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Four => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum CpuRefusal {
    Health = 1,
    Throttle = 2,
    CpuUnavailable = 3,
    Scratch = 4,
}

impl ResourceGuard {
    /// This is in addition to the normal C08 reservation and C10 authority checks.
    /// THROTTLE permits only the unchanged one-thread baseline.
    pub(crate) fn admit_cpu_profile(
        &self,
        profile: CpuProfile,
        cpus: usize,
        scratch: u64,
        now: u64,
    ) -> Result<(), CpuRefusal> {
        let health = self.health_status(now);
        if health.safety.is_refused() {
            return Err(CpuRefusal::Health);
        }
        if profile != CpuProfile::One && health.safety != SafetyBand::Nominal {
            return Err(CpuRefusal::Throttle);
        }
        if cpus < profile.threads() {
            return Err(CpuRefusal::CpuUnavailable);
        }
        // Three additional 512 KiB stacks at most, hashers, scoped-thread control
        // and bounded diagnostics fit inside the existing 8 MiB scratch class.
        // Require the full existing envelope; never silently enlarge it.
        if profile != CpuProfile::One && scratch != pb_pbmux::NATIVE_OP_SCRATCH_MAX_BYTES {
            return Err(CpuRefusal::Scratch);
        }
        Ok(())
    }
}

pub(crate) fn available_cpus() -> usize {
    rustix::thread::sched_getaffinity(None)
        .map(|set| set.count() as usize)
        .unwrap_or(0)
}

fn boottime_ns() -> u64 {
    ns(rustix::time::clock_gettime(rustix::time::ClockId::Boottime))
}
fn thread_cpu_ns() -> u64 {
    ns(rustix::time::clock_gettime(
        rustix::time::ClockId::ThreadCPUTime,
    ))
}
fn ns(time: rustix::time::Timespec) -> u64 {
    (time.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(time.tv_nsec as u64)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ThreadTrace {
    pub worker_index: usize,
    pub tid: u32,
    pub begin_ns: u64,
    pub end_ns: u64,
    pub cpu_ns: u64,
    pub cpus: u64,
    pub observed_migrations: u64,
    pub bytes: u64,
    pub cpu_id_overflow: bool,
}

#[derive(Clone, Debug, Default)]
pub struct HashTrace {
    pub wall_ns: u64,
    /// Overlapping hash scopes, NOT proof of simultaneous CPU execution.
    pub peak_hash_scopes: usize,
    pub threads: Vec<ThreadTrace>,
}

#[derive(Clone, Debug)]
pub struct CpuRecord {
    pub sequence: u64,
    pub profile: CpuProfile,
    pub input_bytes: u64,
    pub scratch_bytes: u64,
    pub held_before: u64,
    pub held_after: u64,
    pub health_before: HealthStatus,
    pub health_after: HealthStatus,
    pub refusal: Option<CpuRefusal>,
    pub outcome: ComputeReason,
    pub trace: HashTrace,
}

impl crate::WorkerCore {
    /// Trusted local experiment configuration only, with the worker mutex held.
    /// No PBMUX message can select this setting. Admission is checked per job.
    pub fn set_experimental_cpu_profile(&mut self, threads: usize) -> bool {
        let Some(profile) = CpuProfile::from_threads(threads) else {
            return false;
        };
        self.cpu_profile = profile;
        self.cpu_record = None;
        true
    }

    /// Bounded last workload/refusal diagnostic; tiny successful readiness
    /// probes cannot erase the workload evidence. Not an admission authority.
    pub fn experimental_cpu_field(&self, field: usize) -> i64 {
        let Some(r) = &self.cpu_record else {
            return -1;
        };
        if field >= 100 {
            let Some(t) = r.trace.threads.get((field - 100) / 10) else {
                return -1;
            };
            return match (field - 100) % 10 {
                0 => t.tid as i64,
                1 => t.begin_ns as i64,
                2 => t.end_ns as i64,
                3 => t.cpu_ns as i64,
                4 => t.cpus as i64,
                5 => t.observed_migrations as i64,
                6 => t.bytes as i64,
                7 => i64::from(t.cpu_id_overflow),
                8 => t.worker_index as i64,
                _ => -1,
            };
        }
        match field {
            0 => r.sequence as i64,
            1 => r.profile.threads() as i64,
            2 => r.input_bytes as i64,
            3 => r.trace.wall_ns as i64,
            4 => r.trace.threads.iter().map(|t| t.cpu_ns).sum::<u64>() as i64,
            5 => r.trace.peak_hash_scopes as i64,
            6 => r.trace.threads.len() as i64,
            7 => r.trace.threads.iter().fold(0, |mask, t| mask | t.cpus) as i64,
            8 => r.refusal.map_or(0, |reason| reason as i64),
            9 => r.outcome as i64,
            10 => r.scratch_bytes as i64,
            11 => r.held_before as i64,
            12 => r.held_after as i64,
            13 => r.health_before.safety as i64,
            14 => r.health_after.safety as i64,
            15 => r.health_before.thermal as i64,
            16 => r.health_after.thermal as i64,
            17 => r.health_before.sample_age_ms.map_or(-1, |age| age as i64),
            18 => r.health_after.sample_age_ms.map_or(-1, |age| age as i64),
            19 => r.health_before.available_memory_bytes as i64,
            20 => i64::from(r.health_before.charging),
            21 => i64::from(r.health_before.power_save),
            22 => r.health_before.budget_bytes as i64,
            23 => r.health_before.battery_percent as i64,
            _ => -1,
        }
    }
}

struct TraceState {
    active: AtomicUsize,
    peak: AtomicUsize,
    leaves: Mutex<Vec<ThreadTrace>>,
}

/// The check executes before and after every <=1 MiB update and at completion.
/// All spawned threads are joined before return, including error and unwind.
pub(crate) fn hash_bounded(
    input: &[u8],
    profile: CpuProfile,
    check: &(impl Fn() -> Result<(), ComputeReason> + Sync),
) -> (Result<[u8; 32], ComputeReason>, HashTrace) {
    let trace = TraceState {
        active: AtomicUsize::new(0),
        peak: AtomicUsize::new(0),
        leaves: Mutex::new(Vec::with_capacity(profile.threads())),
    };
    let begin = boottime_ns();
    // The existing command thread is compute worker 0. The name is diagnostic;
    // Perfetto analysis selects its TID only inside the recorded compute interval.
    let result = rustix::thread::set_name(c"pb-cpu-0")
        .map_err(|_| ComputeReason::ProviderFailed)
        .and_then(|()| tree(input, 0, profile.threads(), 0, true, check, &trace))
        .and_then(|digest| {
            check()?;
            Ok(digest)
        });
    let wall_ns = boottime_ns().saturating_sub(begin);
    let mut leaves = trace.leaves.into_inner().unwrap_or_else(|p| p.into_inner());
    leaves.sort_by_key(|entry| entry.tid);
    (
        result,
        HashTrace {
            wall_ns,
            peak_hash_scopes: trace.peak.load(Ordering::Relaxed),
            threads: leaves,
        },
    )
}

fn tree(
    input: &[u8],
    offset: u64,
    workers: usize,
    worker_index: usize,
    root: bool,
    check: &(impl Fn() -> Result<(), ComputeReason> + Sync),
    trace: &TraceState,
) -> Result<[u8; 32], ComputeReason> {
    check()?;
    if workers == 1 || input.len() <= blake3::CHUNK_LEN {
        let mut record = ThreadTrace {
            worker_index,
            tid: rustix::thread::gettid().as_raw_nonzero().get() as u32,
            begin_ns: boottime_ns(),
            ..ThreadTrace::default()
        };
        let cpu_start = thread_cpu_ns();
        let active = trace.active.fetch_add(1, Ordering::Relaxed) + 1;
        trace.peak.fetch_max(active, Ordering::Relaxed);
        let mut previous_cpu = None;
        let mut sample_cpu = || {
            let cpu = rustix::thread::sched_getcpu();
            if cpu < 64 {
                record.cpus |= 1 << cpu;
            } else {
                record.cpu_id_overflow = true;
            }
            if previous_cpu.is_some_and(|old| old != cpu) {
                record.observed_migrations += 1;
            }
            previous_cpu = Some(cpu);
        };
        let result = (|| {
            let mut hasher = blake3::Hasher::new();
            hasher.set_input_offset(offset);
            sample_cpu();
            for chunk in input.chunks(QUANTUM_BYTES) {
                check()?;
                hasher.update(chunk);
                record.bytes += chunk.len() as u64;
                sample_cpu();
                check()?;
            }
            check()?;
            Ok(if root {
                *hasher.finalize().as_bytes()
            } else {
                hasher.finalize_non_root()
            })
        })();
        record.cpu_ns = thread_cpu_ns().saturating_sub(cpu_start);
        record.end_ns = boottime_ns();
        trace.active.fetch_sub(1, Ordering::Relaxed);
        trace
            .leaves
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(record);
        return result;
    }
    // BLAKE3 requires the largest power-of-two left subtree strictly below len.
    // Splitting by an arbitrary equal byte count would produce a wrong digest.
    let split = left_subtree_len(input.len() as u64) as usize;
    let (left, right) = input.split_at(split);
    std::thread::scope(|scope| {
        let left_thread = std::thread::Builder::new()
            .name(format!("pb-cpu-{}", worker_index + workers / 2))
            .stack_size(THREAD_STACK_BYTES)
            .spawn_scoped(scope, || {
                tree(
                    left,
                    offset,
                    workers / 2,
                    worker_index + workers / 2,
                    false,
                    check,
                    trace,
                )
            })
            .map_err(|_| ComputeReason::ResourceExhausted)?;
        let right_result = tree(
            right,
            offset + split as u64,
            workers / 2,
            worker_index,
            false,
            check,
            trace,
        );
        let left_result = left_thread
            .join()
            .map_err(|_| ComputeReason::ProviderFailed)?;
        let left_cv = left_result?;
        let right_cv = right_result?;
        check()?;
        Ok(if root {
            *merge_subtrees_root(&left_cv, &right_cv, Mode::Hash).as_bytes()
        } else {
            merge_subtrees_non_root(&left_cv, &right_cv, Mode::Hash)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HealthSample, ThermalBand};
    const MIB: u64 = 1024 * 1024;

    fn ready() -> (ResourceGuard, HealthSample) {
        let mut guard = ResourceGuard::new();
        let mut sample = HealthSample {
            available_memory_bytes: 2 * 1024 * MIB,
            low_memory: false,
            thermal: ThermalBand::None,
            battery_percent: 80,
            charging: true,
            power_save: false,
            monotonic_ms: 0,
        };
        for time in [0, 5000, 10000] {
            sample.monotonic_ms = time;
            guard.record_health(sample);
        }
        (guard, sample)
    }

    #[test]
    fn exact_tree_equivalence_across_chunk_and_subtree_boundaries() {
        let input: Vec<u8> = (0..4 * 1024 * 1024 + 17)
            .map(|i| (i as u8).wrapping_mul(31))
            .collect();
        let mut lengths = vec![0, 1, 63, 64, 65, 1023, 1024, 1025, 2047, 2048, 2049];
        for power in 12..=22 {
            lengths.extend([(1 << power) - 1, 1 << power, (1 << power) + 1]);
        }
        lengths.extend([3073, 7001, 1024 * 1024 + 12345, input.len()]);
        for profile in [CpuProfile::One, CpuProfile::Two, CpuProfile::Four] {
            for &length in &lengths {
                let (digest, trace) = hash_bounded(&input[..length], profile, &|| Ok(()));
                assert_eq!(
                    digest.unwrap(),
                    *blake3::hash(&input[..length]).as_bytes(),
                    "{profile:?} len={length}"
                );
                assert!(trace.threads.len() <= profile.threads());
                assert!(
                    trace
                        .threads
                        .iter()
                        .all(|t| t.worker_index < profile.threads())
                );
                let indices: std::collections::BTreeSet<_> =
                    trace.threads.iter().map(|t| t.worker_index).collect();
                assert_eq!(indices.len(), trace.threads.len());
                assert!(trace.peak_hash_scopes <= profile.threads());
                assert_eq!(
                    trace.threads.iter().map(|t| t.bytes).sum::<u64>(),
                    length as u64
                );
                if length >= 4096 && length.is_power_of_two() {
                    assert_eq!(trace.threads.len(), profile.threads());
                }
            }
        }
    }

    #[test]
    fn guard_profiles_health_throttle_cpu_and_scratch_fail_closed() {
        let (mut guard, mut sample) = ready();
        for profile in [CpuProfile::One, CpuProfile::Two, CpuProfile::Four] {
            assert_eq!(guard.admit_cpu_profile(profile, 8, 8 * MIB, 10000), Ok(()));
            assert_eq!(
                guard.admit_cpu_profile(profile, 0, 8 * MIB, 10000),
                Err(CpuRefusal::CpuUnavailable)
            );
            assert_eq!(
                guard.admit_cpu_profile(profile, 8, 8 * MIB, 16001),
                Err(CpuRefusal::Health)
            );
        }
        assert_eq!(
            guard.admit_cpu_profile(CpuProfile::Four, 2, 8 * MIB, 10000),
            Err(CpuRefusal::CpuUnavailable)
        );
        assert_eq!(
            guard.admit_cpu_profile(CpuProfile::Two, 8, 4 * MIB, 10000),
            Err(CpuRefusal::Scratch)
        );
        for condition in 0..3 {
            let (mut g, mut s) = ready();
            match condition {
                0 => s.power_save = true,
                1 => s.thermal = ThermalBand::Moderate,
                _ => {
                    s.battery_percent = 20;
                    s.charging = false;
                }
            }
            g.record_health(s);
            assert_eq!(
                g.admit_cpu_profile(CpuProfile::One, 8, 8 * MIB, 10000),
                Ok(())
            );
            for p in [CpuProfile::Two, CpuProfile::Four] {
                assert_eq!(
                    g.admit_cpu_profile(p, 8, 8 * MIB, 10000),
                    Err(CpuRefusal::Throttle)
                );
            }
        }
        sample.low_memory = true;
        guard.record_health(sample);
        assert_eq!(
            guard.admit_cpu_profile(CpuProfile::One, 8, 8 * MIB, 10000),
            Err(CpuRefusal::Health)
        );
        guard.invalidate_health();
        assert_eq!(
            guard.admit_cpu_profile(CpuProfile::Four, 8, 8 * MIB, 10000),
            Err(CpuRefusal::Health)
        );
        assert!(CpuProfile::from_threads(3).is_none());
        assert_eq!(crate::NATIVE_OPERATION_THREADS, 1);
        assert!(3 * THREAD_STACK_BYTES + 512 * 1024 < 8 * MIB as usize);
    }

    #[test]
    fn interruption_errors_return_no_partial_digest_and_join_every_thread() {
        let input = vec![7; 8 * MIB as usize];
        for reason in [
            ComputeReason::SessionLost,
            ComputeReason::StaleControllerLease,
            ComputeReason::ProviderTimeout,
            ComputeReason::ResourceExhausted,
        ] {
            for profile in [CpuProfile::One, CpuProfile::Two, CpuProfile::Four] {
                let calls = AtomicUsize::new(0);
                let check = || {
                    if calls.fetch_add(1, Ordering::SeqCst) >= 8 {
                        Err(reason)
                    } else {
                        Ok(())
                    }
                };
                let (result, trace) = hash_bounded(&input, profile, &check);
                assert_eq!(result, Err(reason));
                assert!(trace.threads.len() <= profile.threads());
                assert!(trace.threads.iter().all(|t| t.end_ns >= t.begin_ns));
                // No background work survives return, including the sibling of a failed branch.
                let count = calls.load(Ordering::SeqCst);
                std::thread::yield_now();
                assert_eq!(calls.load(Ordering::SeqCst), count);
            }
        }
    }

    #[test]
    fn admission_keeps_existing_total_and_class_caps() {
        use crate::{
            AuthenticatedSession, ControllerLeaseManager, RequestId, ReservationDecision,
            ReserveRequest, ResourceClass, WorkerIncarnationId,
        };
        let (mut guard, _) = ready();
        let session =
            AuthenticatedSession::test_only(pb_types::PeerId::from_sha256_digest([7; 32]));
        let mut leases = ControllerLeaseManager::new(WorkerIncarnationId([1; 16]));
        let lease = leases.acquire(&session, 10000).unwrap();
        let request = |id, bytes, class| ReserveRequest {
            request_id: RequestId::from_u64(id),
            bytes,
            class,
        };
        assert!(matches!(
            guard.reserve(
                &mut leases,
                &session,
                lease,
                request(1, 121 * MIB, ResourceClass::RemoteBufferBytes),
                10000
            ),
            ReservationDecision::Reserved { .. }
        ));
        assert_eq!(
            guard.reserve(
                &mut leases,
                &session,
                lease,
                request(2, 8 * MIB, ResourceClass::NativeOpScratchBytes),
                10000
            ),
            ReservationDecision::RefusedBudget
        );
        assert_eq!(guard.reserved_bytes(), 121 * MIB);
        assert_eq!(
            guard.reserve(
                &mut leases,
                &session,
                lease,
                request(3, 9 * MIB, ResourceClass::NativeOpScratchBytes),
                10000
            ),
            ReservationDecision::RefusedBudget
        );
    }
}
