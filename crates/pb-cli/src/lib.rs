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
        write!(formatter, "Android worker: {}", self.remote_worker_state)
    }
}

pub fn status() -> Result<StatusView, CliError> {
    let mut stream = connect_local_api()?;
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

fn connect_local_api() -> Result<UnixStream, CliError> {
    let socket_path = canonical_control_socket(env::var_os("XDG_RUNTIME_DIR"))?;
    let stream = UnixStream::connect(socket_path).map_err(|_| CliError::ConnectFailed)?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|_| CliError::IoFailed)?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|_| CliError::IoFailed)?;
    Ok(stream)
}

fn pairing_request(method: &'static str) -> Result<PairingView, CliError> {
    let mut stream = connect_local_api()?;
    let request_id = next_request_id();
    let response = request_value(&mut stream, request_id, method)?;
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
    let response = request_value(stream, request_id, "system.status")?;
    let result = response.as_object().ok_or(CliError::MalformedResponse)?;
    Ok(StatusView {
        runtime_state: required_string(result, "runtime_state")?,
        local_api_state: required_string(result, "local_api_state")?,
        remote_worker_state: required_string(result, "remote_worker_state")?,
    })
}

fn request_value(
    stream: &mut UnixStream,
    request_id: Value,
    method: &'static str,
) -> Result<Value, CliError> {
    let request = json!({"api": 1, "id": request_id, "method": method, "params": {}});
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
                "result": {
                    "runtime_state": "READY",
                    "local_api_state": "ACTIVE",
                    "remote_worker_state": "NOT_CONFIGURED"
                }
            }),
            json!("cli-test-id"),
        )
        .expect("status roundtrip succeeds");
        assert_eq!(
            view.to_string(),
            "PhoneBoost: READY\nLocal API: ACTIVE\nAndroid worker: NOT_CONFIGURED"
        );
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
}
