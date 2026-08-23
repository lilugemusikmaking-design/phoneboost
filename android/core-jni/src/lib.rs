use std::ffi::c_void;
use std::net::TcpStream;
use std::os::fd::{FromRawFd, OwnedFd};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, OnceLock};

use pb_runtime_secure::{
    EndpointRole, PairingActionResult, RuntimeState, SecureRuntime, StateStore,
    run_responder_session,
};
use pb_worker_core::{
    HealthSample, LeaseState, ResourceGuardState, ThermalBand, WorkerCore, WorkerState,
};

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
const ERROR_BAD_LOCAL_SAMPLE: JInt = -5;
const ERROR_SECURE_STATE: JInt = -6;
const ERROR_SECURE_SESSION: JInt = -7;

const STATE_STOPPED: JInt = 0;
const STATE_COLD_START: JInt = WorkerState::ColdStart as JInt;
const STATE_PAIRING_REQUIRED: JInt = WorkerState::PairingRequired as JInt;

fn worker_slot() -> &'static Mutex<Option<WorkerCore>> {
    static WORKER: OnceLock<Mutex<Option<WorkerCore>>> = OnceLock::new();
    WORKER.get_or_init(|| Mutex::new(None))
}

fn secure_slot() -> &'static Mutex<Option<Arc<SecureRuntime>>> {
    static SECURE: OnceLock<Mutex<Option<Arc<SecureRuntime>>>> = OnceLock::new();
    SECURE.get_or_init(|| Mutex::new(None))
}

fn initialize_secure(directory_fd: JInt) -> JInt {
    if directory_fd < 0 {
        return ERROR_SECURE_STATE;
    }
    let Ok(mut slot) = secure_slot().lock() else {
        return ERROR_INTERNAL;
    };
    if slot.is_some() {
        // The descriptor ownership crosses JNI even on an idempotent call.
        drop(unsafe { OwnedFd::from_raw_fd(directory_fd) });
        return RESULT_ALREADY_RUNNING;
    }
    // SAFETY: Kotlin transfers an fd with ParcelFileDescriptor.detachFd and
    // never closes or reuses it afterwards. Rust becomes its sole owner here.
    let directory = unsafe { OwnedFd::from_raw_fd(directory_fd) };
    let Ok(store) = StateStore::from_directory_fd(directory) else {
        return ERROR_SECURE_STATE;
    };
    let Ok(runtime) = SecureRuntime::initialize(EndpointRole::AndroidResponder, store) else {
        return ERROR_SECURE_STATE;
    };
    *slot = Some(Arc::new(runtime));
    RESULT_OK
}

fn secure_runtime() -> Result<Arc<SecureRuntime>, JInt> {
    secure_slot()
        .lock()
        .map_err(|_| ERROR_INTERNAL)?
        .as_ref()
        .map(Arc::clone)
        .ok_or(ERROR_SECURE_STATE)
}

fn accept_secure_fd(socket_fd: JInt, prefix_first: JInt, prefix_second: JInt) -> JInt {
    if socket_fd < 0
        || !matches!(prefix_first, -1..=255)
        || !matches!(prefix_second, -1..=255)
        || (prefix_first == -1) != (prefix_second == -1)
    {
        return ERROR_SECURE_SESSION;
    }
    let runtime = match secure_runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            // SAFETY: the rejected JNI call still transferred this fd.
            drop(unsafe { OwnedFd::from_raw_fd(socket_fd) });
            return error;
        }
    };
    // SAFETY: Kotlin transfers a detached socket fd and relinquishes ownership.
    let socket = unsafe { OwnedFd::from_raw_fd(socket_fd) };
    let mut stream = TcpStream::from(socket);
    let prefix = if prefix_first < 0 {
        Vec::new()
    } else {
        vec![prefix_first as u8, prefix_second as u8]
    };
    match run_responder_session(&mut stream, &runtime, &prefix) {
        Ok(_) => RESULT_OK,
        Err(_) => ERROR_SECURE_SESSION,
    }
}

