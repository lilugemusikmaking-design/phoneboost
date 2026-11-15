//! Bounded, opt-in local intervals. No payload, handle, key or session ID.
//! Request tags are truncated SHA-256 of the existing random request ID; no
//! diagnostic data is sent on the wire. Epochs are process-local, never shared.
use crate::diagnostic_timing::Phase;
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::io::Write;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

pub const LIMIT: usize = 32_768;
pub const READY: u64 = 1000;
pub const ENQUEUE: u64 = 1001;
pub const SESSION_START: u64 = 1002;
pub const SEND_DONE: u64 = 1003;
pub const NOTIFY: u64 = 1004;
pub const AWAKE: u64 = 1005;
pub const TX_FRAME: u64 = 1010;
pub const RX_FRAME: u64 = 1011;
pub const REASSEMBLED: u64 = 1012;
pub const HANDLER_ENQUEUE: u64 = 1013;
pub const HANDLER_START: u64 = 1014;
pub const HANDLER_END: u64 = 1015;
pub const HANDLER_OBSERVED: u64 = 1016;
pub const RESPONSE_BUILT: u64 = 1017;
pub const RESPONSE_SENT: u64 = 1018;

// [kind, request tag (or caller-local token), local thread, start ns, end ns, value]
pub type Row = [u64; 6];
static ENABLED: AtomicU8 = AtomicU8::new(2);
static ORIGIN: OnceLock<Instant> = OnceLock::new();
static THREADS: AtomicU64 = AtomicU64::new(1);
static CALLERS: AtomicU64 = AtomicU64::new(1);
static DROPPED: AtomicU64 = AtomicU64::new(0);
static ROWS: Mutex<Vec<Row>> = Mutex::new(Vec::new());
thread_local! { static THREAD: Cell<u64> = const { Cell::new(0) }; }

pub fn enabled() -> bool {
    if ENABLED.load(Ordering::Relaxed) == 2 {
        let on = std::env::var_os("PHONEBOOST_PUT_DIAGNOSTIC").is_some_and(|v| v == "1");
        let _ = ENABLED.compare_exchange(2, u8::from(on), Ordering::Relaxed, Ordering::Relaxed);
    }
    ENABLED.load(Ordering::Relaxed) == 1
}

/// Only at a quiescent diagnostic boundary. Independent of product state.
pub fn control(on: bool) {
    ENABLED.store(0, Ordering::Relaxed);
    if let Ok(mut rows) = ROWS.lock() {
        rows.clear();
        if on && rows.capacity() < LIMIT {
            *rows = Vec::with_capacity(LIMIT);
        }
    }
    DROPPED.store(0, Ordering::Relaxed);
    ORIGIN.get_or_init(Instant::now);
    ENABLED.store(u8::from(on), Ordering::Relaxed);
}

pub fn now() -> Option<Instant> {
    enabled().then(|| {
        ORIGIN.get_or_init(Instant::now);
        Instant::now()
    })
}

pub fn tag(request_id: u64) -> u64 {
    if !enabled() {
        return 0;
    }
    let digest = Sha256::digest(request_id.to_le_bytes());
    // 48 bits are lossless in JSON/JavaScript and positive in JNI jlong.
    digest[..6]
        .iter()
        .fold(0, |value, byte| (value << 8) | u64::from(*byte))
}

pub fn caller() -> u64 {
    if enabled() {
        CALLERS.fetch_add(1, Ordering::Relaxed)
    } else {
        0
    }
}

pub fn mark(kind: u64, key: u64, value: u64) {
    interval(kind, key, now(), value);
}

pub fn interval(kind: u64, key: u64, start: Option<Instant>, value: u64) {
    let Some(start) = start else {
        return;
    };
    if !enabled() {
        return;
    }
    let end = Instant::now();
    let origin = *ORIGIN.get_or_init(|| start);
    let ns = |t: Instant| {
        t.saturating_duration_since(origin)
            .as_nanos()
            .min(i64::MAX as u128) as u64
    };
    let thread = THREAD.with(|slot| {
        if slot.get() == 0 {
            slot.set(THREADS.fetch_add(1, Ordering::Relaxed));
        }
        slot.get()
    });
    let row = [kind, key, thread, ns(start), ns(end), value];
    if let Ok(mut rows) = ROWS.lock() {
        if rows.len() < LIMIT {
            rows.push(row);
            return;
        }
    }
    DROPPED.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn phase(phase: Phase, start: Instant, bytes: u64) {
    if matches!(
        phase,
        Phase::FrameEncode
            | Phase::Encrypt
            | Phase::WireWrite
            | Phase::WireReadPrefix
            | Phase::WireReadBody
            | Phase::Decrypt
            | Phase::FrameDecode
            | Phase::Reassembly
            | Phase::BufferRequestParse
            | Phase::SessionPoll
            | Phase::BufferHandlerPoll
            | Phase::WorkerPutCopy
    ) {
        interval(phase as u64, 0, Some(start), bytes);
    }
}

pub fn snapshot() -> (Vec<Row>, u64) {
    match ROWS.lock() {
        Ok(rows) => (rows.clone(), DROPPED.load(Ordering::Relaxed)),
        Err(_) => (
            Vec::new(),
            DROPPED.load(Ordering::Relaxed).saturating_add(1),
        ),
    }
}

/// Called after the local client has closed, outside the measured operation.
pub fn emit() {
    if !enabled() {
        return;
    }
    let (rows, dropped) = snapshot();
    let mut output = format!("PB_PUT_TRACE {{\"schema\":1,\"dropped\":{dropped},\"rows\":[");
    use std::fmt::Write as _;
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        let _ = write!(
            output,
            "[{},{},{},{},{},{}]",
            row[0], row[1], row[2], row[3], row[4], row[5]
        );
    }
    output.push_str("]}");
    let _ = writeln!(std::io::stderr().lock(), "{output}");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opt_in_bounded_numeric_intervals_and_pseudonyms() {
        control(false);
        mark(READY, 1, 2);
        assert!(snapshot().0.is_empty());
        control(true);
        assert_eq!(tag(42), tag(42));
        assert_ne!(tag(42), tag(43));
        assert!(tag(42) < (1 << 48));
        for _ in 0..LIMIT + 1 {
            mark(READY, 1, 2);
        }
        let (rows, dropped) = snapshot();
        assert_eq!(rows.len(), LIMIT);
        assert!(dropped >= 1);
        assert!(rows.iter().all(|r| r[3] <= r[4]));
        control(false);
        assert_eq!(caller(), 0);
    }
}
