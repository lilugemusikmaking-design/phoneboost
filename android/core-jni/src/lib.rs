use std::ffi::c_void;
use std::net::TcpStream;
use std::os::fd::{FromRawFd, OwnedFd};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, OnceLock};

use pb_runtime_secure::{
    AckPayload, AuthenticatedCommandHandler, AuthenticatedCommandHandlerError, CommandPayload,
    EndpointRole, PairingActionResult, RuntimeState, SecureRuntime, StateStore,
    VerifiedPeerSession, run_responder_session_with_handler,
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
    use std::sync::atomic::{AtomicU64, Ordering};

    use curve25519_dalek::montgomery::MontgomeryPoint;
    use pb_pbmux::{Frame, Header, build_command_frame, decode, encode, parse_command_ack_frame};
    use pb_secure::{NOISE_IK_NAME, PROLOGUE};
    use pb_types::{Channel, ControlType, FLAG_END, FLAG_START};
    use snow::{Builder, TransportState, params::NoiseParams};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

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
        let mut length = [0; 2];
        stream.read_exact(&mut length).unwrap();
        let mut bytes = vec![0; u16::from_be_bytes(length) as usize];
        stream.read_exact(&mut bytes).unwrap();
        bytes
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
}