fn secure_state() -> JInt {
    let Ok(runtime) = secure_runtime() else {
        return ERROR_SECURE_STATE;
    };
    match runtime.snapshot().state {
        RuntimeState::Unpaired => 0,
        RuntimeState::PairingXx => 1,
        RuntimeState::SasPending => 2,
        RuntimeState::LocalConfirmed => 3,
        RuntimeState::PeerConfirmed => 4,
        RuntimeState::MutualConfirmed => 5,
        RuntimeState::TrustCommitting => 6,
        RuntimeState::CommittedWaitingPeer => 7,
        RuntimeState::Paired => 8,
        RuntimeState::Authenticated => 9,
        RuntimeState::PairRejected => 10,
        RuntimeState::PairingFailed => 11,
        RuntimeState::Cooldown => 12,
    }
}

fn secure_sas() -> JInt {
    let Ok(runtime) = secure_runtime() else {
        return ERROR_SECURE_STATE;
    };
    runtime
        .snapshot()
        .sas
        .and_then(|sas| sas.parse::<JInt>().ok())
        .unwrap_or(ERROR_SECURE_STATE)
}

fn secure_action(action: JInt) -> JInt {
    let Ok(runtime) = secure_runtime() else {
        return ERROR_SECURE_STATE;
    };
    let result = match action {
        0 => runtime.local_confirm(),
        1 => runtime.cancel(),
        2 => runtime.mismatch(),
        _ => return ERROR_SECURE_STATE,
    };
    match result {
        PairingActionResult::Accepted => RESULT_OK,
        PairingActionResult::Duplicate => RESULT_ALREADY_RUNNING,
        PairingActionResult::InvalidState => ERROR_SECURE_STATE,
    }
}

