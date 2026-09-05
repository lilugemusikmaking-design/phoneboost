use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

const MAX_NDJSON_LINE_BYTES: usize = 65_536;
const IO_TIMEOUT: Duration = Duration::from_secs(60);
static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const BLAKE3_OP_ID: &str = "pb.native.blake3/1";
const BLAKE3_FIXTURE: &str = "c10-abc-v1";
const BLAKE3_INPUT_BYTES: u64 = 3;
const BLAKE3_DIGEST_HEX: &str = "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85";
const REMOTE_SUCCESS: &str = "REMOTE_SUCCESS";
const LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE: &str = "LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE";
const LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE: &str = "LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE";

#[derive(Debug, Eq, PartialEq)]
pub enum CliError {
    RuntimeUnavailable,
    ConnectFailed,
    IoFailed,
    ResponseTooLarge,
    MalformedResponse,
    Remote { code: String, message_safe: String },
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeUnavailable => formatter.write_str("PhoneBoost runtime unavailable"),
            Self::ConnectFailed => formatter.write_str("unable to connect to phoneboostd"),
            Self::IoFailed => formatter.write_str("local API I/O failed"),
            Self::ResponseTooLarge => formatter.write_str("local API response exceeds 64 KiB"),
            Self::MalformedResponse => formatter.write_str("malformed local API response"),
            Self::Remote { code, message_safe } => write!(formatter, "{code}: {message_safe}"),
        }
    }
}

impl std::error::Error for CliError {}

#[derive(Debug, Eq, PartialEq)]
pub struct StatusView {
    runtime_state: String,
    local_api_state: String,
    remote_worker_state: String,
    auto_use_state: String,
    auto_use_reason: String,
    remote_blake3_available: bool,
    discovery_observation: GateView,
    controller_lease: GateView,
    resource_guard_admission_proof: GateView,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct GateView {
    state: String,
    reason: String,
}

impl GateView {
    fn not_exposed() -> Self {
        Self {
            state: "UNKNOWN".to_owned(),
            reason: "NOT_EXPOSED_BY_C12".to_owned(),
        }
    }

    pub fn state(&self) -> &str {
        &self.state
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ComputeView {
    digest_blake3_hex: String,
    execution_source: String,
    auto_use_reason: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct PairingView {
    state: String,
    sas: Option<String>,
    authenticated: bool,
}

impl fmt::Display for PairingView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Pairing state: {}", self.state)?;
        if let Some(sas) = &self.sas {
            writeln!(formatter, "Pairing code: {sas}")?;
        }
        write!(
            formatter,
            "Secure session: {}",
            if self.authenticated {
                "AUTHENTICATED"
            } else {
                "NOT_AUTHENTICATED"
            }
        )
    }
}

impl fmt::Display for StatusView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "PhoneBoost: {}", self.runtime_state)?;
        writeln!(formatter, "Local API: {}", self.local_api_state)?;
        writeln!(formatter, "Android worker: {}", self.remote_worker_state)?;
        writeln!(formatter, "Auto-use: {}", self.auto_use_state)?;
        writeln!(formatter, "Auto-use reason: {}", self.auto_use_reason)?;
        writeln!(
            formatter,
            "Discovery observation: {} ({})",
            self.discovery_observation.state, self.discovery_observation.reason
        )?;
        writeln!(
            formatter,
            "Controller lease: {} ({})",
            self.controller_lease.state, self.controller_lease.reason
        )?;
        writeln!(
            formatter,
            "Latest admission/readiness proof: {} ({})",
            self.resource_guard_admission_proof.state, self.resource_guard_admission_proof.reason
        )?;
        write!(
            formatter,
            "Remote BLAKE3: {}",
            if self.remote_blake3_available {
                "AVAILABLE"
            } else {
                "UNAVAILABLE"
            }
        )
    }
}

impl ComputeView {
    pub fn exit_code(&self) -> i32 {
        if self.execution_source == REMOTE_SUCCESS {
            0
        } else {
            3
        }
    }

