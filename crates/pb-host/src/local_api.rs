use serde_json::{Value, json};
use std::time::{Duration, Instant};

use crate::{
    AuthenticatedLocalClient, FramedLocalClient, LocalFrameOutcome, LocalMethod,
    LocalValidationError, LocalValidationErrorKind, ValidatedLocalRequest,
};

const C12_API: u64 = 1;
const C12_SCOPE_REQUEST: &str = "REQUEST";
const LOCAL_BAD_REQUEST: &str = "LOCAL_BAD_REQUEST";
const LOCAL_UNSUPPORTED_METHOD: &str = "LOCAL_UNSUPPORTED_METHOD";
const MAX_RESPONSE_WITH_LF_BYTES: usize = 65_536;
const MESSAGE_MALFORMED_REQUEST: &str = "malformed local request";
const MESSAGE_UNSUPPORTED_METHOD: &str = "unsupported local method";
const MESSAGE_INVALID_STATUS_PARAMS: &str = "system.status requires empty object params";
const MESSAGE_DEFERRED_HANDLER: &str = "method unavailable in current build";

/// Explicit marker for canonical methods whose owner is not implemented in A4.
pub const A4_DEFERRED_HANDLER: &str = "A4_DEFERRED_HANDLER";

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
pub fn serve_local_client(client: AuthenticatedLocalClient) {
    serve_local_client_observed(client, |_| {});
}

fn serve_local_client_observed(
    client: AuthenticatedLocalClient,
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
            Ok(request) => (Some(request.method()), dispatch(&request, &framed)),
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

fn dispatch(request: &ValidatedLocalRequest, framed: &FramedLocalClient) -> C12Response {
    match request.method() {
        LocalMethod::SystemStatus => system_status(request, framed),
        LocalMethod::DevicesList
        | LocalMethod::DevicesGet
        | LocalMethod::JobsGet
        | LocalMethod::DiagnosticsRedacted
        | LocalMethod::EventsTail
        | LocalMethod::PairingBegin
        | LocalMethod::PairingConfirm
        | LocalMethod::PairingCancel
        | LocalMethod::PeerUnpair
        | LocalMethod::ControllerAcquire
        | LocalMethod::ControllerRelease
        | LocalMethod::ComputeSubmit
        | LocalMethod::ComputeCancel
        | LocalMethod::BenchmarkRun => a4_deferred_handler(request),
    }
}

fn system_status(request: &ValidatedLocalRequest, framed: &FramedLocalClient) -> C12Response {
    if !system_status_params_are_empty(request) {
        return C12Response::error(
            request.id().clone(),
            LOCAL_BAD_REQUEST,
            MESSAGE_INVALID_STATUS_PARAMS,
        );
    }

    let limits = framed.limits();
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
            "remote_worker_state": "NOT_CONFIGURED"
        }),
    )
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
    use crate::LocalClientLimits;
    use crate::admission::{authenticated_test_client, validation::validate_local_request};
    use std::io::{Read, Write};
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::thread;

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
    fn all_fourteen_unwired_methods_are_explicitly_deferred() {
        assert_eq!(A4_DEFERRED_HANDLER, "A4_DEFERRED_HANDLER");
        for method in LocalMethod::ALL
            .into_iter()
            .filter(|method| *method != LocalMethod::SystemStatus)
        {
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
        let worker = thread::spawn(move || serve_local_client(authenticated));

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
        let result = status["result"].as_object().expect("status result object");
        assert_eq!(result.len(), 8);
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
    fn e2e_t14_lc009_event_contains_only_method_latency_and_result_code() {
        let secret = "RAW_PARAMS_MUST_NEVER_APPEAR";
        let (mut peer, server) = UnixStream::pair().expect("create observed loop socket");
        let authenticated = authenticated_test_client(OwnedFd::from(server));
        let worker = thread::spawn(move || {
            let mut events = Vec::new();
            serve_local_client_observed(authenticated, |event| events.push(event));
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
