use std::collections::HashMap;
use std::ffi::c_void;
use std::net::{Shutdown, TcpStream};
use std::os::fd::{FromRawFd, OwnedFd};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, OnceLock};

use pb_runtime_secure::{
    AckPayload, AuthenticatedCommandHandler, AuthenticatedCommandHandlerError, BufferResult,
    CommandPayload, ComputeRequest, ComputeResponse, EndpointRole, PairingActionResult,
    RemoteBufferRequest, RemoteBufferResponseKind, ResourceRequest, ResourceResponseKind,
    ResourceResult, RuntimeState, SecureRuntime, StateStore, VerifiedPeerSession,
    run_responder_session_with_handler,
};
use pb_worker_core::{
    ControllerCommand, ControllerCommandError, ControllerCommandResult, ControllerFailureReason,
    HealthSample, LeaseId, LeaseState, ResourceGuardState, ThermalBand, WorkerCore, WorkerState,
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

struct SecureTransportSessions {
    accepting: bool,
    next_token: u64,
    sockets: HashMap<u64, TcpStream>,
}

fn secure_transport_sessions() -> &'static Mutex<SecureTransportSessions> {
    static SESSIONS: OnceLock<Mutex<SecureTransportSessions>> = OnceLock::new();
    SESSIONS.get_or_init(|| {
        Mutex::new(SecureTransportSessions {
            accepting: false,
            next_token: 0,
            sockets: HashMap::new(),
        })
    })
}

struct SecureTransportRegistration {
    token: u64,
}

impl Drop for SecureTransportRegistration {
    fn drop(&mut self) {
        if let Ok(mut sessions) = secure_transport_sessions().lock() {
            sessions.sockets.remove(&self.token);
        }
    }
}

fn start_secure_transport() -> JInt {
    let Ok(mut sessions) = secure_transport_sessions().lock() else {
        return ERROR_INTERNAL;
    };
    if !sessions.sockets.is_empty() {
        return ERROR_SECURE_SESSION;
    }
    sessions.accepting = true;
    RESULT_OK
}

fn stop_secure_transport() -> JInt {
    let Ok(mut sessions) = secure_transport_sessions().lock() else {
        return ERROR_INTERNAL;
    };
    sessions.accepting = false;
    for socket in sessions.sockets.values() {
        let _ = socket.shutdown(Shutdown::Both);
    }
    RESULT_OK
}

fn register_secure_transport(stream: &TcpStream) -> Result<SecureTransportRegistration, JInt> {
    let cancellation = stream.try_clone().map_err(|_| ERROR_SECURE_SESSION)?;
    let mut sessions = secure_transport_sessions()
        .lock()
        .map_err(|_| ERROR_INTERNAL)?;
    if !sessions.accepting {
        let _ = cancellation.shutdown(Shutdown::Both);
        return Err(ERROR_SECURE_SESSION);
    }
    sessions.next_token = sessions.next_token.saturating_add(1).max(1);
    let token = sessions.next_token;
    sessions.sockets.insert(token, cancellation);
    Ok(SecureTransportRegistration { token })
}

struct AndroidAuthenticatedCommandHandler;

static ANDROID_AUTHENTICATED_COMMAND_HANDLER: AndroidAuthenticatedCommandHandler =
    AndroidAuthenticatedCommandHandler;

impl AuthenticatedCommandHandler for AndroidAuthenticatedCommandHandler {
    fn handle_authenticated_command(
        &self,
        verified_session: &VerifiedPeerSession<'_>,
        _request_id: u64,
        command: CommandPayload,
    ) -> Result<AckPayload, AuthenticatedCommandHandlerError> {
        let command = controller_command(command);
        let result = with_worker(|worker| {
            worker
                .apply_controller_command(verified_session, command)
                .map_err(|error| match error {
                    ControllerCommandError::EntropyUnavailable
                    | ControllerCommandError::InternalInvariant => {
                        AuthenticatedCommandHandlerError::Failed
                    }
                })
        })?;
        Ok(controller_ack(result))
    }

    fn handle_authenticated_resource(
        &self,
        verified_session: &VerifiedPeerSession<'_>,
        request_id: u64,
        request: ResourceRequest,
    ) -> Result<(ResourceResponseKind, ResourceResult), AuthenticatedCommandHandlerError> {
        with_worker(|worker| {
            Ok(worker.apply_resource_request(verified_session, request_id, request))
        })
    }

    fn handle_authenticated_remote_buffer(
        &self,
        verified_session: &VerifiedPeerSession<'_>,
        _request_id: u64,
        request: RemoteBufferRequest,
    ) -> Result<(RemoteBufferResponseKind, BufferResult), AuthenticatedCommandHandlerError> {
        with_worker(|worker| Ok(worker.apply_remote_buffer_request(verified_session, request)))
    }

    fn handle_authenticated_compute(
        &self,
        verified_session: &VerifiedPeerSession<'_>,
        request_id: u64,
        request: ComputeRequest,
    ) -> Result<ComputeResponse, AuthenticatedCommandHandlerError> {
        with_worker(|worker| {
            Ok(worker.apply_compute_request(verified_session, request_id, request))
        })
    }

    fn authenticated_session_ended(
        &self,
        verified_session: &VerifiedPeerSession<'_>,
    ) -> Result<(), AuthenticatedCommandHandlerError> {
        with_worker(|worker| {
            worker.authenticated_session_ended(verified_session);
            Ok(())
        })
    }
}

fn with_worker<T>(
    apply: impl FnOnce(&mut WorkerCore) -> Result<T, AuthenticatedCommandHandlerError>,
) -> Result<T, AuthenticatedCommandHandlerError> {
    let mut slot = worker_slot()
        .lock()
        .map_err(|_| AuthenticatedCommandHandlerError::Failed)?;
    let worker = slot
        .as_mut()
        .ok_or(AuthenticatedCommandHandlerError::Unavailable)?;
    apply(worker)
}

fn controller_command(command: CommandPayload) -> ControllerCommand {
    match command.command_type {
        1 => ControllerCommand::Acquire,
        2 => ControllerCommand::Renew {
            lease_id: LeaseId::from_bytes(command.lease_id),
            command_seq: command.command_seq,
        },
        3 => ControllerCommand::Release {
            lease_id: LeaseId::from_bytes(command.lease_id),
            command_seq: command.command_seq,
        },
        _ => ControllerCommand::Unsupported {
            command_seq: command.command_seq,
        },
    }
}