    pub fn digest_blake3_hex(&self) -> &str {
        &self.digest_blake3_hex
    }

    pub fn execution_source(&self) -> &str {
        &self.execution_source
    }

    pub fn auto_use_reason(&self) -> &str {
        &self.auto_use_reason
    }
}

impl StatusView {
    pub fn runtime_state(&self) -> &str {
        &self.runtime_state
    }

    pub fn local_api_state(&self) -> &str {
        &self.local_api_state
    }

    pub fn remote_worker_state(&self) -> &str {
        &self.remote_worker_state
    }

    pub fn auto_use_state(&self) -> &str {
        &self.auto_use_state
    }

    pub fn auto_use_reason(&self) -> &str {
        &self.auto_use_reason
    }

    pub const fn remote_blake3_available(&self) -> bool {
        self.remote_blake3_available
    }

    pub fn discovery_observation(&self) -> &GateView {
        &self.discovery_observation
    }

    pub fn controller_lease(&self) -> &GateView {
        &self.controller_lease
    }

    pub fn resource_guard_admission_proof(&self) -> &GateView {
        &self.resource_guard_admission_proof
    }
}

impl fmt::Display for ComputeView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "BLAKE3 fixture: {BLAKE3_FIXTURE}")?;
        writeln!(formatter, "Input bytes: {BLAKE3_INPUT_BYTES}")?;
        writeln!(formatter, "BLAKE3 digest: {}", self.digest_blake3_hex)?;
        writeln!(formatter, "Execution source: {}", self.execution_source)?;
        write!(formatter, "Auto-use reason: {}", self.auto_use_reason)
    }
}

pub fn status() -> Result<StatusView, CliError> {
    status_with_timeout(IO_TIMEOUT)
}

pub fn status_with_timeout(timeout: Duration) -> Result<StatusView, CliError> {
    let mut stream = connect_local_api(timeout)?;
    status_over_stream(&mut stream, next_request_id())
}

pub fn pair() -> Result<PairingView, CliError> {
    pairing_request("pairing.begin")
}

pub fn pair_confirm() -> Result<PairingView, CliError> {
    pairing_request("pairing.confirm")
}

pub fn pair_cancel() -> Result<PairingView, CliError> {
    pairing_request("pairing.cancel")
}

pub fn compute_blake3() -> Result<ComputeView, CliError> {
    compute_blake3_with_timeout(IO_TIMEOUT)
}

pub fn compute_blake3_with_timeout(timeout: Duration) -> Result<ComputeView, CliError> {
    let mut stream = connect_local_api(timeout)?;
    compute_blake3_over_stream(&mut stream, next_request_id())
}

fn connect_local_api(timeout: Duration) -> Result<UnixStream, CliError> {
    let socket_path = canonical_control_socket(env::var_os("XDG_RUNTIME_DIR"))?;
    let stream = UnixStream::connect(socket_path).map_err(|_| CliError::ConnectFailed)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|_| CliError::IoFailed)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|_| CliError::IoFailed)?;
    Ok(stream)
}

fn pairing_request(method: &'static str) -> Result<PairingView, CliError> {
    let mut stream = connect_local_api(IO_TIMEOUT)?;
    let request_id = next_request_id();
    let response = request_value(&mut stream, request_id, method, json!({}))?;
    let result = response.as_object().ok_or(CliError::MalformedResponse)?;
    let state = required_string(result, "state")?;
    let authenticated = result
        .get("authenticated")
        .and_then(Value::as_bool)
        .ok_or(CliError::MalformedResponse)?;
    let sas = match result.get("sas") {
        None | Some(Value::Null) => None,
        Some(Value::String(value))
            if value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            Some(value.clone())
        }
        _ => return Err(CliError::MalformedResponse),
    };
    if method == "pairing.begin" && state == "SAS_PENDING" && sas.is_none() {
        return Err(CliError::MalformedResponse);
    }
    Ok(PairingView {
        state,
        sas,
        authenticated,
    })
}

