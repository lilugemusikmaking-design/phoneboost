//! Opt-in, bounded numeric counters for a single controlled diagnostic job.
//! No payload, identifier, protocol field, timeout or admission decision is stored.
use std::io::Write;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::Instant;

macro_rules! phases {
    ($($name:ident),+ $(,)?) => {
        #[derive(Clone, Copy)]
        #[repr(usize)]
        pub enum Phase { $($name),+ }
        pub const NAMES: &[&str] = &[$(stringify!($name)),+];
    };
}
phases!(
    CliTotal,
    CliConnect,
    CliCreate,
    CliReadyWait,
    CliPoll,
    CliPollSleep,
    CliSubmit,
    CliResult,
    LocalRequest,
    ImportStart,
    ImportRead,
    ImportWrite,
    ImportFlush,
    ImportFsync,
    ImportFinalize,
    ImportConsume,
    HostTotal,
    HostLock,
    HostPlacement,
    BufferReserveCommit,
    BufferAlloc,
    PutLoop,
    PutPrepare,
    PutFull,
    PutTail,
    BufferStat,
    ScratchReserveCommit,
    ComputeSubmit,
    ComputeStatus,
    ComputePollSleep,
    RemoteCleanup,
    ClientQueueLock,
    ClientResponseWait,
    RequestBuild,
    FrameEncode,
    Encrypt,
    WireWrite,
    WireReadPrefix,
    WireReadBody,
    Decrypt,
    FrameDecode,
    TxFrame,
    RxFrame,
    TxControl,
    RxControl,
    SessionPoll,
    BufferHandlerWait,
    BufferHandlerPoll,
    ComputeHandlerWait,
    ComputeHandlerPoll,
    JniLock,
    WorkerBuffer,
    WorkerAlloc,
    WorkerPutCopy,
    WorkerCompute,
    LeaseAcquire,
    LeaseRenew,
    Reassembly,
    BufferRequestParse,
    SessionReadableWake,
    SessionTimeoutWake,
    SessionPartialWait,
);
const N: usize = NAMES.len();
static ENABLED: AtomicU8 = AtomicU8::new(2);
struct Metric {
    calls: AtomicU64,
    ns: AtomicU64,
    bytes: AtomicU64,
}
static METRICS: [Metric; N] = [const {
    Metric {
        calls: AtomicU64::new(0),
        ns: AtomicU64::new(0),
        bytes: AtomicU64::new(0),
    }
}; N];

pub fn initialize_from_environment() {
    set_enabled(std::env::var_os("PHONEBOOST_TIMING_DIAGNOSTIC").is_some_and(|v| v == "1"));
    // Reserve the bounded trace before serving work, never during a PUT.
    crate::put_timing::control(
        std::env::var_os("PHONEBOOST_PUT_DIAGNOSTIC").is_some_and(|v| v == "1"),
    );
}
pub fn set_enabled(enabled: bool) {
    ENABLED.store(u8::from(enabled), Ordering::Relaxed);
}
/// Operator-only reset at a quiescent boundary; never used for product state.
pub fn reset() {
    for m in &METRICS {
        m.calls.store(0, Ordering::Relaxed);
        m.ns.store(0, Ordering::Relaxed);
        m.bytes.store(0, Ordering::Relaxed);
    }
}
fn enabled() -> bool {
    let state = ENABLED.load(Ordering::Relaxed);
    if state < 2 {
        return state == 1;
    }
    let value =
        u8::from(std::env::var_os("PHONEBOOST_TIMING_DIAGNOSTIC").is_some_and(|v| v == "1"));
    let _ = ENABLED.compare_exchange(2, value, Ordering::Relaxed, Ordering::Relaxed);
    ENABLED.load(Ordering::Relaxed) == 1
}

pub struct Span {
    started: Option<Instant>,
    phase: Phase,
    bytes: u64,
}
impl Span {
    #[inline]
    pub fn new(phase: Phase, bytes: u64) -> Self {
        Self {
            started: enabled().then(Instant::now),
            phase,
            bytes,
        }
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some(start) = self.started {
            let elapsed = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            let m = &METRICS[self.phase as usize];
            m.ns.fetch_add(elapsed, Ordering::Relaxed);
            m.bytes.fetch_add(self.bytes, Ordering::Relaxed);
            m.calls.fetch_add(1, Ordering::Relaxed);
            crate::put_timing::phase(self.phase, start, self.bytes);
        }
    }
}
pub fn field(phase: usize, field: usize) -> Option<u64> {
    let m = METRICS.get(phase)?;
    Some(match field {
        0 => m.calls.load(Ordering::Relaxed),
        1 => m.ns.load(Ordering::Relaxed),
        2 => m.bytes.load(Ordering::Relaxed),
        _ => return None,
    })
}
pub struct Snapshot([[u64; 3]; N]);
impl Snapshot {
    pub fn value(&self, phase: usize, field: usize) -> Option<u64> {
        self.0.get(phase)?.get(field).copied()
    }
    pub fn take() -> Self {
        Self(std::array::from_fn(|p| {
            std::array::from_fn(|f| field(p, f).unwrap())
        }))
    }
    /// Snapshots are process-wide; concurrent/background traffic is included.
    /// Host/Android clocks are intentionally never exported as a common epoch.
    pub fn emit_delta(&self, source: &'static str) {
        if !enabled() {
            return;
        }
        let end = Self::take();
        let mut out = String::from("PB_TIMING {\"schema\":1,\"source\":\"");
        // Only fixed call-site labels; this function never receives request data.
        out.push_str(match source {
            "cli" => "cli",
            _ => "host",
        });
        out.push_str("\",\"metrics\":[");
        for (p, name) in NAMES.iter().enumerate() {
            if p != 0 {
                out.push(',');
            }
            let d: [u64; 3] = std::array::from_fn(|f| end.0[p][f].saturating_sub(self.0[p][f]));
            use std::fmt::Write;
            let _ = write!(out, "[\"{}\",{},{},{}]", name, d[0], d[1], d[2]);
        }
        out.push_str("]}");
        // Diagnostic output failure cannot change the operation outcome.
        let _ = writeln!(std::io::stderr().lock(), "{out}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opt_in_bounded_numeric_metrics_preserve_return_and_error() {
        reset();
        set_enabled(false);
        let result: Result<u8, ()> = crate::timed!(CliTotal, 9, Err(()));
        assert_eq!(result, Err(()));
        assert_eq!(field(Phase::CliTotal as usize, 0), Some(0));
        set_enabled(true);
        let result: Result<u8, ()> = crate::timed!(CliTotal, 9, Ok(7));
        assert_eq!(result, Ok(7));
        assert_eq!(field(Phase::CliTotal as usize, 0), Some(1));
        assert_eq!(field(Phase::CliTotal as usize, 2), Some(9));
        assert!(field(N, 0).is_none());
        assert!(field(0, 3).is_none());
        set_enabled(false);
    }
    #[test]
    #[ignore = "explicit instrumentation overhead measurement"]
    fn measure_counter_overhead() {
        for enabled in [false, true] {
            set_enabled(enabled);
            let begin = Instant::now();
            for _ in 0..1_000_000 {
                drop(Span::new(Phase::LocalRequest, 0));
            }
            println!(
                "timing enabled={enabled} iterations=1000000 elapsed_ns={}",
                begin.elapsed().as_nanos()
            );
        }
        set_enabled(false);
    }
}
