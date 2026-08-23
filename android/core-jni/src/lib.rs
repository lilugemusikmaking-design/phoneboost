use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, OnceLock};

use pb_worker_core::{WorkerCore, WorkerState};

type JInt = i32;
type JLong = i64;
type JniEnv = *mut c_void;
type JObject = *mut c_void;

const RESULT_OK: JInt = 0;
const RESULT_ALREADY_RUNNING: JInt = 1;
const ERROR_INTERNAL: JInt = -1;
const ERROR_NOT_RUNNING: JInt = -2;
const ERROR_ENTROPY: JInt = -3;
const ERROR_PANIC_CONTAINED: JInt = -4;

const STATE_STOPPED: JInt = 0;
const STATE_COLD_START: JInt = WorkerState::ColdStart as JInt;
const STATE_PAIRING_REQUIRED: JInt = WorkerState::PairingRequired as JInt;

fn worker_slot() -> &'static Mutex<Option<WorkerCore>> {
    static WORKER: OnceLock<Mutex<Option<WorkerCore>>> = OnceLock::new();
    WORKER.get_or_init(|| Mutex::new(None))
}

fn int_boundary(action: impl FnOnce() -> JInt) -> JInt {
    catch_unwind(AssertUnwindSafe(action)).unwrap_or(ERROR_PANIC_CONTAINED)
}

fn long_boundary(action: impl FnOnce() -> JLong) -> JLong {
    catch_unwind(AssertUnwindSafe(action)).unwrap_or(ERROR_PANIC_CONTAINED as JLong)
}

fn start_worker() -> JInt {
    let Ok(mut slot) = worker_slot().lock() else {
        return ERROR_INTERNAL;
    };
    if slot.is_some() {
        return RESULT_ALREADY_RUNNING;
    }
    match WorkerCore::cold_start() {
        Ok(worker) => {
            *slot = Some(worker);
            RESULT_OK
        }
        Err(_) => ERROR_ENTROPY,
    }
}

fn stop_worker() -> JInt {
    let Ok(mut slot) = worker_slot().lock() else {
        return ERROR_INTERNAL;
    };
    if slot.take().is_some() {
        RESULT_OK
    } else {
        ERROR_NOT_RUNNING
    }
}

fn worker_state() -> JInt {
    let Ok(slot) = worker_slot().lock() else {
        return ERROR_INTERNAL;
    };
    slot.as_ref()
        .map_or(STATE_STOPPED, |worker| match worker.state() {
            WorkerState::ColdStart => STATE_COLD_START,
            WorkerState::PairingRequired => STATE_PAIRING_REQUIRED,
        })
}

fn incarnation_word(high: bool) -> JLong {
    let Ok(slot) = worker_slot().lock() else {
        return ERROR_INTERNAL as JLong;
    };
    let Some(worker) = slot.as_ref() else {
        return ERROR_NOT_RUNNING as JLong;
    };
    let incarnation = worker.incarnation();
    if high {
        incarnation.high_u64() as JLong
    } else {
        incarnation.low_u64() as JLong
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_workerStart(
    _env: JniEnv,
    _object: JObject,
) -> JInt {
    int_boundary(start_worker)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_workerStatusState(
    _env: JniEnv,
    _object: JObject,
) -> JInt {
    int_boundary(worker_state)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_workerIncarnationHigh(
    _env: JniEnv,
    _object: JObject,
) -> JLong {
    long_boundary(|| incarnation_word(true))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_workerIncarnationLow(
    _env: JniEnv,
    _object: JObject,
) -> JLong {
    long_boundary(|| incarnation_word(false))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_workerStop(
    _env: JniEnv,
    _object: JObject,
) -> JInt {
    int_boundary(stop_worker)
}

#[cfg(feature = "jni-test-probes")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_workerPanicProbe(
    _env: JniEnv,
    _object: JObject,
) -> JInt {
    int_boundary(|| std::panic::panic_any("A5 JNI TEST-ONLY panic probe"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        let mut slot = worker_slot().lock().expect("worker test lock");
        *slot = None;
    }

    #[test]
    fn start_status_stop_and_restart_are_rust_owned() {
        reset();
        assert_eq!(start_worker(), RESULT_OK);
        assert_eq!(worker_state(), STATE_PAIRING_REQUIRED);
        let first = (incarnation_word(true), incarnation_word(false));
        assert_ne!(first, (0, 0));
        assert_eq!(start_worker(), RESULT_ALREADY_RUNNING);
        assert_eq!(first, (incarnation_word(true), incarnation_word(false)));
        assert_eq!(stop_worker(), RESULT_OK);
        assert_eq!(worker_state(), STATE_STOPPED);
        assert_eq!(start_worker(), RESULT_OK);
        let second = (incarnation_word(true), incarnation_word(false));
        assert_ne!(first, second);
        reset();
    }

    #[test]
    fn panic_boundary_returns_typed_fail_stop() {
        assert_eq!(
            int_boundary(|| std::panic::panic_any("test-only boundary probe")),
            ERROR_PANIC_CONTAINED
        );
    }

    #[test]
    fn stopped_status_never_claims_pairing_or_ready() {
        reset();
        assert_eq!(worker_state(), STATE_STOPPED);
        assert_eq!(incarnation_word(true), ERROR_NOT_RUNNING as JLong);
    }
}