fn canonical_control_socket(runtime: Option<OsString>) -> Result<PathBuf, CliError> {
    let runtime = runtime.ok_or(CliError::RuntimeUnavailable)?;
    if runtime.is_empty() {
        return Err(CliError::RuntimeUnavailable);
    }
    let runtime = PathBuf::from(runtime);
    if !runtime.is_absolute() {
        return Err(CliError::RuntimeUnavailable);
    }
    Ok(runtime.join("phoneboost").join("control.sock"))
}

fn next_request_id() -> Value {
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    Value::String(format!("phoneboostctl-{}-{sequence}", std::process::id()))
}

fn status_over_stream(stream: &mut UnixStream, request_id: Value) -> Result<StatusView, CliError> {
    let response = request_value(stream, request_id, "system.status", json!({}))?;
    let result = response.as_object().ok_or(CliError::MalformedResponse)?;
    let auto_use_state = required_string(result, "auto_use_state")?;
    let auto_use_reason = required_string(result, "auto_use_reason")?;
    if !valid_auto_use_state(&auto_use_state) || !valid_auto_use_reason(&auto_use_reason) {
        return Err(CliError::MalformedResponse);
    }
    let remote_blake3_available = result
        .get("remote_blake3_available")
        .and_then(Value::as_bool)
        .ok_or(CliError::MalformedResponse)?;
    if remote_blake3_available != (auto_use_state == "AVAILABLE" && auto_use_reason == "READY") {
        return Err(CliError::MalformedResponse);
    }
    Ok(StatusView {
        runtime_state: required_string(result, "runtime_state")?,
        local_api_state: required_string(result, "local_api_state")?,
        remote_worker_state: required_string(result, "remote_worker_state")?,
        auto_use_state,
        auto_use_reason,
        remote_blake3_available,
        discovery_observation: gate_view(result, "discovery_observation", valid_discovery_pair)?,
        controller_lease: gate_view(result, "controller_lease", valid_controller_lease_pair)?,
        resource_guard_admission_proof: gate_view(
            result,
            "resource_guard_admission_proof",
            valid_resource_guard_admission_proof_pair,
        )?,
    })
}

fn gate_view(
    result: &serde_json::Map<String, Value>,
    field: &str,
    valid_pair: fn(&str, &str) -> bool,
) -> Result<GateView, CliError> {
    let Some(value) = result.get(field) else {
        // Additive API-1 migration: absent means no claim, never a positive
        // approximation. A present object is strict and must be exact.
        return Ok(GateView::not_exposed());
    };
    let object = value.as_object().ok_or(CliError::MalformedResponse)?;
    if object.len() != 2 {
        return Err(CliError::MalformedResponse);
    }
    let state = required_string(object, "state")?;
    let reason = required_string(object, "reason")?;
    if !valid_pair(&state, &reason) {
        return Err(CliError::MalformedResponse);
    }
    Ok(GateView { state, reason })
}

pub fn valid_discovery_pair(state: &str, reason: &str) -> bool {
    matches!(
        (state, reason),
        ("FRESH_HINT", "C04_CANDIDATE_OBSERVED")
            | ("NO_HINT", "C04_NO_CANDIDATE")
            | ("BACKEND_UNAVAILABLE", "DISCOVERY_BACKEND_UNAVAILABLE")
            | ("STALE", "OBSERVATION_EXPIRED")
            | ("UNKNOWN", "EPOCH_INVALIDATED")
            | ("UNKNOWN", "NOT_OBSERVED")
            | ("UNKNOWN", "NOT_EXPOSED_BY_C12")
    )
}

pub fn valid_controller_lease_pair(state: &str, reason: &str) -> bool {
    matches!(
        (state, reason),
        ("ACTIVE", "C07_ACK_FRESH")
            | ("EXPIRED", "ACK_TTL_ELAPSED")
            | ("UNAVAILABLE", "C07_ACQUIRE_FAILED")
            | ("UNAVAILABLE", "C07_RENEW_FAILED")
            | ("UNAVAILABLE", "SESSION_INVALIDATED")
            | ("UNAVAILABLE", "IDENTITY_OR_INCARNATION_CHANGED")
            | ("UNAVAILABLE", "AUTO_USE_DISABLED")
            | ("UNKNOWN", "NOT_OBSERVED")
            | ("UNKNOWN", "NOT_EXPOSED_BY_C12")
    )
}