fn secure_field(field: JInt) -> JLong {
    let Ok(runtime) = secure_runtime() else {
        return ERROR_SECURE_STATE as JLong;
    };
    let snapshot = runtime.snapshot();
    match field {
        0 => i64::from(snapshot.authenticated),
        1 => snapshot.heartbeat_count as JLong,
        2 => snapshot.committed_peer_count as JLong,
        3 => snapshot.mismatch_count as JLong,
        _ => ERROR_SECURE_STATE as JLong,
    }
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

#[allow(clippy::too_many_arguments)]
fn update_health(
    available_memory_bytes: JLong,
    low_memory: JInt,
    thermal_code: JInt,
    battery_percent: JInt,
    charging: JInt,
    power_save: JInt,
    monotonic_ms: JLong,
) -> JInt {
    let (Ok(available_memory_bytes), Ok(monotonic_ms), Some(thermal)) = (
        u64::try_from(available_memory_bytes),
        u64::try_from(monotonic_ms),
        ThermalBand::from_android_code(thermal_code),
    ) else {
        return ERROR_BAD_LOCAL_SAMPLE;
    };
    let Ok(battery_percent) = u8::try_from(battery_percent) else {
        return ERROR_BAD_LOCAL_SAMPLE;
    };
    if battery_percent > 100
        || !matches!(low_memory, 0 | 1)
        || !matches!(charging, 0 | 1)
        || !matches!(power_save, 0 | 1)
    {
        return ERROR_BAD_LOCAL_SAMPLE;
    }
    let Ok(mut slot) = worker_slot().lock() else {
        return ERROR_INTERNAL;
    };
    let Some(worker) = slot.as_mut() else {
        return ERROR_NOT_RUNNING;
    };
    worker.record_local_health(HealthSample {
        available_memory_bytes,
        low_memory: low_memory == 1,
        thermal,
        battery_percent,
        charging: charging == 1,
        power_save: power_save == 1,
        monotonic_ms,
    });
    RESULT_OK
}

fn health_field(field: JInt, now_ms: JLong) -> JLong {
    let Ok(now_ms) = u64::try_from(now_ms) else {
        return ERROR_BAD_LOCAL_SAMPLE as JLong;
    };
    let Ok(slot) = worker_slot().lock() else {
        return ERROR_INTERNAL as JLong;
    };
    let Some(worker) = slot.as_ref() else {
        return ERROR_NOT_RUNNING as JLong;
    };
    let status = worker.health_status(now_ms);
    match field {
        0 => worker.health_sample_count() as JLong,
        1 => status.safety as JLong,
        2 => status.thermal as JLong,
        3 => status.battery as JLong,
        4 => status.sample_age_ms.map_or(-1, |age| age as JLong),
        5 => status.budget_bytes as JLong,
        _ => ERROR_BAD_LOCAL_SAMPLE as JLong,
    }
}

fn authority_state(field: JInt) -> JInt {
    let Ok(slot) = worker_slot().lock() else {
        return ERROR_INTERNAL;
    };
    let Some(worker) = slot.as_ref() else {
        return ERROR_NOT_RUNNING;
    };
    match field {
        0 => match worker.lease_state() {
            LeaseState::Free => 0,
            LeaseState::Active => 1,
            LeaseState::Revoking => 2,
            LeaseState::Expired => 3,
        },
        1 => match worker.resource_guard_state() {
            ResourceGuardState::Active => 1,
        },
        _ => ERROR_BAD_LOCAL_SAMPLE,
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_workerUpdateHealth(
    _env: JniEnv,
    _object: JObject,
    available_memory_bytes: JLong,
    low_memory: JInt,
    thermal_code: JInt,
    battery_percent: JInt,
    charging: JInt,
    power_save: JInt,
    monotonic_ms: JLong,
) -> JInt {
    int_boundary(|| {
        update_health(
            available_memory_bytes,
            low_memory,
            thermal_code,
            battery_percent,
            charging,
            power_save,
            monotonic_ms,
        )
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_workerHealthField(
    _env: JniEnv,
    _object: JObject,
    field: JInt,
    now_ms: JLong,
) -> JLong {
    long_boundary(|| health_field(field, now_ms))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_workerAuthorityState(
    _env: JniEnv,
    _object: JObject,
    field: JInt,
) -> JInt {
    int_boundary(|| authority_state(field))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_secureInitialize(
    _env: JniEnv,
    _object: JObject,
    directory_fd: JInt,
) -> JInt {
    int_boundary(|| initialize_secure(directory_fd))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_secureAcceptFd(
    _env: JniEnv,
    _object: JObject,
    socket_fd: JInt,
    prefix_first: JInt,
    prefix_second: JInt,
) -> JInt {
    int_boundary(|| accept_secure_fd(socket_fd, prefix_first, prefix_second))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_secureState(
    _env: JniEnv,
    _object: JObject,
) -> JInt {
    int_boundary(secure_state)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_secureSas(
    _env: JniEnv,
    _object: JObject,
) -> JInt {
    int_boundary(secure_sas)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_secureAction(
    _env: JniEnv,
    _object: JObject,
    action: JInt,
) -> JInt {
    int_boundary(|| secure_action(action))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_secureField(
    _env: JniEnv,
    _object: JObject,
    field: JInt,
) -> JLong {
    long_boundary(|| secure_field(field))
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

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("JNI test serialization lock")
    }

    fn reset() {
        let mut slot = worker_slot().lock().expect("worker test lock");
        *slot = None;
    }

    #[test]
    fn start_status_stop_and_restart_are_rust_owned() {
        let _test_guard = test_lock();
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
        let _test_guard = test_lock();
        assert_eq!(
            int_boundary(|| std::panic::panic_any("test-only boundary probe")),
            ERROR_PANIC_CONTAINED
        );
    }

    #[test]
    fn stopped_status_never_claims_pairing_or_ready() {
        let _test_guard = test_lock();
        reset();
        assert_eq!(worker_state(), STATE_STOPPED);
        assert_eq!(incarnation_word(true), ERROR_NOT_RUNNING as JLong);
    }

    #[test]
    fn local_health_adapter_rejects_malformed_and_records_valid_sample() {
        let _test_guard = test_lock();
        reset();
        assert_eq!(start_worker(), RESULT_OK);
        assert_eq!(update_health(1, 2, 0, 50, 0, 0, 1), ERROR_BAD_LOCAL_SAMPLE);
        assert_eq!(
            update_health(1_073_741_824, 0, 0, 80, 0, 0, 2_000),
            RESULT_OK
        );
        assert_eq!(health_field(0, 2_000), 1);
        assert_eq!(health_field(2, 2_000), ThermalBand::None as JLong);
        assert_eq!(
            health_field(3, 2_000),
            pb_worker_core::BatteryBand::Nominal as JLong
        );
        reset();
    }

    #[test]
    fn production_authority_is_free_but_resource_guard_actor_is_active() {
        let _test_guard = test_lock();
        reset();
        assert_eq!(start_worker(), RESULT_OK);
        assert_eq!(authority_state(0), 0);
        assert_eq!(authority_state(1), 1);
        assert_eq!(
            health_field(1, 0),
            pb_worker_core::SafetyBand::RefusedStaleState as JLong
        );
        reset();
    }
}
