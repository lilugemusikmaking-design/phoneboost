use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{
    AuthenticatedLocalClient, AutoUseController, AutoUseReason, AutoUseState,
    ControllerObservabilitySnapshot, ExecutionSource, FramedLocalClient, GateObservations,
    LocalFrameOutcome, LocalMethod, LocalValidationError, LocalValidationErrorKind,
    ValidatedLocalRequest, remote_pairing_begin, remote_pairing_cancel, remote_pairing_confirm,
    remote_status,
};
use pb_runtime_secure::PairingActionResult;

const C12_API: u64 = 1;
const C12_SCOPE_REQUEST: &str = "REQUEST";
const LOCAL_BAD_REQUEST: &str = "LOCAL_BAD_REQUEST";
const LOCAL_UNSUPPORTED_METHOD: &str = "LOCAL_UNSUPPORTED_METHOD";
const MAX_RESPONSE_WITH_LF_BYTES: usize = 65_536;
const MESSAGE_MALFORMED_REQUEST: &str = "malformed local request";
const MESSAGE_UNSUPPORTED_METHOD: &str = "unsupported local method";
const MESSAGE_INVALID_STATUS_PARAMS: &str = "system.status requires empty object params";
const MESSAGE_DEFERRED_HANDLER: &str = "method unavailable in current build";
const MESSAGE_INVALID_COMPUTE_SUBMIT_PARAMS: &str = "invalid compute.submit params";

const C12_BLAKE3_OP_ID: &str = "pb.native.blake3/1";
const C12_BLAKE3_FIXTURE: &str = "c10-abc-v1";
const C12_BLAKE3_INPUT: &[u8] = b"abc";
#[cfg(test)]
const C12_BLAKE3_DIGEST_HEX: &str =
    "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85";

/// Explicit marker for canonical methods whose owner is not implemented in A4.
pub const A4_DEFERRED_HANDLER: &str = "A4_DEFERRED_HANDLER";

/// Immutable authority shared by the daemon with every authenticated C12 client.
#[derive(Clone, Default)]
pub struct LocalApiContext {
    auto_use: Option<Arc<AutoUseController>>,
}

impl LocalApiContext {
    pub const fn new(auto_use: Option<Arc<AutoUseController>>) -> Self {
        Self { auto_use }
    }

    fn auto_use_status(&self) -> (AutoUseState, AutoUseReason, bool, GateObservations) {
        let Some(controller) = self.auto_use.as_deref() else {
            return (
                AutoUseState::Off,
                AutoUseReason::Off,
                false,
                GateObservations::not_observed(),
            );
        };
        let snapshot: ControllerObservabilitySnapshot = controller.current_observability_snapshot();
        let status = snapshot.node_status();
        let available = remote_blake3_is_available(status.state(), status.reason());
        (
            status.state(),
            status.reason(),
            available,
            snapshot.gate_observations(),
        )
    }
}

const fn remote_blake3_is_available(state: AutoUseState, reason: AutoUseReason) -> bool {
    matches!(state, AutoUseState::Available) && matches!(reason, AutoUseReason::Ready)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalHandlerDomain {
    System,
    Devices,
    Jobs,
    Diagnostics,
    Events,
    Pairing,
    PeerTrust,
    Controller,
    Compute,
    Benchmark,
}

/// Minimal LC-009 seam. It cannot carry params or raw request/response bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalApiRequestEvent {
    method: Option<LocalMethod>,
    latency: Duration,
    result_code: &'static str,
}

impl LocalApiRequestEvent {
    pub const fn method(&self) -> Option<LocalMethod> {
        self.method
    }

    pub const fn latency(&self) -> Duration {
        self.latency
    }

    pub const fn result_code(&self) -> &'static str {
        self.result_code
    }
}

impl LocalMethod {
    pub const fn handler_domain(self) -> LocalHandlerDomain {
        match self {
            Self::SystemStatus => LocalHandlerDomain::System,
            Self::DevicesList | Self::DevicesGet => LocalHandlerDomain::Devices,
            Self::JobsGet => LocalHandlerDomain::Jobs,
            Self::DiagnosticsRedacted => LocalHandlerDomain::Diagnostics,
            Self::EventsTail => LocalHandlerDomain::Events,
            Self::PairingBegin | Self::PairingConfirm | Self::PairingCancel => {
                LocalHandlerDomain::Pairing
            }
            Self::PeerUnpair => LocalHandlerDomain::PeerTrust,
            Self::ControllerAcquire | Self::ControllerRelease => LocalHandlerDomain::Controller,
            Self::ComputeSubmit | Self::ComputeCancel => LocalHandlerDomain::Compute,
            Self::BenchmarkRun => LocalHandlerDomain::Benchmark,
        }
    }
}