pub fn valid_resource_guard_admission_proof_pair(state: &str, reason: &str) -> bool {
    matches!(
        (state, reason),
        ("FRESH_PASS", "C08_C09_C10_PROBE_PASSED")
            | ("FAILED", "C08_C09_C10_PROBE_FAILED")
            | ("STALE", "PROOF_EXPIRED")
            | ("UNKNOWN", "SESSION_INVALIDATED")
            | ("UNKNOWN", "LEASE_INVALIDATED")
            | ("UNKNOWN", "IDENTITY_OR_INCARNATION_CHANGED")
            | ("UNKNOWN", "AUTO_USE_DISABLED")
            | ("UNKNOWN", "NOT_OBSERVED")
            | ("UNKNOWN", "NOT_EXPOSED_BY_C12")
    )
}

fn compute_blake3_over_stream(
    stream: &mut UnixStream,
    request_id: Value,
) -> Result<ComputeView, CliError> {
    let response = request_value(
        stream,
        request_id,
        "compute.submit",
        json!({"op_id": BLAKE3_OP_ID, "fixture": BLAKE3_FIXTURE}),
    )?;
    let result = response.as_object().ok_or(CliError::MalformedResponse)?;
    if result.len() != 7
        || result.get("state").and_then(Value::as_str) != Some("COMPLETED")
        || result.get("op_id").and_then(Value::as_str) != Some(BLAKE3_OP_ID)
        || result.get("fixture").and_then(Value::as_str) != Some(BLAKE3_FIXTURE)
        || result.get("input_bytes").and_then(Value::as_u64) != Some(BLAKE3_INPUT_BYTES)
        || result.get("digest_blake3_hex").and_then(Value::as_str) != Some(BLAKE3_DIGEST_HEX)
    {
        return Err(CliError::MalformedResponse);
    }
    let execution_source = required_string(result, "execution_source")?;
    let auto_use_reason = required_string(result, "auto_use_reason")?;
    if !valid_execution_source(&execution_source) || !valid_auto_use_reason(&auto_use_reason) {
        return Err(CliError::MalformedResponse);
    }
    if execution_source == REMOTE_SUCCESS && auto_use_reason != "READY" {
        return Err(CliError::MalformedResponse);
    }
    Ok(ComputeView {
        digest_blake3_hex: BLAKE3_DIGEST_HEX.to_owned(),
        execution_source,
        auto_use_reason,
    })
}

fn request_value(
    stream: &mut UnixStream,
    request_id: Value,
    method: &'static str,
    params: Value,
) -> Result<Value, CliError> {
    let request = json!({"api": 1, "id": request_id, "method": method, "params": params});
    let expected_id = request["id"].clone();
    let mut bytes = serde_json::to_vec(&request).map_err(|_| CliError::MalformedResponse)?;
    if bytes.len() + 1 > MAX_NDJSON_LINE_BYTES {
        return Err(CliError::ResponseTooLarge);
    }
    bytes.push(b'\n');
    stream.write_all(&bytes).map_err(|_| CliError::IoFailed)?;

    let response = read_bounded_line(stream)?;
    validate_response(&response, &expected_id)
}

fn valid_auto_use_state(value: &str) -> bool {
    matches!(
        value,
        "OFF"
            | "DISCOVERING"
            | "CONNECTING"
            | "AUTHENTICATING"
            | "ACQUIRING_AUTHORITY"
            | "CHECKING_READINESS"
            | "AVAILABLE"
            | "DEGRADED"
            | "RECONNECTING"
            | "UNAVAILABLE"
    )
}