fn controller_ack(result: ControllerCommandResult) -> AckPayload {
    match result {
        ControllerCommandResult::Completed { command_seq, lease } => {
            let (result_ref_present, lease_id, worker_incarnation, ttl_remaining_ms, next_seq) =
                lease.map_or((0, [0; 16], [0; 16], 0, 0), |lease| {
                    (
                        1,
                        lease.lease_id.into_bytes(),
                        lease.worker_incarnation_id.into_bytes(),
                        lease.ttl_remaining_ms,
                        lease.next_command_seq,
                    )
                });
            AckPayload {
                ack_state: 2,
                reason_code: 0,
                command_seq,
                expected_present: 0,
                expected: 0,
                result_ref_present,
                lease_id,
                worker_incarnation,
                ttl_remaining_ms,
                next_command_seq: next_seq,
                digest_present: 0,
                digest: [0; 32],
            }
        }
        ControllerCommandResult::Failed {
            command_seq,
            reason,
            expected_next_seq,
        } => AckPayload {
            ack_state: 3,
            reason_code: match reason {
                ControllerFailureReason::ControllerBusy => 1,
                ControllerFailureReason::StaleControllerLease => 2,
                ControllerFailureReason::OutOfOrder => 3,
                ControllerFailureReason::DuplicateResultEvicted => 4,
                ControllerFailureReason::UnsupportedMessage => 5,
            },
            command_seq,
            expected_present: u8::from(expected_next_seq.is_some()),
            expected: expected_next_seq.unwrap_or(0),
            result_ref_present: 0,
            lease_id: [0; 16],
            worker_incarnation: [0; 16],
            ttl_remaining_ms: 0,
            next_command_seq: 0,
            digest_present: 0,
            digest: [0; 32],
        },
    }
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
    let _registration = match register_secure_transport(&stream) {
        Ok(registration) => registration,
        Err(error) => return error,
    };
    let prefix = if prefix_first < 0 {
        Vec::new()
    } else {
        vec![prefix_first as u8, prefix_second as u8]
    };
    match run_responder_session_with_handler(
        &mut stream,
        &runtime,
        &prefix,
        &ANDROID_AUTHENTICATED_COMMAND_HANDLER,
    ) {
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
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_secureTransportStart(
    _env: JniEnv,
    _object: JObject,
) -> JInt {
    int_boundary(start_secure_transport)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_WorkerNative_secureTransportStop(
    _env: JniEnv,
    _object: JObject,
) -> JInt {
    int_boundary(stop_secure_transport)
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
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener};
    use std::os::unix::fs::DirBuilderExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    use curve25519_dalek::montgomery::MontgomeryPoint;
    use pb_pbmux::{
        AllocationFlags, BufferReason, BufferState, ComputeJobRequest, ComputeJobState,
        ComputeReason, ComputeRequest, ComputeResponse, ComputeSubmit, Frame, Header, MAX_PUT_BODY,
        RemoteBufferRequest, ResourceRequest, WireReservationState, WireResourceClass,
        build_command_frame, build_compute_request_frame, build_remote_buffer_request_frames,
        build_resource_request_frame, decode, encode, parse_command_ack_frame,
        parse_compute_response_frame, parse_remote_buffer_result_frame,
        parse_resource_result_frame,
    };
    use pb_runtime_secure::VerifiedSessionId;
    use pb_secure::{NOISE_IK_NAME, PROLOGUE};
    use pb_types::{Channel, ControlType, FLAG_END, FLAG_START};
    use snow::{Builder, TransportState, params::NoiseParams};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum HostDelivery<T> {
        Known(T),
        UnknownAfterDisconnect,
    }

    struct ResilienceHandler {
        session_ids: Mutex<Vec<VerifiedSessionId>>,
        fail_after_remote_request: AtomicU64,
        ended_sessions: AtomicU64,
    }

    impl ResilienceHandler {
        fn new() -> Self {
            Self {
                session_ids: Mutex::new(Vec::new()),
                fail_after_remote_request: AtomicU64::new(0),
                ended_sessions: AtomicU64::new(0),
            }
        }

        fn record(&self, verified_session: &VerifiedPeerSession<'_>) {
            let mut sessions = self.session_ids.lock().expect("session observation lock");
            let session_id = verified_session.session_id();
            if sessions.last().copied() != Some(session_id) {
                sessions.push(session_id);
            }
        }

        fn fail_after_remote_request(&self, request_id: u64) {
            assert_ne!(request_id, 0);
            self.fail_after_remote_request
                .store(request_id, Ordering::Release);
        }

        fn session_ids(&self) -> Vec<VerifiedSessionId> {
            self.session_ids
                .lock()
                .expect("session observation lock")
                .clone()
        }
    }

    impl AuthenticatedCommandHandler for ResilienceHandler {
        fn handle_authenticated_command(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            command: CommandPayload,
        ) -> Result<AckPayload, AuthenticatedCommandHandlerError> {
            self.record(verified_session);
            ANDROID_AUTHENTICATED_COMMAND_HANDLER.handle_authenticated_command(
                verified_session,
                request_id,
                command,
            )
        }

        fn handle_authenticated_resource(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            request: ResourceRequest,
        ) -> Result<(ResourceResponseKind, ResourceResult), AuthenticatedCommandHandlerError>
        {
            self.record(verified_session);
            ANDROID_AUTHENTICATED_COMMAND_HANDLER.handle_authenticated_resource(
                verified_session,
                request_id,
                request,
            )
        }

        fn handle_authenticated_remote_buffer(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            request: RemoteBufferRequest,
        ) -> Result<(RemoteBufferResponseKind, BufferResult), AuthenticatedCommandHandlerError>
        {
            self.record(verified_session);
            let result = ANDROID_AUTHENTICATED_COMMAND_HANDLER.handle_authenticated_remote_buffer(
                verified_session,
                request_id,
                request,
            )?;
            if self
                .fail_after_remote_request
                .compare_exchange(request_id, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Err(AuthenticatedCommandHandlerError::Failed);
            }
            Ok(result)
        }

        fn handle_authenticated_compute(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            request: ComputeRequest,
        ) -> Result<ComputeResponse, AuthenticatedCommandHandlerError> {
            self.record(verified_session);
            ANDROID_AUTHENTICATED_COMMAND_HANDLER.handle_authenticated_compute(
                verified_session,
                request_id,
                request,
            )
        }

        fn authenticated_session_ended(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
        ) -> Result<(), AuthenticatedCommandHandlerError> {
            self.ended_sessions.fetch_add(1, Ordering::AcqRel);
            ANDROID_AUTHENTICATED_COMMAND_HANDLER.authenticated_session_ended(verified_session)
        }
    }

    struct StopDuringComputeHandler {
        entered: AtomicBool,
        release: AtomicBool,
        observed_live_after_stop: AtomicBool,
        ended_sessions: AtomicU64,
    }

    struct StopDuringRemoteBufferHandler {
        entered: AtomicBool,
        observed_revoked: AtomicBool,
        handler_completed: AtomicBool,
        ended_sessions: AtomicU64,
    }

    impl StopDuringRemoteBufferHandler {
        fn new() -> Self {
            Self {
                entered: AtomicBool::new(false),
                observed_revoked: AtomicBool::new(false),
                handler_completed: AtomicBool::new(false),
                ended_sessions: AtomicU64::new(0),
            }
        }
    }

    impl AuthenticatedCommandHandler for StopDuringRemoteBufferHandler {
        fn handle_authenticated_command(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            command: CommandPayload,
        ) -> Result<AckPayload, AuthenticatedCommandHandlerError> {
            ANDROID_AUTHENTICATED_COMMAND_HANDLER.handle_authenticated_command(
                verified_session,
                request_id,
                command,
            )
        }

        fn handle_authenticated_resource(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            request: ResourceRequest,
        ) -> Result<(ResourceResponseKind, ResourceResult), AuthenticatedCommandHandlerError>
        {
            ANDROID_AUTHENTICATED_COMMAND_HANDLER.handle_authenticated_resource(
                verified_session,
                request_id,
                request,
            )
        }

        fn handle_authenticated_remote_buffer(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            request: RemoteBufferRequest,
        ) -> Result<(RemoteBufferResponseKind, BufferResult), AuthenticatedCommandHandlerError>
        {
            if matches!(&request, RemoteBufferRequest::Put { .. }) {
                self.entered.store(true, Ordering::Release);
                while verified_session.is_live() {
                    std::thread::yield_now();
                }
                self.observed_revoked.store(true, Ordering::Release);
            }
            let result = ANDROID_AUTHENTICATED_COMMAND_HANDLER.handle_authenticated_remote_buffer(
                verified_session,
                request_id,
                request,
            );
            if matches!(&result, Ok((_, response)) if response.completed) {
                self.handler_completed.store(true, Ordering::Release);
            }
            result
        }

        fn handle_authenticated_compute(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            request: ComputeRequest,
        ) -> Result<ComputeResponse, AuthenticatedCommandHandlerError> {
            ANDROID_AUTHENTICATED_COMMAND_HANDLER.handle_authenticated_compute(
                verified_session,
                request_id,
                request,
            )
        }

        fn authenticated_session_ended(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
        ) -> Result<(), AuthenticatedCommandHandlerError> {
            self.ended_sessions.fetch_add(1, Ordering::AcqRel);
            ANDROID_AUTHENTICATED_COMMAND_HANDLER.authenticated_session_ended(verified_session)
        }
    }

    impl StopDuringComputeHandler {
        fn new() -> Self {
            Self {
                entered: AtomicBool::new(false),
                release: AtomicBool::new(false),
                observed_live_after_stop: AtomicBool::new(true),
                ended_sessions: AtomicU64::new(0),
            }
        }
    }

    impl AuthenticatedCommandHandler for StopDuringComputeHandler {
        fn handle_authenticated_command(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            command: CommandPayload,
        ) -> Result<AckPayload, AuthenticatedCommandHandlerError> {
            ANDROID_AUTHENTICATED_COMMAND_HANDLER.handle_authenticated_command(
                verified_session,
                request_id,
                command,
            )
        }

        fn handle_authenticated_resource(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            request: ResourceRequest,
        ) -> Result<(ResourceResponseKind, ResourceResult), AuthenticatedCommandHandlerError>
        {
            ANDROID_AUTHENTICATED_COMMAND_HANDLER.handle_authenticated_resource(
                verified_session,
                request_id,
                request,
            )
        }

        fn handle_authenticated_remote_buffer(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            request: RemoteBufferRequest,
        ) -> Result<(RemoteBufferResponseKind, BufferResult), AuthenticatedCommandHandlerError>
        {
            ANDROID_AUTHENTICATED_COMMAND_HANDLER.handle_authenticated_remote_buffer(
                verified_session,
                request_id,
                request,
            )
        }

        fn handle_authenticated_compute(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            request: ComputeRequest,
        ) -> Result<ComputeResponse, AuthenticatedCommandHandlerError> {
            self.entered.store(true, Ordering::Release);
            while !self.release.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            self.observed_live_after_stop
                .store(verified_session.is_live(), Ordering::Release);
            ANDROID_AUTHENTICATED_COMMAND_HANDLER.handle_authenticated_compute(
                verified_session,
                request_id,
                request,
            )
        }

        fn authenticated_session_ended(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
        ) -> Result<(), AuthenticatedCommandHandlerError> {
            self.ended_sessions.fetch_add(1, Ordering::AcqRel);
            ANDROID_AUTHENTICATED_COMMAND_HANDLER.authenticated_session_ended(verified_session)
        }
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "phoneboost-c07-jni-{}-{}",
                std::process::id(),
                TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700).create(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_record(stream: &mut TcpStream, bytes: &[u8]) {
        stream
            .write_all(&(bytes.len() as u16).to_be_bytes())
            .unwrap();
        stream.write_all(bytes).unwrap();
        stream.flush().unwrap();
    }

    fn read_record(stream: &mut TcpStream) -> Vec<u8> {
        try_read_record(stream).unwrap()
    }

    fn try_read_record(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
        let mut length = [0; 2];
        stream.read_exact(&mut length)?;
        let mut bytes = vec![0; u16::from_be_bytes(length) as usize];
        stream.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn send_encrypted(stream: &mut TcpStream, transport: &mut TransportState, frame: &Frame) {
        let plaintext = encode(frame).unwrap();
        let mut ciphertext = vec![0; u16::MAX as usize];
        let length = transport
            .write_message(&plaintext, &mut ciphertext)
            .unwrap();
        write_record(stream, &ciphertext[..length]);
    }

    fn receive_encrypted(stream: &mut TcpStream, transport: &mut TransportState) -> Frame {
        let ciphertext = read_record(stream);
        let mut plaintext = vec![0; u16::MAX as usize];
        let length = transport.read_message(&ciphertext, &mut plaintext).unwrap();
        decode(&plaintext[..length]).unwrap()
    }

    fn try_receive_encrypted(
        stream: &mut TcpStream,
        transport: &mut TransportState,
    ) -> Result<Frame, ()> {
        let ciphertext = try_read_record(stream).map_err(|_| ())?;
        let mut plaintext = vec![0; u16::MAX as usize];
        let length = transport
            .read_message(&ciphertext, &mut plaintext)
            .map_err(|_| ())?;
        decode(&plaintext[..length]).map_err(|_| ())
    }

    fn run_production_android_command(
        runtime: Arc<SecureRuntime>,
        host_private: &[u8; 32],
        android_public: &[u8; 32],
        command: CommandPayload,
    ) -> AckPayload {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let responder_runtime = Arc::clone(&runtime);
        let responder = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            run_responder_session_with_handler(
                &mut stream,
                &responder_runtime,
                &[],
                &ANDROID_AUTHENTICATED_COMMAND_HANDLER,
            )
            .unwrap_err()
        });

        let mut stream = TcpStream::connect(endpoint).unwrap();
        let params: NoiseParams = NOISE_IK_NAME.parse().unwrap();
        let mut handshake = Builder::new(params)
            .local_private_key(host_private)
            .remote_public_key(android_public)
            .prologue(PROLOGUE)
            .build_initiator()
            .unwrap();
        let mut handshake_message = vec![0; u16::MAX as usize];
        let length = handshake
            .write_message(&[], &mut handshake_message)
            .unwrap();
        write_record(&mut stream, &handshake_message[..length]);
        let response = read_record(&mut stream);
        handshake
            .read_message(&response, &mut handshake_message)
            .unwrap();
        let mut transport = handshake.into_transport_mode().unwrap();

        let ping = Frame {
            header: Header {
                channel: Channel::Control,
                flags: FLAG_START | FLAG_END,
                message_type: ControlType::Ping as u16,
                request_id: 9,
                sequence: 0,
                fragment_index: 0,
                payload_len: 0,
                logical_message_len: 0,
            },
            payload: Vec::new(),
        };
        send_encrypted(&mut stream, &mut transport, &ping);
        let pong = receive_encrypted(&mut stream, &mut transport);
        assert_eq!(pong.header.message_type, ControlType::Pong as u16);
        assert_eq!(pong.header.request_id, 9);

        let command = build_command_frame(&command, 44, 1).unwrap();
        send_encrypted(&mut stream, &mut transport, &command);
        let ack = receive_encrypted(&mut stream, &mut transport);
        assert_eq!(ack.header.request_id, 44);
        assert_eq!(ack.payload.len(), 98);
        let ack = parse_command_ack_frame(&ack).unwrap();
        stream.shutdown(Shutdown::Both).unwrap();
        assert_eq!(
            responder.join().unwrap(),
            pb_runtime_secure::RuntimeError::SessionLost
        );
        ack
    }

    struct ProductionClient {
        stream: TcpStream,
        transport: TransportState,
        send_sequence: u64,
    }

    impl ProductionClient {
        fn command(&mut self, request_id: u64, command: CommandPayload) -> AckPayload {
            let frame = build_command_frame(&command, request_id, self.send_sequence).unwrap();
            self.send_sequence += 1;
            send_encrypted(&mut self.stream, &mut self.transport, &frame);
            parse_command_ack_frame(&receive_encrypted(&mut self.stream, &mut self.transport))
                .unwrap()
        }

        fn resource(
            &mut self,
            request_id: u64,
            request: ResourceRequest,
        ) -> pb_pbmux::ResourceResult {
            let frame =
                build_resource_request_frame(&request, request_id, self.send_sequence).unwrap();
            self.send_sequence += 1;
            send_encrypted(&mut self.stream, &mut self.transport, &frame);
            parse_resource_result_frame(&receive_encrypted(&mut self.stream, &mut self.transport))
                .unwrap()
                .1
        }

        fn remote(
            &mut self,
            request_id: u64,
            request: RemoteBufferRequest,
        ) -> pb_pbmux::BufferResult {
            let frames =
                build_remote_buffer_request_frames(&request, request_id, self.send_sequence)
                    .unwrap();
            self.send_sequence += frames.len() as u64;
            for frame in frames {
                send_encrypted(&mut self.stream, &mut self.transport, &frame);
            }
            parse_remote_buffer_result_frame(&receive_encrypted(
                &mut self.stream,
                &mut self.transport,
            ))
            .unwrap()
            .1
        }

        fn compute(&mut self, request_id: u64, request: ComputeRequest) -> ComputeResponse {
            let frame =
                build_compute_request_frame(&request, request_id, self.send_sequence).unwrap();
            self.send_sequence += 1;
            send_encrypted(&mut self.stream, &mut self.transport, &frame);
            parse_compute_response_frame(&receive_encrypted(&mut self.stream, &mut self.transport))
                .unwrap()
        }

        fn send_remote_without_waiting(&mut self, request_id: u64, request: RemoteBufferRequest) {
            let frames =
                build_remote_buffer_request_frames(&request, request_id, self.send_sequence)
                    .unwrap();
            self.send_sequence += frames.len() as u64;
            for frame in frames {
                send_encrypted(&mut self.stream, &mut self.transport, &frame);
            }
        }

        fn receive_remote_delivery(&mut self) -> HostDelivery<BufferResult> {
            let Ok(frame) = try_receive_encrypted(&mut self.stream, &mut self.transport) else {
                return HostDelivery::UnknownAfterDisconnect;
            };
            match parse_remote_buffer_result_frame(&frame) {
                Ok((_, result)) => HostDelivery::Known(result),
                Err(_) => HostDelivery::UnknownAfterDisconnect,
            }
        }

        fn send_compute_without_waiting(&mut self, request_id: u64, request: ComputeRequest) {
            let frame =
                build_compute_request_frame(&request, request_id, self.send_sequence).unwrap();
            self.send_sequence += 1;
            send_encrypted(&mut self.stream, &mut self.transport, &frame);
        }

        fn receive_compute_delivery(&mut self) -> HostDelivery<ComputeResponse> {
            let Ok(frame) = try_receive_encrypted(&mut self.stream, &mut self.transport) else {
                return HostDelivery::UnknownAfterDisconnect;
            };
            match parse_compute_response_frame(&frame) {
                Ok(result) => HostDelivery::Known(result),
                Err(_) => HostDelivery::UnknownAfterDisconnect,
            }
        }
    }

    fn connect_production_android_session(
        runtime: Arc<SecureRuntime>,
        host_private: &[u8; 32],
        android_public: &[u8; 32],
        handler: Arc<dyn AuthenticatedCommandHandler>,
    ) -> (
        ProductionClient,
        std::thread::JoinHandle<pb_runtime_secure::RuntimeError>,
    ) {
        connect_production_android_session_inner(
            runtime,
            host_private,
            android_public,
            handler,
            false,
        )
    }

    fn connect_production_android_session_inner(
        runtime: Arc<SecureRuntime>,
        host_private: &[u8; 32],
        android_public: &[u8; 32],
        handler: Arc<dyn AuthenticatedCommandHandler>,
        cancellation_registered: bool,
    ) -> (
        ProductionClient,
        std::thread::JoinHandle<pb_runtime_secure::RuntimeError>,
    ) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let responder_runtime = Arc::clone(&runtime);
        let responder = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _registration =
                cancellation_registered.then(|| register_secure_transport(&stream).unwrap());
            run_responder_session_with_handler(
                &mut stream,
                &responder_runtime,
                &[],
                handler.as_ref(),
            )
            .unwrap_err()
        });

        let mut stream = TcpStream::connect(endpoint).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let params: NoiseParams = NOISE_IK_NAME.parse().unwrap();
        let mut handshake = Builder::new(params)
            .local_private_key(host_private)
            .remote_public_key(android_public)
            .prologue(PROLOGUE)
            .build_initiator()
            .unwrap();
        let mut handshake_message = vec![0; u16::MAX as usize];
        let length = handshake
            .write_message(&[], &mut handshake_message)
            .unwrap();
        write_record(&mut stream, &handshake_message[..length]);
        let response = read_record(&mut stream);
        handshake
            .read_message(&response, &mut handshake_message)
            .unwrap();
        let mut transport = handshake.into_transport_mode().unwrap();
        let ping = Frame {
            header: Header {
                channel: Channel::Control,
                flags: FLAG_START | FLAG_END,
                message_type: ControlType::Ping as u16,
                request_id: 9,
                sequence: 0,
                fragment_index: 0,
                payload_len: 0,
                logical_message_len: 0,
            },
            payload: Vec::new(),
        };
        send_encrypted(&mut stream, &mut transport, &ping);
        let pong = receive_encrypted(&mut stream, &mut transport);
        assert_eq!(pong.header.message_type, ControlType::Pong as u16);
        assert_eq!(pong.header.request_id, 9);
        (
            ProductionClient {
                stream,
                transport,
                send_sequence: 1,
            },
            responder,
        )
    }

    fn with_production_android_session<T>(
        runtime: Arc<SecureRuntime>,
        host_private: &[u8; 32],
        android_public: &[u8; 32],
        exchange: impl FnOnce(&mut ProductionClient) -> T,
    ) -> T {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let responder_runtime = Arc::clone(&runtime);
        let responder = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            run_responder_session_with_handler(
                &mut stream,
                &responder_runtime,
                &[],
                &ANDROID_AUTHENTICATED_COMMAND_HANDLER,
            )
            .unwrap_err()
        });

        let mut stream = TcpStream::connect(endpoint).unwrap();
        let params: NoiseParams = NOISE_IK_NAME.parse().unwrap();
        let mut handshake = Builder::new(params)
            .local_private_key(host_private)
            .remote_public_key(android_public)
            .prologue(PROLOGUE)
            .build_initiator()
            .unwrap();
        let mut handshake_message = vec![0; u16::MAX as usize];
        let length = handshake
            .write_message(&[], &mut handshake_message)
            .unwrap();
        write_record(&mut stream, &handshake_message[..length]);
        let response = read_record(&mut stream);
        handshake
            .read_message(&response, &mut handshake_message)
            .unwrap();
        let mut transport = handshake.into_transport_mode().unwrap();
        let ping = Frame {
            header: Header {
                channel: Channel::Control,
                flags: FLAG_START | FLAG_END,
                message_type: ControlType::Ping as u16,
                request_id: 9,
                sequence: 0,
                fragment_index: 0,
                payload_len: 0,
                logical_message_len: 0,
            },
            payload: Vec::new(),
        };
        send_encrypted(&mut stream, &mut transport, &ping);
        let pong = receive_encrypted(&mut stream, &mut transport);
        assert_eq!(pong.header.message_type, ControlType::Pong as u16);
        let mut client = ProductionClient {
            stream,
            transport,
            send_sequence: 1,
        };
        let result = exchange(&mut client);
        client.stream.shutdown(Shutdown::Both).unwrap();
        assert_eq!(
            responder.join().unwrap(),
            pb_runtime_secure::RuntimeError::SessionLost
        );
        result
    }

    fn reserve_and_commit(
        client: &mut ProductionClient,
        next_request_id: &mut u64,
        lease_id: [u8; 16],
        incarnation: [u8; 16],
        resource_class: WireResourceClass,
        bytes: u64,
    ) -> [u8; 16] {
        let reserve = client.resource(
            *next_request_id,
            ResourceRequest::Reserve {
                lease_id,
                worker_incarnation_id: incarnation,
                resource_class,
                requested_bytes: bytes,
            },
        );
        *next_request_id += 1;
        assert_eq!(
            reserve.state,
            pb_pbmux::ResourceResultState::Completed,
            "reserve request={} class={resource_class:?} bytes={bytes} reason={:?}",
            *next_request_id - 1,
            reserve.reason
        );
        let reservation = reserve.reservation.unwrap();
        assert_eq!(reservation.resource_class, resource_class);
        assert_eq!(reservation.granted_bytes, bytes);
        let reservation_id = reservation.reservation_id;
        let commit = client.resource(
            *next_request_id,
            ResourceRequest::Commit {
                lease_id,
                worker_incarnation_id: incarnation,
                reservation_id,
            },
        );
        *next_request_id += 1;
        assert_eq!(commit.state, pb_pbmux::ResourceResultState::Completed);
        assert_eq!(
            commit.reservation.unwrap().state,
            WireReservationState::Committed
        );
        reservation_id
    }

    fn completed_digest(response: ComputeResponse) -> ([u8; 16], [u8; 32]) {
        let ComputeResponse::Result(result) = response else {
            panic!("synchronous BLAKE3 must return RESULT");
        };
        assert_eq!(result.state, ComputeJobState::Completed);
        assert_eq!(result.reason, ComputeReason::None);
        (result.job.unwrap().job_id, result.digest.unwrap())
    }

    #[derive(Clone, Copy)]
    struct LeaseContext {
        lease_id: [u8; 16],
        incarnation: [u8; 16],
        next_command_seq: u64,
    }

    fn refresh_health() {
        assert_eq!(update_health(2_147_483_648, 0, 0, 80, 0, 0, 0), RESULT_OK);
    }

    fn start_healthy_worker() {
        reset();
        assert_eq!(start_worker(), RESULT_OK);
        refresh_health();
        std::thread::sleep(std::time::Duration::from_millis(10_050));
        refresh_health();
    }

    fn acquire_or_renew(
        client: &mut ProductionClient,
        request_id: u64,
        existing: Option<LeaseContext>,
    ) -> LeaseContext {
        let command = if let Some(lease) = existing {
            CommandPayload {
                command_type: 2,
                lease_present: 1,
                lease_id: lease.lease_id,
                command_seq: lease.next_command_seq,
                trace_id: [0x65; 16],
                provider_present: 0,
                provider_id: 0,
                payload_len: 0,
            }
        } else {
            CommandPayload {
                command_type: 1,
                lease_present: 0,
                lease_id: [0; 16],
                command_seq: 0,
                trace_id: [0x65; 16],
                provider_present: 0,
                provider_id: 0,
                payload_len: 0,
            }
        };
        let ack = client.command(request_id, command);
        assert_eq!(ack.ack_state, 2);
        if let Some(old) = existing {
            assert_eq!(ack.lease_id, old.lease_id);
            assert_eq!(ack.worker_incarnation, old.incarnation);
        }
        LeaseContext {
            lease_id: ack.lease_id,
            incarnation: ack.worker_incarnation,
            next_command_seq: ack.next_command_seq,
        }
    }

    fn create_ready_buffer(
        client: &mut ProductionClient,
        next_request_id: &mut u64,
        lease: LeaseContext,
        data: &[u8],
    ) -> ([u8; 16], [u8; 16]) {
        assert!(!data.is_empty());
        refresh_health();
        let reservation_id = reserve_and_commit(
            client,
            next_request_id,
            lease.lease_id,
            lease.incarnation,
            WireResourceClass::RemoteBufferBytes,
            data.len() as u64,
        );
        let alloc = client.remote(
            *next_request_id,
            RemoteBufferRequest::Alloc {
                lease_id: lease.lease_id,
                worker_incarnation_id: lease.incarnation,
                reservation_id,
                size_bytes: data.len() as u64,
                allocation_flags: AllocationFlags::NONE,
            },
        );
        *next_request_id += 1;
        assert!(alloc.completed, "ALLOC failed: {:?}", alloc.reason);
        let buffer_id = alloc.buffer.unwrap().buffer_id;
        let put = client.remote(
            *next_request_id,
            RemoteBufferRequest::Put {
                lease_id: lease.lease_id,
                worker_incarnation_id: lease.incarnation,
                buffer_id,
                offset: 0,
                data: data.to_vec(),
            },
        );
        *next_request_id += 1;
        assert!(put.completed, "PUT failed: {:?}", put.reason);
        assert_eq!(put.buffer.unwrap().state, BufferState::Ready);
        (buffer_id, reservation_id)
    }

    fn assert_full_budget_available(
        client: &mut ProductionClient,
        next_request_id: &mut u64,
        lease: LeaseContext,
    ) {
        refresh_health();
        let reservation_id = reserve_and_commit(
            client,
            next_request_id,
            lease.lease_id,
            lease.incarnation,
            WireResourceClass::RemoteBufferBytes,
            128 * 1024 * 1024,
        );
        let release = client.resource(
            *next_request_id,
            ResourceRequest::Release {
                lease_id: lease.lease_id,
                worker_incarnation_id: lease.incarnation,
                reservation_id,
            },
        );
        *next_request_id += 1;
        assert_eq!(
            release.reservation.unwrap().state,
            WireReservationState::Released
        );
    }

    fn prove_fresh_c08_c09_c10_work(
        client: &mut ProductionClient,
        next_request_id: &mut u64,
        next_compute_request_id: &mut u64,
        lease: LeaseContext,
    ) {
        let input = b"fresh-session-work";
        let (buffer_id, _) = create_ready_buffer(client, next_request_id, lease, input);
        let scratch = reserve_and_commit(
            client,
            next_request_id,
            lease.lease_id,
            lease.incarnation,
            WireResourceClass::NativeOpScratchBytes,
            1024,
        );
        let (_, digest) = completed_digest(client.compute(
            *next_compute_request_id,
            ComputeRequest::Submit(ComputeSubmit {
                lease_id: lease.lease_id,
                worker_incarnation_id: lease.incarnation,
                reservation_id: scratch,
                provider_id: 1,
                provider_version: 1,
                input_kind: 1,
                buffer_id,
                input_offset: 0,
                input_length: input.len() as u64,
            }),
        ));
        *next_compute_request_id += 1;
        assert_eq!(digest, *blake3::hash(input).as_bytes());
        let freed = client.remote(
            *next_request_id,
            RemoteBufferRequest::Free {
                lease_id: lease.lease_id,
                worker_incarnation_id: lease.incarnation,
                buffer_id,
            },
        );
        *next_request_id += 1;
        assert!(freed.completed);
        assert_eq!(freed.buffer.unwrap().state, BufferState::Freed);
    }

    fn close_production_session(
        client: ProductionClient,
        responder: std::thread::JoinHandle<pb_runtime_secure::RuntimeError>,
    ) -> pb_runtime_secure::RuntimeError {
        let _ = client.stream.shutdown(Shutdown::Both);
        drop(client);
        responder.join().unwrap()
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FailurePhase {
        PartialPutBeforeDispatch,
        PutMutatedBeforeResponse,
        ComputeBeforeTerminalResponse,
    }

    fn fixed_failure_schedule(mut seed: u64) -> Vec<FailurePhase> {
        assert_ne!(seed, 0);
        let mut phases = vec![
            FailurePhase::PartialPutBeforeDispatch,
            FailurePhase::PartialPutBeforeDispatch,
            FailurePhase::PartialPutBeforeDispatch,
            FailurePhase::PutMutatedBeforeResponse,
            FailurePhase::PutMutatedBeforeResponse,
            FailurePhase::PutMutatedBeforeResponse,
            FailurePhase::ComputeBeforeTerminalResponse,
            FailurePhase::ComputeBeforeTerminalResponse,
            FailurePhase::ComputeBeforeTerminalResponse,
        ];
        for index in (1..phases.len()).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            phases.swap(index, seed as usize % (index + 1));
        }
        phases
    }

    fn configured_resilience_runtime() -> (TestDirectory, Arc<SecureRuntime>, [u8; 32], [u8; 32]) {
        let directory = TestDirectory::new();
        let directory_fd: OwnedFd = fs::File::open(&directory.0).unwrap().into();
        let store = StateStore::from_directory_fd(directory_fd).unwrap();
        let android_identity = store.load_or_create_identity().unwrap();
        let host_private = [0x6d; 32];
        let host_public = MontgomeryPoint::mul_base_clamped(host_private).to_bytes();
        store
            .commit_peer(&pb_runtime_secure::PeerRecord::new(host_public, "host", 1))
            .unwrap();
        let android_public = *android_identity.public();
        let runtime =
            Arc::new(SecureRuntime::initialize(EndpointRole::AndroidResponder, store).unwrap());
        (directory, runtime, host_private, android_public)
    }

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
        drop(slot);
        assert_eq!(stop_secure_transport(), RESULT_OK);
    }

    #[test]
    fn secure_transport_stop_unblocks_pre_auth_transfer_and_is_idempotent() {
        let _test_guard = test_lock();
        reset();
        assert_eq!(start_secure_transport(), RESULT_OK);
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let (registered, observed) = std::sync::mpsc::sync_channel(1);
        let session = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _registration = register_secure_transport(&stream).unwrap();
            registered.send(()).unwrap();
            let mut byte = [0_u8; 1];
            stream.read_exact(&mut byte)
        });
        let _client = TcpStream::connect(endpoint).unwrap();
        observed.recv().unwrap();
        assert_eq!(stop_secure_transport(), RESULT_OK);
        assert_eq!(stop_secure_transport(), RESULT_OK);
        assert!(session.join().unwrap().is_err());
        assert!(
            secure_transport_sessions()
                .lock()
                .unwrap()
                .sockets
                .is_empty()
        );
    }

    #[test]
    fn secure_transport_stop_revokes_authenticated_session_and_blocks_later_requests() {
        let _test_guard = test_lock();
        reset();
        start_healthy_worker();
        let (_directory, runtime, host_private, android_public) = configured_resilience_runtime();
        let handler = Arc::new(ResilienceHandler::new());
        assert_eq!(start_secure_transport(), RESULT_OK);
        let trait_handler: Arc<dyn AuthenticatedCommandHandler> = handler.clone();
        let (mut client, responder) = connect_production_android_session_inner(
            Arc::clone(&runtime),
            &host_private,
            &android_public,
            trait_handler,
            true,
        );
        let _lease = acquire_or_renew(&mut client, 91_000, None);
        assert!(runtime.snapshot().authenticated);

        assert_eq!(stop_secure_transport(), RESULT_OK);
        assert_eq!(stop_secure_transport(), RESULT_OK);
        assert_eq!(
            responder.join().unwrap(),
            pb_runtime_secure::RuntimeError::SessionLost
        );
        assert!(!runtime.snapshot().authenticated);
        assert_eq!(handler.ended_sessions.load(Ordering::Acquire), 1);
        let mut byte = [0_u8; 1];
        assert!(matches!(client.stream.read(&mut byte), Ok(0) | Err(_)));
        assert!(
            secure_transport_sessions()
                .lock()
                .unwrap()
                .sockets
                .is_empty()
        );
        assert_eq!(start_secure_transport(), RESULT_OK);
        assert_eq!(stop_secure_transport(), RESULT_OK);
        reset();
    }

    #[test]
    fn secure_transport_stop_during_c09_c10_revokes_before_blocked_compute_resumes() {
        let _test_guard = test_lock();
        start_healthy_worker();
        let (_directory, runtime, host_private, android_public) = configured_resilience_runtime();
        let handler = Arc::new(StopDuringComputeHandler::new());
        assert_eq!(start_secure_transport(), RESULT_OK);
        let trait_handler: Arc<dyn AuthenticatedCommandHandler> = handler.clone();
        let (mut client, responder) = connect_production_android_session_inner(
            Arc::clone(&runtime),
            &host_private,
            &android_public,
            trait_handler,
            true,
        );
        let lease = acquire_or_renew(&mut client, 92_000, None);
        let mut next_resource_request_id = 92_100;
        let input = b"cancel during native operation";
        let (buffer_id, _) =
            create_ready_buffer(&mut client, &mut next_resource_request_id, lease, input);
        let scratch = reserve_and_commit(
            &mut client,
            &mut next_resource_request_id,
            lease.lease_id,
            lease.incarnation,
            WireResourceClass::NativeOpScratchBytes,
            1024,
        );
        client.send_compute_without_waiting(
            92_200,
            ComputeRequest::Submit(ComputeSubmit {
                lease_id: lease.lease_id,
                worker_incarnation_id: lease.incarnation,
                reservation_id: scratch,
                provider_id: 1,
                provider_version: 1,
                input_kind: 1,
                buffer_id,
                input_offset: 0,
                input_length: input.len() as u64,
            }),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !handler.entered.load(Ordering::Acquire) {
            assert!(
                std::time::Instant::now() < deadline,
                "compute did not enter"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        assert_eq!(stop_secure_transport(), RESULT_OK);
        std::thread::sleep(std::time::Duration::from_millis(100));
        handler.release.store(true, Ordering::Release);
        assert_eq!(
            responder.join().unwrap(),
            pb_runtime_secure::RuntimeError::SessionLost
        );
        assert!(!handler.observed_live_after_stop.load(Ordering::Acquire));
        assert_eq!(handler.ended_sessions.load(Ordering::Acquire), 1);
        assert_eq!(
            client.receive_compute_delivery(),
            HostDelivery::UnknownAfterDisconnect
        );
        assert!(!runtime.snapshot().authenticated);
        assert!(
            secure_transport_sessions()
                .lock()
                .unwrap()
                .sockets
                .is_empty()
        );
        reset();
    }

    #[test]
    fn secure_transport_stop_during_active_c09_revokes_and_cleans_before_join() {
        let _test_guard = test_lock();
        start_healthy_worker();
        let (_directory, runtime, host_private, android_public) = configured_resilience_runtime();
        let handler = Arc::new(StopDuringRemoteBufferHandler::new());
        assert_eq!(start_secure_transport(), RESULT_OK);
        let trait_handler: Arc<dyn AuthenticatedCommandHandler> = handler.clone();
        let (mut client, responder) = connect_production_android_session_inner(
            Arc::clone(&runtime),
            &host_private,
            &android_public,
            trait_handler,
            true,
        );
        let lease = acquire_or_renew(&mut client, 93_000, None);
        let mut next_request_id = 93_100;
        let data = b"stop while c09 put is inside the authenticated handler";
        let reservation_id = reserve_and_commit(
            &mut client,
            &mut next_request_id,
            lease.lease_id,
            lease.incarnation,
            WireResourceClass::RemoteBufferBytes,
            data.len() as u64,
        );
        let alloc = client.remote(
            next_request_id,
            RemoteBufferRequest::Alloc {
                lease_id: lease.lease_id,
                worker_incarnation_id: lease.incarnation,
                reservation_id,
                size_bytes: data.len() as u64,
                allocation_flags: AllocationFlags::NONE,
            },
        );
        next_request_id += 1;
        assert!(alloc.completed);
        let buffer_id = alloc.buffer.expect("allocated buffer").buffer_id;
        client.send_remote_without_waiting(
            next_request_id,
            RemoteBufferRequest::Put {
                lease_id: lease.lease_id,
                worker_incarnation_id: lease.incarnation,
                buffer_id,
                offset: 0,
                data: data.to_vec(),
            },
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !handler.entered.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline, "C09 did not enter");
            std::thread::yield_now();
        }

        assert_eq!(stop_secure_transport(), RESULT_OK);
        assert_eq!(
            responder.join().unwrap(),
            pb_runtime_secure::RuntimeError::SessionLost
        );
        assert!(handler.observed_revoked.load(Ordering::Acquire));
        assert_eq!(handler.ended_sessions.load(Ordering::Acquire), 1);
        assert_eq!(
            client.receive_remote_delivery(),
            HostDelivery::UnknownAfterDisconnect
        );
        assert!(!runtime.snapshot().authenticated);
        assert!(
            secure_transport_sessions()
                .lock()
                .unwrap()
                .sockets
                .is_empty()
        );

        // The handler may finish its already-admitted local mutation after
        // revocation, but no success can cross the closed transport and the
        // session-ended hook must terminalize the buffer and release its hold.
        assert!(handler.handler_completed.load(Ordering::Acquire));
        let replacement_handler: Arc<dyn AuthenticatedCommandHandler> =
            Arc::new(AndroidAuthenticatedCommandHandler);
        let (mut replacement, replacement_responder) = connect_production_android_session(
            Arc::clone(&runtime),
            &host_private,
            &android_public,
            replacement_handler,
        );
        let replacement_lease = acquire_or_renew(&mut replacement, 93_200, Some(lease));
        assert_full_budget_available(&mut replacement, &mut next_request_id, replacement_lease);
        let _ = replacement.stream.shutdown(Shutdown::Both);
        assert_eq!(
            replacement_responder.join().unwrap(),
            pb_runtime_secure::RuntimeError::SessionLost
        );
        reset();
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

    #[test]
    fn c07_command_mapping_and_terminal_ack_profiles_are_exact() {
        let renew = controller_command(CommandPayload {
            command_type: 2,
            lease_present: 1,
            lease_id: [4; 16],
            command_seq: 9,
            trace_id: [5; 16],
            provider_present: 0,
            provider_id: 0,
            payload_len: 0,
        });
        assert_eq!(
            renew,
            ControllerCommand::Renew {
                lease_id: LeaseId::from_bytes([4; 16]),
                command_seq: 9,
            }
        );
        assert_eq!(
            controller_command(CommandPayload {
                command_type: 99,
                lease_present: 0,
                lease_id: [0; 16],
                command_seq: 17,
                trace_id: [0; 16],
                provider_present: 0,
                provider_id: 0,
                payload_len: 0,
            }),
            ControllerCommand::Unsupported { command_seq: 17 }
        );

        let incarnation = WorkerCore::cold_start().unwrap().incarnation();
        let completed = controller_ack(ControllerCommandResult::Completed {
            command_seq: 9,
            lease: Some(pb_worker_core::ControllerLeaseRef {
                lease_id: LeaseId::from_bytes([4; 16]),
                worker_incarnation_id: incarnation,
                ttl_remaining_ms: 60_000,
                next_command_seq: 10,
            }),
        });
        assert_eq!(completed.ack_state, 2);
        assert_eq!(completed.reason_code, 0);
        assert_eq!(completed.command_seq, 9);
        assert_eq!(completed.expected_present, 0);
        assert_eq!(completed.result_ref_present, 1);
        assert_eq!(completed.lease_id, [4; 16]);
        assert_eq!(completed.worker_incarnation, incarnation.into_bytes());
        assert_eq!(completed.ttl_remaining_ms, 60_000);
        assert_eq!(completed.next_command_seq, 10);
        assert_eq!(completed.digest_present, 0);
        assert_eq!(completed.digest, [0; 32]);

        let release = controller_ack(ControllerCommandResult::Completed {
            command_seq: 10,
            lease: None,
        });
        assert_eq!(release.ack_state, 2);
        assert_eq!(release.result_ref_present, 0);
        assert_eq!(release.lease_id, [0; 16]);
        assert_eq!(release.worker_incarnation, [0; 16]);
        assert_eq!(release.ttl_remaining_ms, 0);
        assert_eq!(release.next_command_seq, 0);

        let out_of_order = controller_ack(ControllerCommandResult::Failed {
            command_seq: 15,
            reason: ControllerFailureReason::OutOfOrder,
            expected_next_seq: Some(10),
        });
        assert_eq!(out_of_order.ack_state, 3);
        assert_eq!(out_of_order.reason_code, 3);
        assert_eq!(out_of_order.expected_present, 1);
        assert_eq!(out_of_order.expected, 10);
        assert_eq!(out_of_order.result_ref_present, 0);
    }

    #[test]
    fn worker_unavailable_is_explicit_before_command_result_or_ack() {
        let _test_guard = test_lock();
        reset();
        assert_eq!(
            with_worker::<()>(|_| panic!("unavailable worker closure must not run")),
            Err(AuthenticatedCommandHandlerError::Unavailable)
        );
    }

    #[test]
    fn production_c05_to_c07_acquire_reaches_worker_and_resumes_after_reconnect() {
        let _test_guard = test_lock();
        reset();
        assert_eq!(start_worker(), RESULT_OK);

        let directory = TestDirectory::new();
        let directory_fd: OwnedFd = fs::File::open(&directory.0).unwrap().into();
        let store = StateStore::from_directory_fd(directory_fd).unwrap();
        let android_identity = store.load_or_create_identity().unwrap();
        let host_private = [0x39; 32];
        let host_public = MontgomeryPoint::mul_base_clamped(host_private).to_bytes();
        store
            .commit_peer(&pb_runtime_secure::PeerRecord::new(host_public, "host", 1))
            .unwrap();
        let runtime =
            Arc::new(SecureRuntime::initialize(EndpointRole::AndroidResponder, store).unwrap());
        let command = CommandPayload {
            command_type: 1,
            lease_present: 0,
            lease_id: [0; 16],
            command_seq: 0,
            trace_id: [7; 16],
            provider_present: 0,
            provider_id: 0,
            payload_len: 0,
        };

        let first = run_production_android_command(
            Arc::clone(&runtime),
            &host_private,
            android_identity.public(),
            command,
        );
        assert_eq!(first.ack_state, 2);
        assert_eq!(first.reason_code, 0);
        assert_eq!(first.result_ref_present, 1);
        assert_ne!(first.lease_id, [0; 16]);
        assert_eq!(first.ttl_remaining_ms, 60_000);
        assert_eq!(first.next_command_seq, 0);

        let reconnected = run_production_android_command(
            runtime,
            &host_private,
            android_identity.public(),
            command,
        );
        assert_eq!(reconnected.ack_state, 2);
        assert_eq!(reconnected.lease_id, first.lease_id);
        assert_eq!(reconnected.worker_incarnation, first.worker_incarnation);
        assert!(reconnected.ttl_remaining_ms <= first.ttl_remaining_ms);
        assert_eq!(reconnected.next_command_seq, 0);
        assert_eq!(authority_state(0), 1);
        reset();
    }

    #[test]
    fn e_gen_03_authenticated_c08_c09_end_to_end_and_loss_restart_fail_closed() {
        let _test_guard = test_lock();
        reset();
        assert_eq!(start_worker(), RESULT_OK);
        assert_eq!(update_health(2_147_483_648, 0, 0, 80, 0, 0, 0), RESULT_OK);
        std::thread::sleep(std::time::Duration::from_millis(10_050));
        assert_eq!(
            update_health(2_147_483_648, 0, 0, 80, 0, 0, 10_050),
            RESULT_OK
        );
        let directory = TestDirectory::new();
        let directory_fd: OwnedFd = fs::File::open(&directory.0).unwrap().into();
        let store = StateStore::from_directory_fd(directory_fd).unwrap();
        let android_identity = store.load_or_create_identity().unwrap();
        let host_private = [0x4a; 32];
        let host_public = MontgomeryPoint::mul_base_clamped(host_private).to_bytes();
        store
            .commit_peer(&pb_runtime_secure::PeerRecord::new(host_public, "host", 1))
            .unwrap();
        let runtime =
            Arc::new(SecureRuntime::initialize(EndpointRole::AndroidResponder, store).unwrap());

        let (lease_id, incarnation, lost_buffer_id) = with_production_android_session(
            Arc::clone(&runtime),
            &host_private,
            android_identity.public(),
            |client| {
                let lease = client.command(
                    100,
                    CommandPayload {
                        command_type: 1,
                        lease_present: 0,
                        lease_id: [0; 16],
                        command_seq: 0,
                        trace_id: [1; 16],
                        provider_present: 0,
                        provider_id: 0,
                        payload_len: 0,
                    },
                );
                assert_eq!(lease.ack_state, 2);
                let reserve = client.resource(
                    101,
                    ResourceRequest::Reserve {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.worker_incarnation,
                        resource_class: WireResourceClass::RemoteBufferBytes,
                        requested_bytes: 8,
                    },
                );
                assert!(reserve.state == pb_pbmux::ResourceResultState::Completed);
                let reservation_id = reserve.reservation.unwrap().reservation_id;
                let replay = client.resource(
                    101,
                    ResourceRequest::Reserve {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.worker_incarnation,
                        resource_class: WireResourceClass::RemoteBufferBytes,
                        requested_bytes: 8,
                    },
                );
                assert_eq!(replay.reservation.unwrap().reservation_id, reservation_id);
                let conflict = client.resource(
                    101,
                    ResourceRequest::Reserve {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.worker_incarnation,
                        resource_class: WireResourceClass::RemoteBufferBytes,
                        requested_bytes: 4,
                    },
                );
                assert_eq!(conflict.reason, pb_pbmux::ResourceReason::RequestIdConflict);
                let commit = client.resource(
                    102,
                    ResourceRequest::Commit {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.worker_incarnation,
                        reservation_id,
                    },
                );
                assert_eq!(
                    commit.reservation.unwrap().state,
                    WireReservationState::Committed
                );
                let alloc = client.remote(
                    103,
                    RemoteBufferRequest::Alloc {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.worker_incarnation,
                        reservation_id,
                        size_bytes: 8,
                        allocation_flags: AllocationFlags::EVICTABLE,
                    },
                );
                let buffer_id = alloc.buffer.unwrap().buffer_id;
                let put = client.remote(
                    104,
                    RemoteBufferRequest::Put {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.worker_incarnation,
                        buffer_id,
                        offset: 0,
                        data: b"PB-C0909".to_vec(),
                    },
                );
                assert_eq!(put.buffer.unwrap().state, BufferState::Ready);
                let get = client.remote(
                    105,
                    RemoteBufferRequest::Get {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.worker_incarnation,
                        buffer_id,
                        offset: 0,
                        length: 8,
                    },
                );
                assert_eq!(get.data, b"PB-C0909");
                let stat = client.remote(
                    106,
                    RemoteBufferRequest::Stat {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.worker_incarnation,
                        buffer_id,
                    },
                );
                assert!(stat.completed);
                assert!(stat.data.is_empty());
                let touch = client.remote(
                    107,
                    RemoteBufferRequest::Touch {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.worker_incarnation,
                        buffer_id,
                    },
                );
                assert!(touch.completed);
                let free = client.remote(
                    108,
                    RemoteBufferRequest::Free {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.worker_incarnation,
                        buffer_id,
                    },
                );
                assert_eq!(free.buffer.unwrap().state, BufferState::Freed);

                let releasable = client.resource(
                    109,
                    ResourceRequest::Reserve {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.worker_incarnation,
                        resource_class: WireResourceClass::RemoteBufferBytes,
                        requested_bytes: 1,
                    },
                );
                let release = client.resource(
                    110,
                    ResourceRequest::Release {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.worker_incarnation,
                        reservation_id: releasable.reservation.unwrap().reservation_id,
                    },
                );
                assert_eq!(
                    release.reservation.unwrap().state,
                    WireReservationState::Released
                );
                let reserve = client.resource(
                    111,
                    ResourceRequest::Reserve {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.worker_incarnation,
                        resource_class: WireResourceClass::RemoteBufferBytes,
                        requested_bytes: 4,
                    },
                );
                let reservation_id = reserve.reservation.unwrap().reservation_id;
                assert!(
                    client
                        .resource(
                            112,
                            ResourceRequest::Commit {
                                lease_id: lease.lease_id,
                                worker_incarnation_id: lease.worker_incarnation,
                                reservation_id,
                            },
                        )
                        .state
                        == pb_pbmux::ResourceResultState::Completed
                );
                let lost = client.remote(
                    113,
                    RemoteBufferRequest::Alloc {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.worker_incarnation,
                        reservation_id,
                        size_bytes: 4,
                        allocation_flags: AllocationFlags::NONE,
                    },
                );
                (
                    lease.lease_id,
                    lease.worker_incarnation,
                    lost.buffer.unwrap().buffer_id,
                )
            },
        );

        with_production_android_session(
            Arc::clone(&runtime),
            &host_private,
            android_identity.public(),
            |client| {
                let lost = client.remote(
                    200,
                    RemoteBufferRequest::Stat {
                        lease_id,
                        worker_incarnation_id: incarnation,
                        buffer_id: lost_buffer_id,
                    },
                );
                assert_eq!(lost.reason, BufferReason::BufferLost);
                assert_eq!(lost.buffer.unwrap().state, BufferState::Lost);
            },
        );

        assert_eq!(stop_worker(), RESULT_OK);
        assert_eq!(start_worker(), RESULT_OK);
        with_production_android_session(
            runtime,
            &host_private,
            android_identity.public(),
            |client| {
                let old = client.remote(
                    300,
                    RemoteBufferRequest::Stat {
                        lease_id,
                        worker_incarnation_id: incarnation,
                        buffer_id: lost_buffer_id,
                    },
                );
                assert_eq!(old.reason, BufferReason::BufferWrongIncarnation);
                assert!(old.buffer.is_none());
            },
        );
        reset();
    }

    #[test]
    fn e_gen_04_authenticated_remote_blake3_64_mib_repeats_and_failures() {
        const INPUT_BYTES: usize = 64 * 1024 * 1024;
        const SCRATCH_BYTES: u64 = 1024 * 1024;

        let _test_guard = test_lock();
        reset();
        assert_eq!(start_worker(), RESULT_OK);
        assert_eq!(update_health(2_147_483_648, 0, 0, 80, 0, 0, 0), RESULT_OK);
        std::thread::sleep(std::time::Duration::from_millis(10_050));
        assert_eq!(
            update_health(2_147_483_648, 0, 0, 80, 0, 0, 10_050),
            RESULT_OK
        );
        let directory = TestDirectory::new();
        let directory_fd: OwnedFd = fs::File::open(&directory.0).unwrap().into();
        let store = StateStore::from_directory_fd(directory_fd).unwrap();
        let android_identity = store.load_or_create_identity().unwrap();
        let host_private = [0x5b; 32];
        let host_public = MontgomeryPoint::mul_base_clamped(host_private).to_bytes();
        store
            .commit_peer(&pb_runtime_secure::PeerRecord::new(host_public, "host", 1))
            .unwrap();
        let runtime =
            Arc::new(SecureRuntime::initialize(EndpointRole::AndroidResponder, store).unwrap());
        let input: Vec<u8> = (0..INPUT_BYTES)
            .map(|index| (index as u8).wrapping_mul(31).wrapping_add(7))
            .collect();
        let oracle = *blake3::hash(&input).as_bytes();
        assert_eq!(update_health(2_147_483_648, 0, 0, 80, 0, 0, 0), RESULT_OK);
        let oracle_hex: String = oracle.iter().map(|byte| format!("{byte:02x}")).collect();
        eprintln!("E-GEN-04 oracle ready digest={oracle_hex}");

        let (lease_id, incarnation, buffer_id, first_job_id, single_elapsed, repeated_elapsed) =
            with_production_android_session(
                Arc::clone(&runtime),
                &host_private,
                android_identity.public(),
                |client| {
                    let lease = client.command(
                        1_000,
                        CommandPayload {
                            command_type: 1,
                            lease_present: 0,
                            lease_id: [0; 16],
                            command_seq: 0,
                            trace_id: [4; 16],
                            provider_present: 0,
                            provider_id: 0,
                            payload_len: 0,
                        },
                    );
                    assert_eq!(lease.ack_state, 2);
                    let ComputeResponse::Status(wrong_incarnation) = client.compute(
                        4_900,
                        ComputeRequest::Status(ComputeJobRequest {
                            lease_id: lease.lease_id,
                            worker_incarnation_id: [0x55; 16],
                            job_id: [0x66; 16],
                        }),
                    ) else {
                        panic!("wrong-incarnation lookup must be STATUS failure");
                    };
                    assert_eq!(
                        wrong_incarnation.reason,
                        ComputeReason::WrongWorkerIncarnation
                    );
                    let ComputeResponse::Status(stale_lease) = client.compute(
                        4_901,
                        ComputeRequest::Status(ComputeJobRequest {
                            lease_id: [0x77; 16],
                            worker_incarnation_id: lease.worker_incarnation,
                            job_id: [0x66; 16],
                        }),
                    ) else {
                        panic!("wrong-lease lookup must be STATUS failure");
                    };
                    assert_eq!(stale_lease.reason, ComputeReason::StaleControllerLease);
                    let mut resource_request_id = 1_001;
                    let storage_reservation = reserve_and_commit(
                        client,
                        &mut resource_request_id,
                        lease.lease_id,
                        lease.worker_incarnation,
                        WireResourceClass::RemoteBufferBytes,
                        INPUT_BYTES as u64,
                    );
                    let alloc = client.remote(
                        resource_request_id,
                        RemoteBufferRequest::Alloc {
                            lease_id: lease.lease_id,
                            worker_incarnation_id: lease.worker_incarnation,
                            reservation_id: storage_reservation,
                            size_bytes: INPUT_BYTES as u64,
                            allocation_flags: AllocationFlags::EVICTABLE,
                        },
                    );
                    resource_request_id += 1;
                    let buffer_id = alloc.buffer.unwrap().buffer_id;
                    eprintln!("E-GEN-04 RemoteBuffer allocated");
                    let mut renewal_sequence = 0_u64;
                    for (index, chunk) in input.chunks(MAX_PUT_BODY).enumerate() {
                        let put = client.remote(
                            resource_request_id,
                            RemoteBufferRequest::Put {
                                lease_id: lease.lease_id,
                                worker_incarnation_id: lease.worker_incarnation,
                                buffer_id,
                                offset: (index * MAX_PUT_BODY) as u64,
                                data: chunk.to_vec(),
                            },
                        );
                        resource_request_id += 1;
                        assert!(
                            put.completed,
                            "PUT chunk {} failed: {:?}",
                            index + 1,
                            put.reason
                        );
                        eprintln!("E-GEN-04 uploaded chunk {}", index + 1);
                        assert_eq!(update_health(2_147_483_648, 0, 0, 80, 0, 0, 0), RESULT_OK);
                        if (index + 1) % 4 == 0 {
                            let renewed = client.command(
                                4_000 + renewal_sequence,
                                CommandPayload {
                                    command_type: 2,
                                    lease_present: 1,
                                    lease_id: lease.lease_id,
                                    command_seq: renewal_sequence,
                                    trace_id: [5; 16],
                                    provider_present: 0,
                                    provider_id: 0,
                                    payload_len: 0,
                                },
                            );
                            assert_eq!(renewed.ack_state, 2);
                            renewal_sequence += 1;
                        }
                    }
                    let stat = client.remote(
                        resource_request_id,
                        RemoteBufferRequest::Stat {
                            lease_id: lease.lease_id,
                            worker_incarnation_id: lease.worker_incarnation,
                            buffer_id,
                        },
                    );
                    resource_request_id += 1;
                    assert_eq!(stat.buffer.unwrap().state, BufferState::Ready);
                    eprintln!("E-GEN-04 RemoteBuffer READY");

                    let not_ready_storage = reserve_and_commit(
                        client,
                        &mut resource_request_id,
                        lease.lease_id,
                        lease.worker_incarnation,
                        WireResourceClass::RemoteBufferBytes,
                        1,
                    );
                    let not_ready = client.remote(
                        resource_request_id,
                        RemoteBufferRequest::Alloc {
                            lease_id: lease.lease_id,
                            worker_incarnation_id: lease.worker_incarnation,
                            reservation_id: not_ready_storage,
                            size_bytes: 1,
                            allocation_flags: AllocationFlags::NONE,
                        },
                    );
                    resource_request_id += 1;
                    let not_ready_id = not_ready.buffer.unwrap().buffer_id;
                    let first_scratch = reserve_and_commit(
                        client,
                        &mut resource_request_id,
                        lease.lease_id,
                        lease.worker_incarnation,
                        WireResourceClass::NativeOpScratchBytes,
                        SCRATCH_BYTES,
                    );
                    let not_ready_failure = client.compute(
                        5_000,
                        ComputeRequest::Submit(ComputeSubmit {
                            lease_id: lease.lease_id,
                            worker_incarnation_id: lease.worker_incarnation,
                            reservation_id: first_scratch,
                            provider_id: 1,
                            provider_version: 1,
                            input_kind: 1,
                            buffer_id: not_ready_id,
                            input_offset: 0,
                            input_length: 1,
                        }),
                    );
                    let ComputeResponse::Result(not_ready_failure) = not_ready_failure else {
                        panic!("not-READY refusal must be RESULT");
                    };
                    assert_eq!(not_ready_failure.reason, ComputeReason::BufferInvalidState);
                    assert!(not_ready_failure.job.is_none());
                    let ComputeResponse::Result(too_large) = client.compute(
                        5_002,
                        ComputeRequest::Submit(ComputeSubmit {
                            lease_id: lease.lease_id,
                            worker_incarnation_id: lease.worker_incarnation,
                            reservation_id: first_scratch,
                            provider_id: 1,
                            provider_version: 1,
                            input_kind: 1,
                            buffer_id,
                            input_offset: 0,
                            input_length: 128 * 1024 * 1024 + 1,
                        }),
                    ) else {
                        panic!(">128 MiB refusal must be RESULT");
                    };
                    assert_eq!(too_large.reason, ComputeReason::InputTooLarge);
                    let freed_not_ready = client.remote(
                        resource_request_id,
                        RemoteBufferRequest::Free {
                            lease_id: lease.lease_id,
                            worker_incarnation_id: lease.worker_incarnation,
                            buffer_id: not_ready_id,
                        },
                    );
                    resource_request_id += 1;
                    assert_eq!(freed_not_ready.buffer.unwrap().state, BufferState::Freed);

                    let freed_storage = reserve_and_commit(
                        client,
                        &mut resource_request_id,
                        lease.lease_id,
                        lease.worker_incarnation,
                        WireResourceClass::RemoteBufferBytes,
                        1,
                    );
                    let freed = client.remote(
                        resource_request_id,
                        RemoteBufferRequest::Alloc {
                            lease_id: lease.lease_id,
                            worker_incarnation_id: lease.worker_incarnation,
                            reservation_id: freed_storage,
                            size_bytes: 1,
                            allocation_flags: AllocationFlags::NONE,
                        },
                    );
                    resource_request_id += 1;
                    let freed_id = freed.buffer.unwrap().buffer_id;
                    assert!(
                        client
                            .remote(
                                resource_request_id,
                                RemoteBufferRequest::Put {
                                    lease_id: lease.lease_id,
                                    worker_incarnation_id: lease.worker_incarnation,
                                    buffer_id: freed_id,
                                    offset: 0,
                                    data: vec![9],
                                },
                            )
                            .completed
                    );
                    resource_request_id += 1;
                    assert!(
                        client
                            .remote(
                                resource_request_id,
                                RemoteBufferRequest::Free {
                                    lease_id: lease.lease_id,
                                    worker_incarnation_id: lease.worker_incarnation,
                                    buffer_id: freed_id,
                                },
                            )
                            .completed
                    );
                    resource_request_id += 1;
                    let freed_scratch = reserve_and_commit(
                        client,
                        &mut resource_request_id,
                        lease.lease_id,
                        lease.worker_incarnation,
                        WireResourceClass::NativeOpScratchBytes,
                        SCRATCH_BYTES,
                    );
                    let ComputeResponse::Result(freed_failure) = client.compute(
                        5_001,
                        ComputeRequest::Submit(ComputeSubmit {
                            lease_id: lease.lease_id,
                            worker_incarnation_id: lease.worker_incarnation,
                            reservation_id: freed_scratch,
                            provider_id: 1,
                            provider_version: 1,
                            input_kind: 1,
                            buffer_id: freed_id,
                            input_offset: 0,
                            input_length: 1,
                        }),
                    ) else {
                        panic!("FREED refusal must be RESULT");
                    };
                    assert_eq!(freed_failure.reason, ComputeReason::BufferFreed);
                    let ComputeResponse::Result(unsupported) = client.compute(
                        5_003,
                        ComputeRequest::Submit(ComputeSubmit {
                            lease_id: lease.lease_id,
                            worker_incarnation_id: lease.worker_incarnation,
                            reservation_id: freed_scratch,
                            provider_id: 2,
                            provider_version: 1,
                            input_kind: 1,
                            buffer_id,
                            input_offset: 0,
                            input_length: INPUT_BYTES as u64,
                        }),
                    ) else {
                        panic!("unsupported provider must be RESULT");
                    };
                    assert_eq!(unsupported.reason, ComputeReason::UnsupportedProvider);
                    assert_eq!(
                        client
                            .resource(
                                resource_request_id,
                                ResourceRequest::Release {
                                    lease_id: lease.lease_id,
                                    worker_incarnation_id: lease.worker_incarnation,
                                    reservation_id: freed_scratch,
                                },
                            )
                            .reservation
                            .unwrap()
                            .state,
                        WireReservationState::Released
                    );
                    resource_request_id += 1;

                    let submit = |reservation_id| {
                        ComputeRequest::Submit(ComputeSubmit {
                            lease_id: lease.lease_id,
                            worker_incarnation_id: lease.worker_incarnation,
                            reservation_id,
                            provider_id: 1,
                            provider_version: 1,
                            input_kind: 1,
                            buffer_id,
                            input_offset: 0,
                            input_length: INPUT_BYTES as u64,
                        })
                    };
                    let first_started = std::time::Instant::now();
                    let first_response = client.compute(5_100, submit(first_scratch));
                    let single_elapsed = first_started.elapsed();
                    let (first_job_id, first_digest) = completed_digest(first_response);
                    assert_eq!(first_digest, oracle);
                    eprintln!("E-GEN-04 first compute complete");
                    assert_eq!(client.compute(5_100, submit(first_scratch)), first_response);
                    let ComputeResponse::Result(conflict) = client.compute(
                        5_100,
                        ComputeRequest::Submit(ComputeSubmit {
                            input_length: INPUT_BYTES as u64 - 1,
                            ..match submit(first_scratch) {
                                ComputeRequest::Submit(submit) => submit,
                                _ => unreachable!(),
                            }
                        }),
                    ) else {
                        panic!("conflict must be RESULT");
                    };
                    assert_eq!(conflict.reason, ComputeReason::RequestIdConflict);
                    let ComputeResponse::Result(reuse) =
                        client.compute(5_101, submit(first_scratch))
                    else {
                        panic!("reservation reuse must be RESULT");
                    };
                    assert_eq!(reuse.reason, ComputeReason::ReservationInvalid);
                    assert!(reuse.job.is_none());

                    let terminal_status = client.compute(
                        5_102,
                        ComputeRequest::Status(ComputeJobRequest {
                            lease_id: lease.lease_id,
                            worker_incarnation_id: lease.worker_incarnation,
                            job_id: first_job_id,
                        }),
                    );
                    assert_eq!(terminal_status, first_response);
                    let ComputeResponse::Cancel(cancel) = client.compute(
                        5_103,
                        ComputeRequest::Cancel(ComputeJobRequest {
                            lease_id: lease.lease_id,
                            worker_incarnation_id: lease.worker_incarnation,
                            job_id: first_job_id,
                        }),
                    ) else {
                        panic!("terminal cancellation must use CANCEL response");
                    };
                    assert_eq!(cancel.state, ComputeJobState::Completed);
                    assert_eq!(cancel.reason, ComputeReason::JobNotCancellable);
                    assert_eq!(update_health(2_147_483_648, 0, 0, 80, 0, 0, 0), RESULT_OK);

                    let repeated_started = std::time::Instant::now();
                    for index in 1..10_u64 {
                        let scratch = reserve_and_commit(
                            client,
                            &mut resource_request_id,
                            lease.lease_id,
                            lease.worker_incarnation,
                            WireResourceClass::NativeOpScratchBytes,
                            SCRATCH_BYTES,
                        );
                        let (_, digest) =
                            completed_digest(client.compute(5_100 + index + 10, submit(scratch)));
                        assert_eq!(digest, oracle, "repeat {index}");
                        eprintln!("E-GEN-04 compute {} complete", index + 1);
                        assert_eq!(update_health(2_147_483_648, 0, 0, 80, 0, 0, 0), RESULT_OK);
                    }
                    let repeated_elapsed = repeated_started.elapsed();

                    let post_hash = client.remote(
                        resource_request_id,
                        RemoteBufferRequest::Get {
                            lease_id: lease.lease_id,
                            worker_incarnation_id: lease.worker_incarnation,
                            buffer_id,
                            offset: 0,
                            length: 1024,
                        },
                    );
                    resource_request_id += 1;
                    assert_eq!(post_hash.data, input[..1024]);

                    let leak_probe = reserve_and_commit(
                        client,
                        &mut resource_request_id,
                        lease.lease_id,
                        lease.worker_incarnation,
                        WireResourceClass::RemoteBufferBytes,
                        INPUT_BYTES as u64,
                    );
                    assert_eq!(
                        client
                            .resource(
                                resource_request_id,
                                ResourceRequest::Release {
                                    lease_id: lease.lease_id,
                                    worker_incarnation_id: lease.worker_incarnation,
                                    reservation_id: leak_probe,
                                },
                            )
                            .reservation
                            .unwrap()
                            .state,
                        WireReservationState::Released,
                        "ten compute jobs leave no scratch accounting leak"
                    );

                    (
                        lease.lease_id,
                        lease.worker_incarnation,
                        buffer_id,
                        first_job_id,
                        single_elapsed,
                        repeated_elapsed,
                    )
                },
            );

        let mib = INPUT_BYTES as f64 / (1024.0 * 1024.0);
        let ten_job_elapsed = single_elapsed + repeated_elapsed;
        eprintln!(
            "E-GEN-04 64 MiB single={:.3}s throughput={:.1} MiB/s; ten-total={:.3}s per-job={:.3}s throughput={:.1} MiB/s",
            single_elapsed.as_secs_f64(),
            mib / single_elapsed.as_secs_f64(),
            ten_job_elapsed.as_secs_f64(),
            ten_job_elapsed.as_secs_f64() / 10.0,
            mib * 10.0 / ten_job_elapsed.as_secs_f64(),
        );

        with_production_android_session(
            runtime,
            &host_private,
            android_identity.public(),
            |client| {
                let mut request_id = 8_000;
                let scratch = reserve_and_commit(
                    client,
                    &mut request_id,
                    lease_id,
                    incarnation,
                    WireResourceClass::NativeOpScratchBytes,
                    SCRATCH_BYTES,
                );
                let ComputeResponse::Result(lost) = client.compute(
                    8_100,
                    ComputeRequest::Submit(ComputeSubmit {
                        lease_id,
                        worker_incarnation_id: incarnation,
                        reservation_id: scratch,
                        provider_id: 1,
                        provider_version: 1,
                        input_kind: 1,
                        buffer_id,
                        input_offset: 0,
                        input_length: INPUT_BYTES as u64,
                    }),
                ) else {
                    panic!("LOST input must be RESULT");
                };
                assert_eq!(lost.reason, ComputeReason::BufferLost);
                assert_eq!(
                    client
                        .resource(
                            request_id,
                            ResourceRequest::Release {
                                lease_id,
                                worker_incarnation_id: incarnation,
                                reservation_id: scratch,
                            },
                        )
                        .reservation
                        .unwrap()
                        .state,
                    WireReservationState::Released,
                    "failed LOST admission leaves scratch committed"
                );
                let ComputeResponse::Status(old_job) = client.compute(
                    8_101,
                    ComputeRequest::Status(ComputeJobRequest {
                        lease_id,
                        worker_incarnation_id: incarnation,
                        job_id: first_job_id,
                    }),
                ) else {
                    panic!("fresh-session lookup must use STATUS failure");
                };
                assert_eq!(old_job.reason, ComputeReason::JobNotOwned);
                assert!(old_job.job.is_none());
            },
        );
        reset();
    }

    #[test]
    fn e_gen_05_authenticated_randomized_disconnects_and_restart_never_false_succeed() {
        const SEED: u64 = 0xe5_2026_0828_5eed;

        let _test_guard = test_lock();
        start_healthy_worker();
        let (_directory, runtime, host_private, android_public) = configured_resilience_runtime();
        let handler = Arc::new(ResilienceHandler::new());
        let mut lease = None;
        let mut next_resource_request_id = 20_000_u64;
        let mut next_compute_request_id = 30_000_u64;
        let mut next_command_request_id = 40_000_u64;

        let schedule = fixed_failure_schedule(SEED);
        assert_eq!(schedule.len(), 9);
        for phase in [
            FailurePhase::PartialPutBeforeDispatch,
            FailurePhase::PutMutatedBeforeResponse,
            FailurePhase::ComputeBeforeTerminalResponse,
        ] {
            assert_eq!(
                schedule
                    .iter()
                    .filter(|candidate| **candidate == phase)
                    .count(),
                3
            );
        }

        for (run, phase) in schedule.into_iter().enumerate() {
            eprintln!("E-GEN-05 seed={SEED:#x} run={} phase={phase:?}", run + 1);
            refresh_health();
            let trait_handler: Arc<dyn AuthenticatedCommandHandler> = handler.clone();
            let (mut client, responder) = connect_production_android_session(
                Arc::clone(&runtime),
                &host_private,
                &android_public,
                trait_handler,
            );
            let active = acquire_or_renew(&mut client, next_command_request_id, lease);
            next_command_request_id += 1;

            let buffer_id;
            let mut compute_replay = None;
            let expected_session_error;
            match phase {
                FailurePhase::PartialPutBeforeDispatch => {
                    let bytes = vec![0x31; 128 * 1024];
                    let reservation_id = reserve_and_commit(
                        &mut client,
                        &mut next_resource_request_id,
                        active.lease_id,
                        active.incarnation,
                        WireResourceClass::RemoteBufferBytes,
                        bytes.len() as u64,
                    );
                    let alloc = client.remote(
                        next_resource_request_id,
                        RemoteBufferRequest::Alloc {
                            lease_id: active.lease_id,
                            worker_incarnation_id: active.incarnation,
                            reservation_id,
                            size_bytes: bytes.len() as u64,
                            allocation_flags: AllocationFlags::NONE,
                        },
                    );
                    next_resource_request_id += 1;
                    buffer_id = alloc.buffer.unwrap().buffer_id;
                    let request_id = next_resource_request_id;
                    next_resource_request_id += 1;
                    let frames = build_remote_buffer_request_frames(
                        &RemoteBufferRequest::Put {
                            lease_id: active.lease_id,
                            worker_incarnation_id: active.incarnation,
                            buffer_id,
                            offset: 0,
                            data: bytes,
                        },
                        request_id,
                        client.send_sequence,
                    )
                    .unwrap();
                    assert!(frames.len() > 1);
                    client.send_sequence += frames.len() as u64;
                    send_encrypted(&mut client.stream, &mut client.transport, &frames[0]);
                    client.stream.shutdown(Shutdown::Both).unwrap();
                    assert_eq!(
                        client.receive_remote_delivery(),
                        HostDelivery::UnknownAfterDisconnect
                    );
                    expected_session_error = pb_runtime_secure::RuntimeError::SessionLost;
                }
                FailurePhase::PutMutatedBeforeResponse => {
                    let reservation_id = reserve_and_commit(
                        &mut client,
                        &mut next_resource_request_id,
                        active.lease_id,
                        active.incarnation,
                        WireResourceClass::RemoteBufferBytes,
                        32,
                    );
                    let alloc = client.remote(
                        next_resource_request_id,
                        RemoteBufferRequest::Alloc {
                            lease_id: active.lease_id,
                            worker_incarnation_id: active.incarnation,
                            reservation_id,
                            size_bytes: 32,
                            allocation_flags: AllocationFlags::NONE,
                        },
                    );
                    next_resource_request_id += 1;
                    buffer_id = alloc.buffer.unwrap().buffer_id;
                    let request_id = next_resource_request_id;
                    next_resource_request_id += 1;
                    handler.fail_after_remote_request(request_id);
                    client.send_remote_without_waiting(
                        request_id,
                        RemoteBufferRequest::Put {
                            lease_id: active.lease_id,
                            worker_incarnation_id: active.incarnation,
                            buffer_id,
                            offset: 0,
                            data: vec![0x52; 32],
                        },
                    );
                    assert_eq!(
                        client.receive_remote_delivery(),
                        HostDelivery::UnknownAfterDisconnect
                    );
                    expected_session_error = pb_runtime_secure::RuntimeError::CommandHandlerFailed;
                }
                FailurePhase::ComputeBeforeTerminalResponse => {
                    let input = vec![0xa7; MAX_PUT_BODY];
                    (buffer_id, _) = create_ready_buffer(
                        &mut client,
                        &mut next_resource_request_id,
                        active,
                        &input,
                    );
                    refresh_health();
                    let scratch = reserve_and_commit(
                        &mut client,
                        &mut next_resource_request_id,
                        active.lease_id,
                        active.incarnation,
                        WireResourceClass::NativeOpScratchBytes,
                        1024 * 1024,
                    );
                    let submit_request_id = next_compute_request_id;
                    next_compute_request_id += 1;
                    let submit = ComputeSubmit {
                        lease_id: active.lease_id,
                        worker_incarnation_id: active.incarnation,
                        reservation_id: scratch,
                        provider_id: 1,
                        provider_version: 1,
                        input_kind: 1,
                        buffer_id,
                        input_offset: 0,
                        input_length: input.len() as u64,
                    };
                    client.send_compute_without_waiting(
                        submit_request_id,
                        ComputeRequest::Submit(submit),
                    );
                    client.stream.shutdown(Shutdown::Both).unwrap();
                    assert_eq!(
                        client.receive_compute_delivery(),
                        HostDelivery::UnknownAfterDisconnect
                    );
                    compute_replay = Some((submit_request_id, submit));
                    expected_session_error = pb_runtime_secure::RuntimeError::SessionLost;
                }
            }
            drop(client);
            assert_eq!(responder.join().unwrap(), expected_session_error);
            assert!(!runtime.snapshot().authenticated);

            refresh_health();
            let trait_handler: Arc<dyn AuthenticatedCommandHandler> = handler.clone();
            let (mut recovery, recovery_responder) = connect_production_android_session(
                Arc::clone(&runtime),
                &host_private,
                &android_public,
                trait_handler,
            );
            let recovered = acquire_or_renew(&mut recovery, next_command_request_id, Some(active));
            next_command_request_id += 1;
            assert_eq!(recovered.lease_id, active.lease_id);
            assert_eq!(recovered.incarnation, active.incarnation);
            lease = Some(recovered);

            let lost = recovery.remote(
                next_resource_request_id,
                RemoteBufferRequest::Stat {
                    lease_id: recovered.lease_id,
                    worker_incarnation_id: recovered.incarnation,
                    buffer_id,
                },
            );
            next_resource_request_id += 1;
            assert_eq!(lost.reason, BufferReason::BufferLost);
            assert_eq!(lost.buffer.unwrap().state, BufferState::Lost);
            let stale_get = recovery.remote(
                next_resource_request_id,
                RemoteBufferRequest::Get {
                    lease_id: recovered.lease_id,
                    worker_incarnation_id: recovered.incarnation,
                    buffer_id,
                    offset: 0,
                    length: 1,
                },
            );
            next_resource_request_id += 1;
            assert!(!stale_get.completed);
            assert_eq!(stale_get.reason, BufferReason::BufferLost);
            assert!(stale_get.data.is_empty());

            if let Some((request_id, submit)) = compute_replay {
                let ComputeResponse::Result(replay) =
                    recovery.compute(request_id, ComputeRequest::Submit(submit))
                else {
                    panic!("dead-session compute replay must be RESULT refusal");
                };
                assert_eq!(replay.state, ComputeJobState::Invalid);
                assert_eq!(replay.reason, ComputeReason::BufferLost);
                assert!(replay.job.is_none());
                assert!(replay.digest.is_none());
            }

            assert_full_budget_available(&mut recovery, &mut next_resource_request_id, recovered);
            prove_fresh_c08_c09_c10_work(
                &mut recovery,
                &mut next_resource_request_id,
                &mut next_compute_request_id,
                recovered,
            );
            assert_eq!(
                close_production_session(recovery, recovery_responder),
                pb_runtime_secure::RuntimeError::SessionLost
            );
        }

        refresh_health();
        let trait_handler: Arc<dyn AuthenticatedCommandHandler> = handler.clone();
        let (mut before_restart, before_restart_responder) = connect_production_android_session(
            Arc::clone(&runtime),
            &host_private,
            &android_public,
            trait_handler,
        );
        let old_lease = acquire_or_renew(&mut before_restart, next_command_request_id, lease);
        next_command_request_id += 1;
        let restart_input = b"restart-bound";
        let (old_buffer_id, old_storage_reservation) = create_ready_buffer(
            &mut before_restart,
            &mut next_resource_request_id,
            old_lease,
            restart_input,
        );
        let old_scratch = reserve_and_commit(
            &mut before_restart,
            &mut next_resource_request_id,
            old_lease.lease_id,
            old_lease.incarnation,
            WireResourceClass::NativeOpScratchBytes,
            1024,
        );
        let (old_job_id, _) = completed_digest(before_restart.compute(
            next_compute_request_id,
            ComputeRequest::Submit(ComputeSubmit {
                lease_id: old_lease.lease_id,
                worker_incarnation_id: old_lease.incarnation,
                reservation_id: old_scratch,
                provider_id: 1,
                provider_version: 1,
                input_kind: 1,
                buffer_id: old_buffer_id,
                input_offset: 0,
                input_length: restart_input.len() as u64,
            }),
        ));
        next_compute_request_id += 1;
        assert_eq!(
            close_production_session(before_restart, before_restart_responder),
            pb_runtime_secure::RuntimeError::SessionLost
        );
        assert_eq!(stop_worker(), RESULT_OK);
        start_healthy_worker();
        let trait_handler: Arc<dyn AuthenticatedCommandHandler> = handler.clone();
        let (mut after_restart, after_restart_responder) = connect_production_android_session(
            Arc::clone(&runtime),
            &host_private,
            &android_public,
            trait_handler,
        );
        let stale_resource = after_restart.resource(
            next_resource_request_id,
            ResourceRequest::Release {
                lease_id: old_lease.lease_id,
                worker_incarnation_id: old_lease.incarnation,
                reservation_id: old_storage_reservation,
            },
        );
        next_resource_request_id += 1;
        assert_eq!(
            stale_resource.reason,
            pb_pbmux::ResourceReason::StaleControllerLease
        );
        let stale_buffer = after_restart.remote(
            next_resource_request_id,
            RemoteBufferRequest::Stat {
                lease_id: old_lease.lease_id,
                worker_incarnation_id: old_lease.incarnation,
                buffer_id: old_buffer_id,
            },
        );
        next_resource_request_id += 1;
        assert_eq!(stale_buffer.reason, BufferReason::BufferWrongIncarnation);
        let ComputeResponse::Status(stale_job) = after_restart.compute(
            next_compute_request_id,
            ComputeRequest::Status(ComputeJobRequest {
                lease_id: old_lease.lease_id,
                worker_incarnation_id: old_lease.incarnation,
                job_id: old_job_id,
            }),
        ) else {
            panic!("old-incarnation job lookup must be STATUS refusal");
        };
        next_compute_request_id += 1;
        assert_eq!(stale_job.reason, ComputeReason::WrongWorkerIncarnation);
        let new_lease = acquire_or_renew(&mut after_restart, next_command_request_id, None);
        assert_ne!(new_lease.incarnation, old_lease.incarnation);
        assert_ne!(new_lease.lease_id, old_lease.lease_id);
        prove_fresh_c08_c09_c10_work(
            &mut after_restart,
            &mut next_resource_request_id,
            &mut next_compute_request_id,
            new_lease,
        );
        assert_eq!(
            close_production_session(after_restart, after_restart_responder),
            pb_runtime_secure::RuntimeError::SessionLost
        );
        reset();
    }

    #[test]
    fn e_gen_06_authenticated_heartbeat_reconnects_fresh_without_object_resurrection() {
        let _test_guard = test_lock();
        start_healthy_worker();
        let (_directory, runtime, host_private, android_public) = configured_resilience_runtime();
        let handler = Arc::new(ResilienceHandler::new());
        let mut next_resource_request_id = 60_000_u64;
        let mut next_compute_request_id = 70_000_u64;

        let trait_handler: Arc<dyn AuthenticatedCommandHandler> = handler.clone();
        let (mut first, first_responder) = connect_production_android_session(
            Arc::clone(&runtime),
            &host_private,
            &android_public,
            trait_handler,
        );
        let first_heartbeat_count = runtime.snapshot().heartbeat_count;
        assert!(first_heartbeat_count > 0);
        let lease = acquire_or_renew(&mut first, 80_000, None);
        let input = b"session-one";
        let (old_buffer_id, _) =
            create_ready_buffer(&mut first, &mut next_resource_request_id, lease, input);
        let scratch = reserve_and_commit(
            &mut first,
            &mut next_resource_request_id,
            lease.lease_id,
            lease.incarnation,
            WireResourceClass::NativeOpScratchBytes,
            1024,
        );
        let (old_job_id, digest) = completed_digest(first.compute(
            next_compute_request_id,
            ComputeRequest::Submit(ComputeSubmit {
                lease_id: lease.lease_id,
                worker_incarnation_id: lease.incarnation,
                reservation_id: scratch,
                provider_id: 1,
                provider_version: 1,
                input_kind: 1,
                buffer_id: old_buffer_id,
                input_offset: 0,
                input_length: input.len() as u64,
            }),
        ));
        next_compute_request_id += 1;
        assert_eq!(digest, *blake3::hash(input).as_bytes());
        first.stream.shutdown(Shutdown::Both).unwrap();
        drop(first);
        assert_eq!(
            first_responder.join().unwrap(),
            pb_runtime_secure::RuntimeError::SessionLost
        );
        assert!(!runtime.snapshot().authenticated);
        assert_eq!(runtime.snapshot().state, RuntimeState::Paired);

        refresh_health();
        let trait_handler: Arc<dyn AuthenticatedCommandHandler> = handler.clone();
        let (mut second, second_responder) = connect_production_android_session(
            Arc::clone(&runtime),
            &host_private,
            &android_public,
            trait_handler,
        );
        let resumed = acquire_or_renew(&mut second, 80_001, Some(lease));
        assert_eq!(resumed.lease_id, lease.lease_id);
        assert_eq!(resumed.incarnation, lease.incarnation);
        assert!(runtime.snapshot().heartbeat_count > first_heartbeat_count);
        let sessions = handler.session_ids();
        assert!(sessions.len() >= 2);
        assert!(sessions[sessions.len() - 2] != sessions[sessions.len() - 1]);

        let old_buffer = second.remote(
            next_resource_request_id,
            RemoteBufferRequest::Stat {
                lease_id: resumed.lease_id,
                worker_incarnation_id: resumed.incarnation,
                buffer_id: old_buffer_id,
            },
        );
        next_resource_request_id += 1;
        assert_eq!(old_buffer.reason, BufferReason::BufferLost);
        assert_eq!(old_buffer.buffer.unwrap().state, BufferState::Lost);
        let ComputeResponse::Status(old_job) = second.compute(
            next_compute_request_id,
            ComputeRequest::Status(ComputeJobRequest {
                lease_id: resumed.lease_id,
                worker_incarnation_id: resumed.incarnation,
                job_id: old_job_id,
            }),
        ) else {
            panic!("dead-session job lookup must be STATUS refusal");
        };
        next_compute_request_id += 1;
        assert_eq!(old_job.reason, ComputeReason::JobNotOwned);
        assert!(old_job.job.is_none());
        assert_full_budget_available(&mut second, &mut next_resource_request_id, resumed);
        prove_fresh_c08_c09_c10_work(
            &mut second,
            &mut next_resource_request_id,
            &mut next_compute_request_id,
            resumed,
        );
        assert_eq!(
            close_production_session(second, second_responder),
            pb_runtime_secure::RuntimeError::SessionLost
        );
        reset();
    }
}