enum C12Outcome {
    Success {
        result: Value,
    },
    Error {
        code: &'static str,
        message_safe: &'static str,
    },
}

struct C12Response {
    id: Value,
    outcome: C12Outcome,
}

impl C12Response {
    fn success(id: Value, result: Value) -> Self {
        Self {
            id,
            outcome: C12Outcome::Success { result },
        }
    }

    fn error(id: Value, code: &'static str, message_safe: &'static str) -> Self {
        Self {
            id,
            outcome: C12Outcome::Error { code, message_safe },
        }
    }

    fn result_code(&self) -> &'static str {
        match &self.outcome {
            C12Outcome::Success { .. } => "OK",
            C12Outcome::Error { code, .. } => code,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResponseSerializationError {
    Json,
    TooLarge,
}

/// Serve one authenticated connection until clean close, timeout, or transport failure.
pub fn serve_local_client(client: AuthenticatedLocalClient, context: Arc<LocalApiContext>) {
    serve_local_client_observed(client, context, |_| {});
}

fn serve_local_client_observed(
    client: AuthenticatedLocalClient,
    context: Arc<LocalApiContext>,
    mut observe: impl FnMut(LocalApiRequestEvent),
) {
    let Ok(mut framed) = client.into_framed() else {
        return;
    };

    loop {
        let line = match framed.next_line_bytes() {
            Ok(LocalFrameOutcome::Line(line)) => line,
            Ok(LocalFrameOutcome::CleanEof | LocalFrameOutcome::IdleTimeout) | Err(_) => return,
        };

        let started = Instant::now();
        let (method, response) = match framed.validate_request_line(line) {
            Ok(request) => (
                Some(request.method()),
                dispatch(&request, &framed, &context),
            ),
            Err(error) => (None, response_for_validation_error(error)),
        };
        let result_code = response.result_code();
        let Ok(serialized) = serialize_c12_response(response) else {
            return;
        };
        if framed.write_response_ndjson(&serialized).is_err() {
            return;
        }
        observe(LocalApiRequestEvent {
            method,
            latency: started.elapsed(),
            result_code,
        });
    }
}

fn dispatch(
    request: &ValidatedLocalRequest,
    framed: &FramedLocalClient,
    context: &LocalApiContext,
) -> C12Response {
    match request.method() {
        LocalMethod::SystemStatus => system_status(request, framed, context),
        LocalMethod::DevicesList
        | LocalMethod::DevicesGet
        | LocalMethod::JobsGet
        | LocalMethod::DiagnosticsRedacted
        | LocalMethod::EventsTail
        | LocalMethod::PeerUnpair
        | LocalMethod::ControllerAcquire
        | LocalMethod::ControllerRelease
        | LocalMethod::ComputeCancel
        | LocalMethod::BenchmarkRun => a4_deferred_handler(request),
        LocalMethod::ComputeSubmit => compute_submit(request, context),
        LocalMethod::PairingBegin => pairing_begin(request),
        LocalMethod::PairingConfirm => pairing_confirm(request),
        LocalMethod::PairingCancel => pairing_cancel(request),
    }
}

fn system_status(
    request: &ValidatedLocalRequest,
    framed: &FramedLocalClient,
    context: &LocalApiContext,
) -> C12Response {
    if !system_status_params_are_empty(request) {
        return C12Response::error(
            request.id().clone(),
            LOCAL_BAD_REQUEST,
            MESSAGE_INVALID_STATUS_PARAMS,
        );
    }

    let limits = framed.limits();
    let (auto_use_state, auto_use_reason, remote_blake3_available, gates) =
        context.auto_use_status();
    C12Response::success(
        request.id().clone(),
        json!({
            "runtime_state": "READY",
            "local_api_state": "ACTIVE",
            "api": C12_API,
            "local_clients_active": framed.active_clients(),
            "local_clients_max": limits.max_clients(),
            "max_line_bytes": limits.max_line_bytes(),
            "idle_timeout_seconds": limits.idle_timeout().as_secs(),
            "remote_worker_state": if remote_status().is_some_and(|status| status.authenticated) {
                "AUTHENTICATED"
            } else {
                "NOT_CONFIGURED"
            },
            "auto_use_state": auto_use_state.as_str(),
            "auto_use_reason": auto_use_reason.as_str(),
            "remote_blake3_available": remote_blake3_available,
            "discovery_observation": {
                "state": gates.discovery_observation().state(),
                "reason": gates.discovery_observation().reason()
            },
            "controller_lease": {
                "state": gates.controller_lease().state(),
                "reason": gates.controller_lease().reason()
            },
            "resource_guard_admission_proof": {
                "state": gates.resource_guard_admission_proof().state(),
                "reason": gates.resource_guard_admission_proof().reason()
            }
        }),
    )
}

fn compute_submit(request: &ValidatedLocalRequest, context: &LocalApiContext) -> C12Response {
    if !compute_submit_params_are_valid(request.params()) {
        return C12Response::error(
            request.id().clone(),
            LOCAL_BAD_REQUEST,
            MESSAGE_INVALID_COMPUTE_SUBMIT_PARAMS,
        );
    }
    let Some(controller) = context.auto_use.as_deref() else {
        return C12Response::error(
            request.id().clone(),
            LOCAL_UNSUPPORTED_METHOD,
            MESSAGE_DEFERRED_HANDLER,
        );
    };
    compute_submit_after_validation(request, |input| {
        let execution = controller.execute_blake3(input);
        (execution.digest(), execution.source(), execution.reason())
    })
}

fn compute_submit_after_validation(
    request: &ValidatedLocalRequest,
    execute: impl FnOnce(&[u8]) -> ([u8; 32], ExecutionSource, AutoUseReason),
) -> C12Response {
    let (digest, source, reason) = execute(C12_BLAKE3_INPUT);
    C12Response::success(
        request.id().clone(),
        json!({
            "state": "COMPLETED",
            "op_id": C12_BLAKE3_OP_ID,
            "fixture": C12_BLAKE3_FIXTURE,
            "input_bytes": C12_BLAKE3_INPUT.len(),
            "digest_blake3_hex": encode_lower_hex(&digest),
            "execution_source": source.as_str(),
            "auto_use_reason": reason.as_str()
        }),
    )
}

fn compute_submit_params_are_valid(params: &Value) -> bool {
    let Value::Object(params) = params else {
        return false;
    };
    params.len() == 2
        && params.get("op_id").and_then(Value::as_str) == Some(C12_BLAKE3_OP_ID)
        && params.get("fixture").and_then(Value::as_str) == Some(C12_BLAKE3_FIXTURE)
}

fn encode_lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn pairing_begin(request: &ValidatedLocalRequest) -> C12Response {
    if !system_status_params_are_empty(request) {
        return C12Response::error(
            request.id().clone(),
            LOCAL_BAD_REQUEST,
            "pairing.begin requires empty object params",
        );
    }
    match remote_pairing_begin() {
        Ok(snapshot) => C12Response::success(
            request.id().clone(),
            json!({
                "state": snapshot.state.as_str(),
                "sas": snapshot.sas,
                "authenticated": snapshot.authenticated,
            }),
        ),
        Err(error) => C12Response::error(
            request.id().clone(),
            error.reason_code(),
            "pairing begin failed",
        ),
    }
}

fn pairing_confirm(request: &ValidatedLocalRequest) -> C12Response {
    pairing_action(
        request,
        "pairing.confirm requires empty object params",
        remote_pairing_confirm,
    )
}

fn pairing_cancel(request: &ValidatedLocalRequest) -> C12Response {
    pairing_action(
        request,
        "pairing.cancel requires empty object params",
        remote_pairing_cancel,
    )
}

fn pairing_action(
    request: &ValidatedLocalRequest,
    invalid_params: &'static str,
    action: impl FnOnce() -> Result<PairingActionResult, pb_runtime_secure::RuntimeError>,
) -> C12Response {
    if !system_status_params_are_empty(request) {
        return C12Response::error(request.id().clone(), LOCAL_BAD_REQUEST, invalid_params);
    }
    match action() {
        Ok(PairingActionResult::Accepted | PairingActionResult::Duplicate) => {
            let snapshot = remote_status();
            C12Response::success(
                request.id().clone(),
                json!({
                    "state": snapshot.as_ref().map_or("UNAVAILABLE", |state| state.state.as_str()),
                    "authenticated": snapshot.is_some_and(|state| state.authenticated),
                }),
            )
        }
        Ok(PairingActionResult::InvalidState) => C12Response::error(
            request.id().clone(),
            "PAIRING_NOT_COMMITTED",
            "pairing action unavailable in current state",
        ),
        Err(error) => C12Response::error(
            request.id().clone(),
            error.reason_code(),
            "pairing action failed",
        ),
    }
}

fn system_status_params_are_empty(request: &ValidatedLocalRequest) -> bool {
    matches!(request.params(), Value::Object(params) if params.is_empty())
}

fn a4_deferred_handler(request: &ValidatedLocalRequest) -> C12Response {
    let _explicit_scaffold_marker = A4_DEFERRED_HANDLER;
    C12Response::error(
        request.id().clone(),
        LOCAL_UNSUPPORTED_METHOD,
        MESSAGE_DEFERRED_HANDLER,
    )
}

fn response_for_validation_error(error: LocalValidationError) -> C12Response {
    let id = error.correlation_id().cloned().unwrap_or(Value::Null);
    match error.kind() {
        LocalValidationErrorKind::LocalBadRequest => {
            C12Response::error(id, LOCAL_BAD_REQUEST, MESSAGE_MALFORMED_REQUEST)
        }
        LocalValidationErrorKind::LocalUnsupportedMethod => {
            C12Response::error(id, LOCAL_UNSUPPORTED_METHOD, MESSAGE_UNSUPPORTED_METHOD)
        }
    }
}

fn serialize_c12_response(response: C12Response) -> Result<Vec<u8>, ResponseSerializationError> {
    let value = match response.outcome {
        C12Outcome::Success { result } => json!({
            "api": C12_API,
            "id": response.id,
            "ok": true,
            "result": result
        }),
        C12Outcome::Error { code, message_safe } => json!({
            "api": C12_API,
            "id": response.id,
            "ok": false,
            "error": {
                "code": code,
                "scope": C12_SCOPE_REQUEST,
                "message_safe": message_safe
            }
        }),
    };
    let mut serialized =
        serde_json::to_vec(&value).map_err(|_| ResponseSerializationError::Json)?;
    if serialized.len() + 1 > MAX_RESPONSE_WITH_LF_BYTES {
        return Err(ResponseSerializationError::TooLarge);
    }
    serialized.push(b'\n');
    Ok(serialized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admission::{authenticated_test_client, validation::validate_local_request};
    use crate::{DeviceDiscovery, DiscoveryError, LocalClientLimits};
    use pb_runtime_secure::{EndpointRole, SecureRuntime, StateStore};
    use std::cell::Cell;
    use std::fs::{self, File};
    use std::io::{Read, Write};
    use std::os::fd::OwnedFd;
    use std::os::unix::fs::DirBuilderExt;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct NoDeviceDiscovery;

    impl DeviceDiscovery for NoDeviceDiscovery {
        fn discover(&self) -> Result<Option<crate::TransportCandidate>, DiscoveryError> {
            Ok(None)
        }
    }

    fn context_with_off_controller() -> (Arc<LocalApiContext>, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "phoneboost-local-api-{}-{}",
            std::process::id(),
            TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .expect("private local API test directory");
        let fd: OwnedFd = File::open(&path)
            .expect("open local API test directory")
            .into();
        let store = StateStore::from_directory_fd(fd).expect("local API test state store");
        store
            .load_or_create_identity()
            .expect("local API test identity");
        let runtime = Arc::new(
            SecureRuntime::initialize(EndpointRole::LinuxInitiator, store)
                .expect("local API test runtime"),
        );
        let controller = Arc::new(
            AutoUseController::new(runtime, Arc::new(NoDeviceDiscovery))
                .expect("local API test controller"),
        );
        (Arc::new(LocalApiContext::new(Some(controller))), path)
    }

    fn request(method: LocalMethod, params: Value) -> ValidatedLocalRequest {
        validate_local_request(
            serde_json::to_vec(&json!({
                "api": 1,
                "id": {"opaque": 7},
                "method": method.as_str(),
                "params": params
            }))
            .expect("serialize test request"),
        )
        .expect("validate test request")
    }

    fn response_value(response: C12Response) -> Value {
        let bytes = serialize_c12_response(response).expect("serialize C12 response");
        assert!(bytes.ends_with(b"\n"));
        assert!(bytes.len() <= MAX_RESPONSE_WITH_LF_BYTES);
        serde_json::from_slice(&bytes[..bytes.len() - 1]).expect("parse serialized response")
    }

    #[test]
    fn closed_domain_map_classifies_all_fifteen_methods() {
        let expected = [
            LocalHandlerDomain::System,
            LocalHandlerDomain::Devices,
            LocalHandlerDomain::Devices,
            LocalHandlerDomain::Jobs,
            LocalHandlerDomain::Diagnostics,
            LocalHandlerDomain::Events,
            LocalHandlerDomain::Pairing,
            LocalHandlerDomain::Pairing,
            LocalHandlerDomain::Pairing,
            LocalHandlerDomain::PeerTrust,
            LocalHandlerDomain::Controller,
            LocalHandlerDomain::Controller,
            LocalHandlerDomain::Compute,
            LocalHandlerDomain::Compute,
            LocalHandlerDomain::Benchmark,
        ];
        let actual: Vec<_> = LocalMethod::ALL
            .into_iter()
            .map(LocalMethod::handler_domain)
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn c12_serializer_preserves_opaque_id_and_exclusive_success_shape() {
        let value = response_value(C12Response::success(
            json!(["opaque", null]),
            json!({"runtime_state": "READY"}),
        ));
        assert_eq!(value["api"], 1);
        assert_eq!(value["id"], json!(["opaque", null]));
        assert_eq!(value["ok"], true);
        assert!(value.get("result").is_some());
        assert!(value.get("error").is_none());
    }

    #[test]
    fn c12_serializer_emits_exclusive_error_shape() {
        let value = response_value(C12Response::error(
            json!(42),
            LOCAL_BAD_REQUEST,
            MESSAGE_MALFORMED_REQUEST,
        ));
        assert_eq!(value["id"], 42);
        assert_eq!(value["ok"], false);
        assert!(value.get("result").is_none());
        assert_eq!(value["error"]["code"], LOCAL_BAD_REQUEST);
        assert_eq!(value["error"]["scope"], C12_SCOPE_REQUEST);
    }

    #[test]
    fn malformed_request_uses_recovered_id_or_null() {
        let with_id = validate_local_request(
            br#"{"api":2,"id":"recover-me","method":"system.status","params":{}}"#.to_vec(),
        )
        .expect_err("invalid api");
        let value = response_value(response_for_validation_error(with_id));
        assert_eq!(value["id"], "recover-me");

        let invalid_json = validate_local_request(b"{".to_vec()).expect_err("invalid JSON");
        let value = response_value(response_for_validation_error(invalid_json));
        assert!(value["id"].is_null());
    }

    #[test]
    fn system_status_rejects_every_params_shape_except_empty_object() {
        for params in [json!(null), json!([]), json!(""), json!({"hidden": true})] {
            let request = request(LocalMethod::SystemStatus, params);
            assert!(!system_status_params_are_empty(&request));
        }
        assert!(system_status_params_are_empty(&request(
            LocalMethod::SystemStatus,
            json!({})
        )));
    }

    #[test]
    fn all_ten_unwired_methods_are_explicitly_deferred() {
        assert_eq!(A4_DEFERRED_HANDLER, "A4_DEFERRED_HANDLER");
        for method in LocalMethod::ALL.into_iter().filter(|method| {
            !matches!(
                method,
                LocalMethod::SystemStatus
                    | LocalMethod::PairingBegin
                    | LocalMethod::PairingConfirm
                    | LocalMethod::PairingCancel
                    | LocalMethod::ComputeSubmit
            )
        }) {
            let value = response_value(a4_deferred_handler(&request(method, json!({}))));
            assert_eq!(value["error"]["code"], LOCAL_UNSUPPORTED_METHOD);
            assert_eq!(value["error"]["message_safe"], MESSAGE_DEFERRED_HANDLER);
        }
    }

    #[test]
    fn response_limit_rejects_json_plus_lf_over_64kib() {
        let oversized = "x".repeat(MAX_RESPONSE_WITH_LF_BYTES);
        assert_eq!(
            serialize_c12_response(C12Response::success(json!(1), json!(oversized))),
            Err(ResponseSerializationError::TooLarge)
        );
    }

    #[test]
    fn local_limits_used_by_status_remain_canonical() {
        let limits = LocalClientLimits::CANONICAL;
        assert_eq!(limits.max_clients(), 8);
        assert_eq!(limits.max_line_bytes(), 65_536);
        assert_eq!(limits.idle_timeout().as_secs(), 60);
    }

    #[test]
    fn locked_fixture_bytes_size_and_digest_are_exact() {
        assert_eq!(C12_BLAKE3_INPUT, b"abc");
        assert_eq!(C12_BLAKE3_INPUT.len(), 3);
        assert_eq!(
            encode_lower_hex(blake3::hash(C12_BLAKE3_INPUT).as_bytes()),
            C12_BLAKE3_DIGEST_HEX
        );
    }

    #[test]
    fn compute_submit_validation_is_closed_and_rejects_every_shape_drift() {
        assert!(compute_submit_params_are_valid(&json!({
            "op_id": C12_BLAKE3_OP_ID,
            "fixture": C12_BLAKE3_FIXTURE
        })));
        for invalid in [
            json!(null),
            json!([]),
            json!({}),
            json!({"op_id": C12_BLAKE3_OP_ID}),
            json!({"fixture": C12_BLAKE3_FIXTURE}),
            json!({"op_id": 1, "fixture": C12_BLAKE3_FIXTURE}),
            json!({"op_id": C12_BLAKE3_OP_ID, "fixture": 1}),
            json!({"op_id": "pb.native.blake3/2", "fixture": C12_BLAKE3_FIXTURE}),
            json!({"op_id": C12_BLAKE3_OP_ID, "fixture": "unknown"}),
            json!({
                "op_id": C12_BLAKE3_OP_ID,
                "fixture": C12_BLAKE3_FIXTURE,
                "extra": true
            }),
        ] {
            assert!(!compute_submit_params_are_valid(&invalid), "{invalid}");
            let value = response_value(compute_submit(
                &request(LocalMethod::ComputeSubmit, invalid),
                &LocalApiContext::default(),
            ));
            assert_eq!(value["error"]["code"], LOCAL_BAD_REQUEST);
            assert_eq!(
                value["error"]["message_safe"],
                MESSAGE_INVALID_COMPUTE_SUBMIT_PARAMS
            );
        }
    }

    #[test]
    fn local_api_context_without_controller_is_closed_and_with_controller_executes_real_seam() {
        let absent = LocalApiContext::default();
        assert_eq!(
            absent.auto_use_status(),
            (
                AutoUseState::Off,
                AutoUseReason::Off,
                false,
                GateObservations::not_observed()
            )
        );
        let unavailable = response_value(compute_submit(
            &request(
                LocalMethod::ComputeSubmit,
                json!({"op_id": C12_BLAKE3_OP_ID, "fixture": C12_BLAKE3_FIXTURE}),
            ),
            &absent,
        ));
        assert_eq!(unavailable["error"]["code"], LOCAL_UNSUPPORTED_METHOD);
        assert_eq!(
            unavailable["error"]["message_safe"],
            MESSAGE_DEFERRED_HANDLER
        );

        let (present, directory) = context_with_off_controller();
        assert!(present.auto_use.is_some());
        let controller_snapshot = present
            .auto_use
            .as_deref()
            .expect("controller")
            .current_observability_snapshot();
        let (state, reason, available, gates) = present.auto_use_status();
        assert_eq!(
            (state, reason, available, gates),
            (
                controller_snapshot.node_status().state(),
                controller_snapshot.node_status().reason(),
                remote_blake3_is_available(
                    controller_snapshot.node_status().state(),
                    controller_snapshot.node_status().reason()
                ),
                controller_snapshot.gate_observations()
            )
        );
        let fallback = response_value(compute_submit(
            &request(
                LocalMethod::ComputeSubmit,
                json!({"op_id": C12_BLAKE3_OP_ID, "fixture": C12_BLAKE3_FIXTURE}),
            ),
            &present,
        ));
        assert_eq!(fallback["ok"], true);
        assert_eq!(
            fallback["result"]["execution_source"],
            "LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE"
        );
        assert_eq!(fallback["result"]["auto_use_reason"], "OFF");
        assert_eq!(
            fallback["result"]["digest_blake3_hex"],
            C12_BLAKE3_DIGEST_HEX
        );
        drop(present);
        fs::remove_dir_all(directory).expect("remove local API test directory");
    }

    #[test]
    fn remote_blake3_status_invariant_is_exact_for_all_state_reason_pairs() {
        let states = [
            AutoUseState::Off,
            AutoUseState::Discovering,
            AutoUseState::Connecting,
            AutoUseState::Authenticating,
            AutoUseState::AcquiringAuthority,
            AutoUseState::CheckingReadiness,
            AutoUseState::Available,
            AutoUseState::Degraded,
            AutoUseState::Reconnecting,
            AutoUseState::Unavailable,
        ];
        let reasons = [
            AutoUseReason::Off,
            AutoUseReason::NoDevice,
            AutoUseReason::NotPaired,
            AutoUseReason::AuthFailed,
            AutoUseReason::LeaseUnavailable,
            AutoUseReason::WorkerUnhealthy,
            AutoUseReason::ResourceRefused,
            AutoUseReason::TransportLost,
            AutoUseReason::Reconnecting,
            AutoUseReason::DiscoveryBackendUnavailable,
            AutoUseReason::Ready,
        ];
        for state in states {
            for reason in reasons {
                assert_eq!(
                    remote_blake3_is_available(state, reason),
                    state == AutoUseState::Available && reason == AutoUseReason::Ready
                );
            }
        }
    }

    #[test]
    fn compute_execution_seam_is_called_once_and_renders_all_exact_sources() {
        let request = request(
            LocalMethod::ComputeSubmit,
            json!({"op_id": C12_BLAKE3_OP_ID, "fixture": C12_BLAKE3_FIXTURE}),
        );
        let cases = [
            (
                ExecutionSource::RemoteSuccess,
                AutoUseReason::Ready,
                "REMOTE_SUCCESS",
            ),
            (
                ExecutionSource::LocalFallbackAfterRemoteUnavailable,
                AutoUseReason::TransportLost,
                "LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE",
            ),
            (
                ExecutionSource::LocalFallbackAfterAmbiguousRemote,
                AutoUseReason::ResourceRefused,
                "LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE",
            ),
        ];
        for (source, reason, expected_source) in cases {
            let calls = Cell::new(0);
            let response = compute_submit_after_validation(&request, |input| {
                calls.set(calls.get() + 1);
                assert_eq!(input, b"abc");
                (*blake3::hash(input).as_bytes(), source, reason)
            });
            assert_eq!(calls.get(), 1);
            let value = response_value(response);
            assert_eq!(value["result"]["state"], "COMPLETED");
            assert_eq!(value["result"]["op_id"], C12_BLAKE3_OP_ID);
            assert_eq!(value["result"]["fixture"], C12_BLAKE3_FIXTURE);
            assert_eq!(value["result"]["input_bytes"], 3);
            assert_eq!(value["result"]["execution_source"], expected_source);
            assert_eq!(value["result"]["auto_use_reason"], reason.as_str());
        }
    }

    fn exchange(stream: &mut UnixStream, request: &[u8]) -> Value {
        stream.write_all(request).expect("write local request");
        stream.write_all(b"\n").expect("terminate local request");
        let mut response = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            stream.read_exact(&mut byte).expect("read local response");
            if byte[0] == b'\n' {
                break;
            }
            response.push(byte[0]);
            assert!(response.len() < MAX_RESPONSE_WITH_LF_BYTES);
        }
        serde_json::from_slice(&response).expect("parse local response")
    }

    #[test]
    fn real_client_loop_handles_status_errors_and_deferred_without_disconnect() {
        let (mut peer, server) = UnixStream::pair().expect("create real local loop socket");
        let authenticated = authenticated_test_client(OwnedFd::from(server));
        let worker = thread::spawn(move || {
            serve_local_client(authenticated, Arc::new(LocalApiContext::default()))
        });

        let status = exchange(
            &mut peer,
            br#"{"api":1,"id":"status-1","method":"system.status","params":{}}"#,
        );
        assert_eq!(status["id"], "status-1");
        assert_eq!(status["ok"], true);
        assert_eq!(status["result"]["runtime_state"], "READY");
        assert_eq!(status["result"]["local_api_state"], "ACTIVE");
        assert_eq!(status["result"]["api"], 1);
        assert_eq!(status["result"]["local_clients_active"], 1);
        assert_eq!(status["result"]["local_clients_max"], 8);
        assert_eq!(status["result"]["max_line_bytes"], 65_536);
        assert_eq!(status["result"]["idle_timeout_seconds"], 60);
        assert_eq!(status["result"]["remote_worker_state"], "NOT_CONFIGURED");
        assert_eq!(status["result"]["auto_use_state"], "OFF");
        assert_eq!(status["result"]["auto_use_reason"], "OFF");
        assert_eq!(status["result"]["remote_blake3_available"], false);
        assert_eq!(
            status["result"]["discovery_observation"],
            json!({"state": "UNKNOWN", "reason": "NOT_OBSERVED"})
        );
        assert_eq!(
            status["result"]["controller_lease"],
            json!({"state": "UNKNOWN", "reason": "NOT_OBSERVED"})
        );
        assert_eq!(
            status["result"]["resource_guard_admission_proof"],
            json!({"state": "UNKNOWN", "reason": "NOT_OBSERVED"})
        );
        let result = status["result"].as_object().expect("status result object");
        assert_eq!(result.len(), 14);
        for forbidden in [
            "device",
            "phone_connected",
            "remote_ram",
            "remote_capacity",
            "remote_job",
            "transport",
            "pairing",
            "benchmark",
            "performance",
        ] {
            assert!(!result.contains_key(forbidden));
        }

        let invalid_params = exchange(
            &mut peer,
            br#"{"api":1,"id":2,"method":"system.status","params":{"hidden":true}}"#,
        );
        assert_eq!(invalid_params["id"], 2);
        assert_eq!(invalid_params["error"]["code"], LOCAL_BAD_REQUEST);

        let invalid_compute = exchange(
            &mut peer,
            br#"{"api":1,"id":"compute-invalid","method":"compute.submit","params":{"op_id":"pb.native.blake3/1","fixture":"c10-abc-v1","extra":true}}"#,
        );
        assert_eq!(invalid_compute["id"], "compute-invalid");
        assert_eq!(invalid_compute["error"]["code"], LOCAL_BAD_REQUEST);
        assert_eq!(
            invalid_compute["error"]["message_safe"],
            MESSAGE_INVALID_COMPUTE_SUBMIT_PARAMS
        );

        let deferred = exchange(
            &mut peer,
            br#"{"api":1,"id":[3],"method":"devices.list","params":{}}"#,
        );
        assert_eq!(deferred["id"], json!([3]));
        assert_eq!(deferred["error"]["code"], LOCAL_UNSUPPORTED_METHOD);
        assert_eq!(deferred["error"]["message_safe"], MESSAGE_DEFERRED_HANDLER);

        let malformed = exchange(&mut peer, b"{");
        assert!(malformed["id"].is_null());
        assert_eq!(malformed["error"]["code"], LOCAL_BAD_REQUEST);

        let unknown = exchange(
            &mut peer,
            br#"{"api":1,"id":"unknown","method":"raw.pbmux.rpc","params":{}}"#,
        );
        assert_eq!(unknown["id"], "unknown");
        assert_eq!(unknown["error"]["code"], LOCAL_UNSUPPORTED_METHOD);

        let second_status = exchange(
            &mut peer,
            br#"{"api":1,"id":"status-2","method":"system.status","params":{}}"#,
        );
        assert_eq!(second_status["id"], "status-2");
        assert_eq!(second_status["ok"], true);
        assert_eq!(second_status["result"]["runtime_state"], "READY");

        drop(peer);
        worker.join().expect("local client worker exits cleanly");
    }

    #[test]
    fn real_client_loop_uses_shared_controller_for_compute_and_remains_usable() {
        let (context, directory) = context_with_off_controller();
        let (mut peer, server) = UnixStream::pair().expect("create controller C12 socket");
        let authenticated = authenticated_test_client(OwnedFd::from(server));
        let worker_context = Arc::clone(&context);
        let worker = thread::spawn(move || serve_local_client(authenticated, worker_context));

        let compute = exchange(
            &mut peer,
            br#"{"api":1,"id":"compute","method":"compute.submit","params":{"op_id":"pb.native.blake3/1","fixture":"c10-abc-v1"}}"#,
        );
        assert_eq!(compute["ok"], true);
        assert_eq!(compute["result"]["state"], "COMPLETED");
        assert_eq!(
            compute["result"]["digest_blake3_hex"],
            C12_BLAKE3_DIGEST_HEX
        );
        assert_eq!(
            compute["result"]["execution_source"],
            "LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE"
        );
        assert_eq!(compute["result"]["auto_use_reason"], "OFF");

        let status = exchange(
            &mut peer,
            br#"{"api":1,"id":"after-compute","method":"system.status","params":{}}"#,
        );
        assert_eq!(status["ok"], true);
        assert_eq!(status["result"]["auto_use_state"], "OFF");
        assert_eq!(status["result"]["auto_use_reason"], "OFF");
        assert_eq!(status["result"]["remote_blake3_available"], false);

        drop(peer);
        worker.join().expect("controller C12 worker exits");
        drop(context);
        fs::remove_dir_all(directory).expect("remove local API test directory");
    }

    #[test]
    fn e2e_t14_lc009_event_contains_only_method_latency_and_result_code() {
        let secret = "RAW_PARAMS_MUST_NEVER_APPEAR";
        let (mut peer, server) = UnixStream::pair().expect("create observed loop socket");
        let authenticated = authenticated_test_client(OwnedFd::from(server));
        let worker = thread::spawn(move || {
            let mut events = Vec::new();
            serve_local_client_observed(
                authenticated,
                Arc::new(LocalApiContext::default()),
                |event| events.push(event),
            );
            events
        });

        let request = format!(
            r#"{{"api":1,"id":"redaction","method":"system.status","params":{{"secret":"{secret}"}}}}"#
        );
        let response = exchange(&mut peer, request.as_bytes());
        assert_eq!(response["error"]["code"], LOCAL_BAD_REQUEST);
        drop(peer);

        let events = worker.join().expect("observed worker exits");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].method(), Some(LocalMethod::SystemStatus));
        assert_eq!(events[0].result_code(), LOCAL_BAD_REQUEST);
        assert!(events[0].latency() < Duration::from_secs(60));
        let debug = format!("{:?}", events[0]);
        assert!(!debug.contains(secret));
        assert!(!debug.contains("params"));
        assert!(!debug.contains("request"));
        assert!(!debug.contains("response"));
    }
}