fn valid_auto_use_reason(value: &str) -> bool {
    matches!(
        value,
        "OFF"
            | "NO_DEVICE"
            | "NOT_PAIRED"
            | "AUTH_FAILED"
            | "LEASE_UNAVAILABLE"
            | "WORKER_UNHEALTHY"
            | "RESOURCE_REFUSED"
            | "TRANSPORT_LOST"
            | "RECONNECTING"
            | "DISCOVERY_BACKEND_UNAVAILABLE"
            | "READY"
    )
}

fn valid_execution_source(value: &str) -> bool {
    matches!(
        value,
        REMOTE_SUCCESS
            | LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE
            | LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE
    )
}

fn read_bounded_line(stream: &mut impl Read) -> Result<Vec<u8>, CliError> {
    let mut line = Vec::with_capacity(4_096);
    let mut chunk = [0_u8; 4_096];
    loop {
        let count = stream.read(&mut chunk).map_err(|_| CliError::IoFailed)?;
        if count == 0 {
            return Err(CliError::MalformedResponse);
        }
        let received = &chunk[..count];
        if let Some(newline) = received.iter().position(|byte| *byte == b'\n') {
            if line.len() + newline > MAX_NDJSON_LINE_BYTES {
                return Err(CliError::ResponseTooLarge);
            }
            line.extend_from_slice(&received[..newline]);
            return Ok(line);
        }
        if line.len() + received.len() > MAX_NDJSON_LINE_BYTES {
            return Err(CliError::ResponseTooLarge);
        }
        line.extend_from_slice(received);
    }
}

fn validate_response(bytes: &[u8], expected_id: &Value) -> Result<Value, CliError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| CliError::MalformedResponse)?;
    let object = value.as_object().ok_or(CliError::MalformedResponse)?;
    let api_is_one = matches!(object.get("api"), Some(Value::Number(number)) if number.is_u64() && number.as_u64() == Some(1));
    if !api_is_one || object.get("id") != Some(expected_id) {
        return Err(CliError::MalformedResponse);
    }

    match object.get("ok") {
        Some(Value::Bool(true)) if !object.contains_key("error") => object
            .get("result")
            .cloned()
            .ok_or(CliError::MalformedResponse),
        Some(Value::Bool(false)) if !object.contains_key("result") => {
            let error = object
                .get("error")
                .and_then(Value::as_object)
                .ok_or(CliError::MalformedResponse)?;
            let code = required_string(error, "code")?;
            let message_safe = required_string(error, "message_safe")?;
            if error.get("scope").and_then(Value::as_str) != Some("REQUEST") {
                return Err(CliError::MalformedResponse);
            }
            Err(CliError::Remote { code, message_safe })
        }
        _ => Err(CliError::MalformedResponse),
    }
}

fn required_string(
    object: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<String, CliError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(CliError::MalformedResponse)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn roundtrip(response: Value, expected_id: Value) -> Result<StatusView, CliError> {
        let (mut client, mut server) = UnixStream::pair().expect("create CLI test stream");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set client test timeout");
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set server test timeout");
        let server_thread = thread::spawn(move || {
            let request = read_bounded_line(&mut server).expect("read CLI request");
            let request: Value = serde_json::from_slice(&request).expect("parse CLI request");
            assert_eq!(request["api"], 1);
            assert_eq!(request["id"], expected_id);
            assert_eq!(request["method"], "system.status");
            assert_eq!(request["params"], json!({}));
            let mut bytes = serde_json::to_vec(&response).expect("serialize server response");
            bytes.push(b'\n');
            server.write_all(&bytes).expect("write server response");
        });
        let result = status_over_stream(&mut client, json!("cli-test-id"));
        server_thread.join().expect("server test thread");
        result
    }

    fn compute_roundtrip(response: Value, expected_id: Value) -> Result<ComputeView, CliError> {
        let (mut client, mut server) = UnixStream::pair().expect("create CLI test stream");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set client test timeout");
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set server test timeout");
        let server_thread = thread::spawn(move || {
            let request = read_bounded_line(&mut server).expect("read CLI request");
            let request: Value = serde_json::from_slice(&request).expect("parse CLI request");
            assert_eq!(request["api"], 1);
            assert_eq!(request["id"], expected_id);
            assert_eq!(request["method"], "compute.submit");
            assert_eq!(
                request["params"],
                json!({"op_id": BLAKE3_OP_ID, "fixture": BLAKE3_FIXTURE})
            );
            let mut bytes = serde_json::to_vec(&response).expect("serialize server response");
            bytes.push(b'\n');
            server.write_all(&bytes).expect("write server response");
        });
        let result = compute_blake3_over_stream(&mut client, json!("compute-test-id"));
        server_thread.join().expect("server test thread");
        result
    }

    fn status_result() -> Value {
        json!({
            "runtime_state": "READY",
            "local_api_state": "ACTIVE",
            "remote_worker_state": "AUTHENTICATED",
            "auto_use_state": "AVAILABLE",
            "auto_use_reason": "READY",
            "remote_blake3_available": true,
            "discovery_observation": {"state": "UNKNOWN", "reason": "NOT_EXPOSED_BY_C12"},
            "controller_lease": {"state": "UNKNOWN", "reason": "NOT_EXPOSED_BY_C12"},
            "resource_guard_admission_proof": {"state": "UNKNOWN", "reason": "NOT_EXPOSED_BY_C12"}
        })
    }

    #[test]
    fn p2_compatibility_tables_accept_every_exact_pair_and_reject_cross_pairs() {
        let discovery = [
            ("FRESH_HINT", "C04_CANDIDATE_OBSERVED"),
            ("NO_HINT", "C04_NO_CANDIDATE"),
            ("BACKEND_UNAVAILABLE", "DISCOVERY_BACKEND_UNAVAILABLE"),
            ("STALE", "OBSERVATION_EXPIRED"),
            ("UNKNOWN", "EPOCH_INVALIDATED"),
            ("UNKNOWN", "NOT_OBSERVED"),
            ("UNKNOWN", "NOT_EXPOSED_BY_C12"),
        ];
        let lease = [
            ("ACTIVE", "C07_ACK_FRESH"),
            ("EXPIRED", "ACK_TTL_ELAPSED"),
            ("UNAVAILABLE", "C07_ACQUIRE_FAILED"),
            ("UNAVAILABLE", "C07_RENEW_FAILED"),
            ("UNAVAILABLE", "SESSION_INVALIDATED"),
            ("UNAVAILABLE", "IDENTITY_OR_INCARNATION_CHANGED"),
            ("UNAVAILABLE", "AUTO_USE_DISABLED"),
            ("UNKNOWN", "NOT_OBSERVED"),
            ("UNKNOWN", "NOT_EXPOSED_BY_C12"),
        ];
        let proof = [
            ("FRESH_PASS", "C08_C09_C10_PROBE_PASSED"),
            ("FAILED", "C08_C09_C10_PROBE_FAILED"),
            ("STALE", "PROOF_EXPIRED"),
            ("UNKNOWN", "SESSION_INVALIDATED"),
            ("UNKNOWN", "LEASE_INVALIDATED"),
            ("UNKNOWN", "IDENTITY_OR_INCARNATION_CHANGED"),
            ("UNKNOWN", "AUTO_USE_DISABLED"),
            ("UNKNOWN", "NOT_OBSERVED"),
            ("UNKNOWN", "NOT_EXPOSED_BY_C12"),
        ];
        for (state, reason) in discovery {
            assert!(valid_discovery_pair(state, reason));
        }
        for (state, reason) in lease {
            assert!(valid_controller_lease_pair(state, reason));
        }
        for (state, reason) in proof {
            assert!(valid_resource_guard_admission_proof_pair(state, reason));
        }
        assert!(!valid_discovery_pair("FRESH_HINT", "C04_NO_CANDIDATE"));
        assert!(!valid_controller_lease_pair("ACTIVE", "ACK_TTL_ELAPSED"));
        assert!(!valid_resource_guard_admission_proof_pair(
            "FRESH_PASS",
            "C08_C09_C10_PROBE_FAILED"
        ));
        assert!(!valid_resource_guard_admission_proof_pair(
            "UNKNOWN",
            "PROOF_EXPIRED"
        ));
    }

    fn compute_result(source: &str, reason: &str) -> Value {
        json!({
            "state": "COMPLETED",
            "op_id": BLAKE3_OP_ID,
            "fixture": BLAKE3_FIXTURE,
            "input_bytes": BLAKE3_INPUT_BYTES,
            "digest_blake3_hex": BLAKE3_DIGEST_HEX,
            "execution_source": source,
            "auto_use_reason": reason
        })
    }

    #[test]
    fn canonical_socket_path_requires_absolute_nonempty_runtime() {
        assert_eq!(
            canonical_control_socket(Some(OsString::from("/run/user/1000"))),
            Ok(PathBuf::from("/run/user/1000/phoneboost/control.sock"))
        );
        assert_eq!(
            canonical_control_socket(None),
            Err(CliError::RuntimeUnavailable)
        );
        assert_eq!(
            canonical_control_socket(Some(OsString::from("relative"))),
            Err(CliError::RuntimeUnavailable)
        );
    }

    #[test]
    fn status_sends_exact_request_and_renders_real_response() {
        let view = roundtrip(
            json!({
                "api": 1,
                "id": "cli-test-id",
                "ok": true,
                "result": status_result()
            }),
            json!("cli-test-id"),
        )
        .expect("status roundtrip succeeds");
        assert_eq!(
            view.to_string(),
            "PhoneBoost: READY\nLocal API: ACTIVE\nAndroid worker: AUTHENTICATED\nAuto-use: AVAILABLE\nAuto-use reason: READY\nDiscovery observation: UNKNOWN (NOT_EXPOSED_BY_C12)\nController lease: UNKNOWN (NOT_EXPOSED_BY_C12)\nLatest admission/readiness proof: UNKNOWN (NOT_EXPOSED_BY_C12)\nRemote BLAKE3: AVAILABLE"
        );
    }

    #[test]
    fn old_status_without_readiness_fields_is_never_remote_ready() {
        let error = roundtrip(
            json!({
                "api": 1,
                "id": "cli-test-id",
                "ok": true,
                "result": {
                    "runtime_state": "READY",
                    "local_api_state": "ACTIVE",
                    "remote_worker_state": "AUTHENTICATED"
                }
            }),
            json!("cli-test-id"),
        );
        assert_eq!(error, Err(CliError::MalformedResponse));
    }

    #[test]
    fn status_rejects_inconsistent_readiness_invariant() {
        for (state, reason, available) in [
            ("AVAILABLE", "READY", false),
            ("AVAILABLE", "NO_DEVICE", true),
            ("DEGRADED", "READY", true),
        ] {
            let mut result = status_result();
            result["auto_use_state"] = json!(state);
            result["auto_use_reason"] = json!(reason);
            result["remote_blake3_available"] = json!(available);
            assert_eq!(
                roundtrip(
                    json!({"api":1,"id":"cli-test-id","ok":true,"result":result}),
                    json!("cli-test-id")
                ),
                Err(CliError::MalformedResponse)
            );
        }
    }

    #[test]
    fn status_rejects_wrong_id_api_or_ambiguous_envelope() {
        for response in [
            json!({"api":1,"id":"wrong","ok":true,"result":{}}),
            json!({"api":"1","id":"cli-test-id","ok":true,"result":{}}),
            json!({"api":1,"id":"cli-test-id","ok":true,"result":{},"error":{}}),
        ] {
            assert_eq!(
                roundtrip(response, json!("cli-test-id")),
                Err(CliError::MalformedResponse)
            );
        }
    }

    #[test]
    fn status_rejects_malformed_json_response() {
        assert_eq!(
            validate_response(b"{", &json!("cli-test-id")),
            Err(CliError::MalformedResponse)
        );
    }

    #[test]
    fn status_surfaces_only_c12_safe_remote_error() {
        let error = roundtrip(
            json!({
                "api":1,
                "id":"cli-test-id",
                "ok":false,
                "error":{
                    "code":"LOCAL_BAD_REQUEST",
                    "scope":"REQUEST",
                    "message_safe":"safe"
                }
            }),
            json!("cli-test-id"),
        )
        .expect_err("remote error is surfaced");
        assert_eq!(
            error,
            CliError::Remote {
                code: "LOCAL_BAD_REQUEST".to_owned(),
                message_safe: "safe".to_owned()
            }
        );
    }

    #[test]
    fn bounded_reader_rejects_response_over_64kib() {
        let mut oversized = vec![b'x'; MAX_NDJSON_LINE_BYTES + 1];
        oversized.push(b'\n');
        let mut input = std::io::Cursor::new(oversized);
        assert_eq!(
            read_bounded_line(&mut input),
            Err(CliError::ResponseTooLarge)
        );
    }

    #[test]
    fn request_ids_are_string_and_unique_within_process() {
        let first = next_request_id();
        let second = next_request_id();
        assert!(first.is_string());
        assert!(second.is_string());
        assert_ne!(first, second);
    }

    #[test]
    fn closed_readiness_and_execution_string_sets_are_exhaustive() {
        let states = [
            "OFF",
            "DISCOVERING",
            "CONNECTING",
            "AUTHENTICATING",
            "ACQUIRING_AUTHORITY",
            "CHECKING_READINESS",
            "AVAILABLE",
            "DEGRADED",
            "RECONNECTING",
            "UNAVAILABLE",
        ];
        assert!(states.into_iter().all(valid_auto_use_state));
        assert!(!valid_auto_use_state("Available"));
        let reasons = [
            "OFF",
            "NO_DEVICE",
            "NOT_PAIRED",
            "AUTH_FAILED",
            "LEASE_UNAVAILABLE",
            "WORKER_UNHEALTHY",
            "RESOURCE_REFUSED",
            "TRANSPORT_LOST",
            "RECONNECTING",
            "DISCOVERY_BACKEND_UNAVAILABLE",
            "READY",
        ];
        assert!(reasons.into_iter().all(valid_auto_use_reason));
        assert!(!valid_auto_use_reason("ready"));
        let sources = [
            REMOTE_SUCCESS,
            LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE,
            LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE,
        ];
        assert!(sources.into_iter().all(valid_execution_source));
        assert!(!valid_execution_source("REMOTE"));
    }

    #[test]
    fn compute_ndjson_output_and_exit_codes_are_exact_for_all_sources() {
        for (source, reason, exit_code) in [
            (REMOTE_SUCCESS, "READY", 0),
            (LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE, "TRANSPORT_LOST", 3),
            (LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE, "RESOURCE_REFUSED", 3),
        ] {
            let view = compute_roundtrip(
                json!({
                    "api": 1,
                    "id": "compute-test-id",
                    "ok": true,
                    "result": compute_result(source, reason)
                }),
                json!("compute-test-id"),
            )
            .expect("valid compute response");
            assert_eq!(view.exit_code(), exit_code);
            assert_eq!(
                view.to_string(),
                format!(
                    "BLAKE3 fixture: c10-abc-v1\nInput bytes: 3\nBLAKE3 digest: {BLAKE3_DIGEST_HEX}\nExecution source: {source}\nAuto-use reason: {reason}"
                )
            );
        }
    }

    #[test]
    fn compute_rejects_schema_drift_and_remote_success_with_nonready_reason() {
        let mut extra = compute_result(REMOTE_SUCCESS, "READY");
        extra["extra"] = json!(true);
        for result in [
            extra,
            compute_result("REMOTE", "READY"),
            compute_result(REMOTE_SUCCESS, "TRANSPORT_LOST"),
            {
                let mut wrong = compute_result(REMOTE_SUCCESS, "READY");
                wrong["digest_blake3_hex"] = json!("00");
                wrong
            },
        ] {
            assert_eq!(
                compute_roundtrip(
                    json!({
                        "api": 1,
                        "id": "compute-test-id",
                        "ok": true,
                        "result": result
                    }),
                    json!("compute-test-id")
                ),
                Err(CliError::MalformedResponse)
            );
        }
    }
}
