use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

const HEADER_LIMIT: usize = 8 * 1024;
const HEADER_FIELD_LIMIT: usize = 32;
const BODY_LIMIT: usize = 1024;
const RESPONSE_LIMIT: usize = 64 * 1024;
const STATIC_LIMIT: u64 = 8 * 1024 * 1024;
const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);
const STATUS_TIMEOUT: Duration = Duration::from_secs(2);
const SNAPSHOT_MAX_AGE_MS: u64 = 3_000;
const TOKEN_BYTES: usize = 32;
const TOKEN_HEX_LEN: usize = TOKEN_BYTES * 2;
const BLAKE3_FIXTURE: &str = "c10-abc-v1";

const SECURITY_HEADERS: &[(&str, &str)] = &[
    (
        "Content-Security-Policy",
        "default-src 'self'; connect-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    ),
    ("X-Content-Type-Options", "nosniff"),
    ("X-Frame-Options", "DENY"),
    ("Referrer-Policy", "no-referrer"),
    ("Cache-Control", "no-store"),
];

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

#[derive(Debug)]
struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    head_only: bool,
}

impl Response {
    fn json(status: u16, value: Value) -> Self {
        let mut status = status;
        let mut body = serde_json::to_vec(&value).unwrap_or_else(|_| {
            br#"{"provenance":"UNAVAILABLE","reason":"BRIDGE_SERIALIZATION_FAILED"}"#.to_vec()
        });
        if body.len() > RESPONSE_LIMIT {
            status = 503;
            body = br#"{"provenance":"UNAVAILABLE","reason":"BRIDGE_RESPONSE_TOO_LARGE"}"#.to_vec();
        }
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body,
            head_only: false,
        }
    }

    fn error(status: u16, reason: &'static str) -> Self {
        Self::json(
            status,
            json!({"provenance": "UNAVAILABLE", "reason": reason}),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeStatus {
    runtime_state: String,
    local_api_state: String,
    remote_worker_state: String,
    auto_use_state: String,
    auto_use_reason: String,
    remote_blake3_available: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeCompute {
    digest_blake3_hex: String,
    execution_source: String,
    auto_use_reason: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeUnavailable;

trait NativeBackend {
    fn status(&mut self) -> Result<NativeStatus, NativeUnavailable>;
    fn compute_blake3(&mut self) -> Result<NativeCompute, NativeUnavailable>;
}

struct ProductionNative;

impl NativeBackend for ProductionNative {
    fn status(&mut self) -> Result<NativeStatus, NativeUnavailable> {
        let view = pb_cli::status_with_timeout(STATUS_TIMEOUT).map_err(|_| NativeUnavailable)?;
        Ok(NativeStatus {
            runtime_state: view.runtime_state().to_owned(),
            local_api_state: view.local_api_state().to_owned(),
            remote_worker_state: view.remote_worker_state().to_owned(),
            auto_use_state: view.auto_use_state().to_owned(),
            auto_use_reason: view.auto_use_reason().to_owned(),
            remote_blake3_available: view.remote_blake3_available(),
        })
    }

    fn compute_blake3(&mut self) -> Result<NativeCompute, NativeUnavailable> {
        let view = pb_cli::compute_blake3().map_err(|_| NativeUnavailable)?;
        Ok(NativeCompute {
            digest_blake3_hex: view.digest_blake3_hex().to_owned(),
            execution_source: view.execution_source().to_owned(),
            auto_use_reason: view.auto_use_reason().to_owned(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExecutionObservation {
    observed_at_unix_ms: u64,
    digest_blake3_hex: String,
    execution_source: String,
    auto_use_reason: String,
}

struct Bridge<N> {
    expected_host: String,
    token: String,
    frontend_root: PathBuf,
    native: N,
    last_execution: Option<ExecutionObservation>,
}

impl<N: NativeBackend> Bridge<N> {
    fn handle(&mut self, request: Request) -> Response {
        if !self.valid_browser_boundary(&request) {
            return Response::error(403, "BROWSER_BOUNDARY_REJECTED");
        }

        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/bridge/v1/snapshot") => {
                if !request.body.is_empty() {
                    return Response::error(400, "INVALID_REQUEST");
                }
                if !self.valid_token(&request) {
                    return Response::error(401, "BRIDGE_CAPABILITY_REQUIRED");
                }
                self.snapshot()
            }
            ("POST", "/bridge/v1/compute/blake3") => {
                if !self.valid_token(&request) {
                    return Response::error(401, "BRIDGE_CAPABILITY_REQUIRED");
                }
                self.compute(&request)
            }
            (_, "/bridge/v1/snapshot" | "/bridge/v1/compute/blake3") => {
                Response::error(405, "METHOD_NOT_ALLOWED")
            }
            ("GET" | "HEAD", path) if !path.starts_with("/bridge/") => {
                self.static_asset(path, request.method == "HEAD")
            }
            ("OPTIONS", _) => Response::error(405, "METHOD_NOT_ALLOWED"),
            _ => Response::error(404, "NOT_FOUND"),
        }
    }

    fn valid_browser_boundary(&self, request: &Request) -> bool {
        if request.headers.get("host") != Some(&self.expected_host) {
            return false;
        }
        let expected_origin = format!("http://{}", self.expected_host);
        if let Some(origin) = request.headers.get("origin")
            && origin != &expected_origin
        {
            return false;
        }
        if request.method == "POST" && request.headers.get("origin") != Some(&expected_origin) {
            return false;
        }
        if let Some(site) = request.headers.get("sec-fetch-site")
            && site != "same-origin"
            && site != "none"
        {
            return false;
        }
        true
    }

    fn valid_token(&self, request: &Request) -> bool {
        request
            .headers
            .get("x-phoneboost-bridge-token")
            .is_some_and(|candidate| constant_time_eq(candidate.as_bytes(), self.token.as_bytes()))
    }

    fn snapshot(&mut self) -> Response {
        let observed_at_unix_ms = now_unix_ms();
        let status = match self.native.status() {
            Ok(status) => status,
            Err(_) => {
                return Response::json(
                    503,
                    json!({
                        "provenance": "UNAVAILABLE",
                        "observed_at_unix_ms": observed_at_unix_ms,
                        "reason": "LOCAL_RUNTIME_UNAVAILABLE"
                    }),
                );
            }
        };
        let authenticated = status.remote_worker_state == "AUTHENTICATED";
        let provider_state = if status.remote_blake3_available {
            "AVAILABLE"
        } else {
            "UNAVAILABLE"
        };
        let last_execution = self.last_execution.as_ref().map(|execution| {
            json!({
                "observed_at_unix_ms": execution.observed_at_unix_ms,
                "fixture": BLAKE3_FIXTURE,
                "digest_blake3_hex": execution.digest_blake3_hex,
                "execution_source": execution.execution_source,
                "auto_use_reason": execution.auto_use_reason,
            })
        });
        Response::json(
            200,
            json!({
                "provenance": "LIVE",
                "observed_at_unix_ms": observed_at_unix_ms,
                "max_age_ms": SNAPSHOT_MAX_AGE_MS,
                "local_daemon": {
                    "state": "REACHABLE",
                    "runtime_state": status.runtime_state,
                    "local_api_state": status.local_api_state,
                },
                "discovery": {
                    "state": "UNKNOWN",
                    "reason": "NOT_EXPOSED_BY_C12"
                },
                "authenticated_session": {
                    "state": if authenticated { "AUTHENTICATED" } else { "UNAVAILABLE" },
                    "remote_worker_state": status.remote_worker_state,
                },
                "controller_lease": {
                    "state": "UNKNOWN",
                    "reason": "NOT_EXPOSED_BY_C12"
                },
                "resource_guard": {
                    "state": "UNKNOWN",
                    "reason": "NOT_EXPOSED_BY_C12"
                },
                "provider_readiness": {
                    "provider": "pb.native.blake3/1",
                    "state": provider_state,
                },
                "auto_use": {
                    "state": status.auto_use_state,
                    "reason": status.auto_use_reason,
                },
                "remote_blake3_available": status.remote_blake3_available,
                "last_execution": last_execution,
            }),
        )
    }

    fn compute(&mut self, request: &Request) -> Response {
        if request.headers.get("content-type").map(String::as_str) != Some("application/json") {
            return Response::error(415, "CONTENT_TYPE_REQUIRED");
        }
        let body: Value = match serde_json::from_slice(&request.body) {
            Ok(value) => value,
            Err(_) => return Response::error(400, "INVALID_COMPUTE_REQUEST"),
        };
        let valid = body.as_object().is_some_and(|object| {
            object.len() == 1
                && object.get("fixture").and_then(Value::as_str) == Some(BLAKE3_FIXTURE)
        });
        if !valid {
            return Response::error(400, "INVALID_COMPUTE_REQUEST");
        }

        let result = match self.native.compute_blake3() {
            Ok(result) => result,
            Err(_) => return Response::error(503, "LOCAL_RUNTIME_UNAVAILABLE"),
        };
        let observed_at_unix_ms = now_unix_ms();
        self.last_execution = Some(ExecutionObservation {
            observed_at_unix_ms,
            digest_blake3_hex: result.digest_blake3_hex.clone(),
            execution_source: result.execution_source.clone(),
            auto_use_reason: result.auto_use_reason.clone(),
        });
        Response::json(
            200,
            json!({
                "provenance": "LIVE",
                "observed_at_unix_ms": observed_at_unix_ms,
                "fixture": BLAKE3_FIXTURE,
                "input_bytes": 3,
                "digest_blake3_hex": result.digest_blake3_hex,
                "execution_source": result.execution_source,
                "auto_use_reason": result.auto_use_reason,
            }),
        )
    }

    fn static_asset(&self, request_path: &str, head_only: bool) -> Response {
        let relative = if request_path == "/" {
            Path::new("index.html")
        } else {
            match safe_relative_path(request_path) {
                Some(path) => path,
                None => return Response::error(404, "NOT_FOUND"),
            }
        };
        let candidate = self.frontend_root.join(relative);
        let canonical = match candidate.canonicalize() {
            Ok(path) if path.starts_with(&self.frontend_root) => path,
            _ => return Response::error(404, "NOT_FOUND"),
        };
        let metadata = match canonical.metadata() {
            Ok(metadata) if metadata.is_file() && metadata.len() <= STATIC_LIMIT => metadata,
            _ => return Response::error(404, "NOT_FOUND"),
        };
        let file = match File::open(&canonical) {
            Ok(file) => file,
            Err(_) => return Response::error(404, "NOT_FOUND"),
        };
        let mut body = Vec::with_capacity(metadata.len() as usize);
        if file.take(STATIC_LIMIT + 1).read_to_end(&mut body).is_err()
            || body.len() as u64 != metadata.len()
            || body.len() as u64 > STATIC_LIMIT
        {
            return Response::error(404, "NOT_FOUND");
        }
        let Some(content_type) = content_type(&canonical) else {
            return Response::error(404, "NOT_FOUND");
        };
        Response {
            status: 200,
            content_type,
            body,
            head_only,
        }
    }
}

fn safe_relative_path(request_path: &str) -> Option<&Path> {
    if !request_path.starts_with('/')
        || request_path.contains('%')
        || request_path.contains('\\')
        || request_path.contains('?')
        || request_path.contains('#')
    {
        return None;
    }
    let relative = Path::new(request_path.trim_start_matches('/'));
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(relative)
}

fn content_type(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => Some("text/html; charset=utf-8"),
        Some("css") => Some("text/css; charset=utf-8"),
        Some("js") => Some("text/javascript; charset=utf-8"),
        Some("json") => Some("application/json; charset=utf-8"),
        Some("svg") => Some("image/svg+xml"),
        Some("png") => Some("image/png"),
        Some("ico") => Some("image/x-icon"),
        Some("woff") => Some("font/woff"),
        Some("woff2") => Some("font/woff2"),
        _ => None,
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn read_request(stream: &mut impl Read) -> Result<Request, Response> {
    let mut bytes = Vec::with_capacity(1024);
    let header_end = loop {
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let end = position + 4;
            if end > HEADER_LIMIT {
                return Err(Response::error(413, "REQUEST_HEADERS_TOO_LARGE"));
            }
            break end;
        }
        if bytes.len() >= HEADER_LIMIT {
            return Err(Response::error(413, "REQUEST_HEADERS_TOO_LARGE"));
        }
        let mut chunk = [0_u8; 1024];
        let count = stream
            .read(&mut chunk)
            .map_err(|_| Response::error(400, "INVALID_REQUEST"))?;
        if count == 0 {
            return Err(Response::error(400, "INVALID_REQUEST"));
        }
        bytes.extend_from_slice(&chunk[..count]);
    };

    let header_text = std::str::from_utf8(&bytes[..header_end - 4])
        .map_err(|_| Response::error(400, "INVALID_REQUEST"))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| Response::error(400, "INVALID_REQUEST"))?;
    let request_parts: Vec<_> = request_line.split(' ').collect();
    if request_parts.len() != 3 || request_parts[2] != "HTTP/1.1" {
        return Err(Response::error(400, "INVALID_REQUEST"));
    }
    let method = request_parts[0];
    if !method.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err(Response::error(400, "INVALID_REQUEST"));
    }
    let path = request_parts[1];
    if !path.starts_with('/') || path.contains('?') || path.contains('#') {
        return Err(Response::error(400, "INVALID_REQUEST"));
    }

    let mut headers = BTreeMap::new();
    let mut field_count = 0_usize;
    for line in lines {
        field_count += 1;
        if field_count > HEADER_FIELD_LIMIT || line.starts_with(' ') || line.starts_with('\t') {
            return Err(Response::error(400, "INVALID_REQUEST"));
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| Response::error(400, "INVALID_REQUEST"))?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(Response::error(400, "INVALID_REQUEST"));
        }
        let value = value.trim();
        if value
            .bytes()
            .any(|byte| byte.is_ascii_control() && byte != b'\t')
        {
            return Err(Response::error(400, "INVALID_REQUEST"));
        }
        if headers
            .insert(name.to_ascii_lowercase(), value.to_owned())
            .is_some()
        {
            return Err(Response::error(400, "INVALID_REQUEST"));
        }
    }
    if headers.contains_key("transfer-encoding") {
        return Err(Response::error(400, "TRANSFER_ENCODING_UNSUPPORTED"));
    }
    let body_length = match headers.get("content-length") {
        Some(value) if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
            value
                .parse::<usize>()
                .map_err(|_| Response::error(400, "INVALID_CONTENT_LENGTH"))?
        }
        Some(_) => return Err(Response::error(400, "INVALID_CONTENT_LENGTH")),
        None => 0,
    };
    if body_length > BODY_LIMIT {
        return Err(Response::error(413, "REQUEST_BODY_TOO_LARGE"));
    }
    if method == "POST" && !headers.contains_key("content-length") {
        return Err(Response::error(411, "CONTENT_LENGTH_REQUIRED"));
    }

    let mut body = bytes[header_end..].to_vec();
    if body.len() > body_length {
        return Err(Response::error(400, "REQUEST_PIPELINING_UNSUPPORTED"));
    }
    while body.len() < body_length {
        let remaining = body_length - body.len();
        let mut chunk = [0_u8; 1024];
        let read_length = remaining.min(chunk.len());
        let count = stream
            .read(&mut chunk[..read_length])
            .map_err(|_| Response::error(400, "INVALID_REQUEST"))?;
        if count == 0 {
            return Err(Response::error(400, "INVALID_REQUEST"));
        }
        body.extend_from_slice(&chunk[..count]);
    }
    if method != "POST" && !body.is_empty() {
        return Err(Response::error(400, "INVALID_REQUEST"));
    }
    Ok(Request {
        method: method.to_owned(),
        path: path.to_owned(),
        headers,
        body,
    })
}

fn write_response(stream: &mut impl Write, response: &Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        503 => "Service Unavailable",
        _ => "Error",
    };
    write!(stream, "HTTP/1.1 {} {}\r\n", response.status, reason)?;
    write!(stream, "Content-Type: {}\r\n", response.content_type)?;
    write!(stream, "Content-Length: {}\r\n", response.body.len())?;
    write!(stream, "Connection: close\r\n")?;
    for (name, value) in SECURITY_HEADERS {
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "\r\n")?;
    if !response.head_only {
        stream.write_all(&response.body)?;
    }
    stream.flush()
}

fn handle_connection<N: NativeBackend>(
    stream: &mut TcpStream,
    bridge: &mut Bridge<N>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(SOCKET_TIMEOUT))?;
    stream.set_write_timeout(Some(SOCKET_TIMEOUT))?;
    let response = match read_request(stream) {
        Ok(request) => bridge.handle(request),
        Err(response) => response,
    };
    write_response(stream, &response)
}

fn bind_loopback() -> std::io::Result<TcpListener> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    match listener.local_addr()? {
        SocketAddr::V4(address) if address.ip().is_loopback() => Ok(listener),
        _ => Err(std::io::Error::other("bridge did not bind IPv4 loopback")),
    }
}

fn generate_token() -> Result<String, String> {
    let mut source = File::open("/dev/urandom")
        .map_err(|_| "secure random capability generation failed".to_owned())?;
    let random = loop {
        let mut candidate = [0_u8; TOKEN_BYTES];
        source
            .read_exact(&mut candidate)
            .map_err(|_| "secure random capability generation failed".to_owned())?;
        if candidate != [0_u8; TOKEN_BYTES] {
            break candidate;
        }
    };
    let mut token = String::with_capacity(TOKEN_HEX_LEN);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in random {
        token.push(HEX[(byte >> 4) as usize] as char);
        token.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(token)
}

fn parse_frontend_root() -> Result<PathBuf, String> {
    if std::env::args_os().len() != 1 {
        return Err("usage: phoneboost-web-bridge".to_owned());
    }
    let path = PathBuf::from("frontend/build");
    let canonical = path
        .canonicalize()
        .map_err(|_| "frontend build directory is unavailable".to_owned())?;
    if !canonical.is_dir() || !canonical.join("index.html").is_file() {
        return Err("frontend build directory is invalid".to_owned());
    }
    Ok(canonical)
}

pub fn run() -> Result<(), String> {
    let frontend_root = parse_frontend_root()?;
    let listener = bind_loopback().map_err(|_| "loopback listener failed".to_owned())?;
    let address = listener
        .local_addr()
        .map_err(|_| "loopback listener address unavailable".to_owned())?;
    let token = generate_token()?;
    let expected_host = address.to_string();
    let mut bridge = Bridge {
        expected_host: expected_host.clone(),
        token: token.clone(),
        frontend_root,
        native: ProductionNative,
        last_execution: None,
    };
    println!("PhoneBoost local Control Center:");
    println!("http://{expected_host}/#token={token}");
    println!("Keep this process running; press Ctrl+C to stop it.");
    std::io::stdout()
        .flush()
        .map_err(|_| "launch URL output failed".to_owned())?;

    for connection in listener.incoming() {
        match connection {
            Ok(mut stream) => {
                let _ = handle_connection(&mut stream, &mut bridge);
            }
            Err(_) => continue,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::io::Cursor;

    struct FakeNative {
        status: Result<NativeStatus, NativeUnavailable>,
        compute: Result<NativeCompute, NativeUnavailable>,
        status_calls: Cell<usize>,
        compute_calls: Cell<usize>,
    }

    impl NativeBackend for FakeNative {
        fn status(&mut self) -> Result<NativeStatus, NativeUnavailable> {
            self.status_calls.set(self.status_calls.get() + 1);
            self.status.clone()
        }

        fn compute_blake3(&mut self) -> Result<NativeCompute, NativeUnavailable> {
            self.compute_calls.set(self.compute_calls.get() + 1);
            self.compute.clone()
        }
    }

    fn ready_status() -> NativeStatus {
        NativeStatus {
            runtime_state: "READY".to_owned(),
            local_api_state: "ACTIVE".to_owned(),
            remote_worker_state: "AUTHENTICATED".to_owned(),
            auto_use_state: "AVAILABLE".to_owned(),
            auto_use_reason: "READY".to_owned(),
            remote_blake3_available: true,
        }
    }

    fn remote_compute(source: &str, reason: &str) -> NativeCompute {
        NativeCompute {
            digest_blake3_hex: "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
                .to_owned(),
            execution_source: source.to_owned(),
            auto_use_reason: reason.to_owned(),
        }
    }

    fn fake_native() -> FakeNative {
        FakeNative {
            status: Ok(ready_status()),
            compute: Ok(remote_compute("REMOTE_SUCCESS", "READY")),
            status_calls: Cell::new(0),
            compute_calls: Cell::new(0),
        }
    }

    fn bridge(native: FakeNative) -> Bridge<FakeNative> {
        Bridge {
            expected_host: "127.0.0.1:32123".to_owned(),
            token: "a".repeat(TOKEN_HEX_LEN),
            frontend_root: PathBuf::from("/nonexistent-test-root"),
            native,
            last_execution: None,
        }
    }

    fn request(method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Request {
        let mut map = BTreeMap::new();
        map.insert("host".to_owned(), "127.0.0.1:32123".to_owned());
        map.insert(
            "x-phoneboost-bridge-token".to_owned(),
            "a".repeat(TOKEN_HEX_LEN),
        );
        if method == "POST" {
            map.insert("origin".to_owned(), "http://127.0.0.1:32123".to_owned());
        }
        for (name, value) in headers {
            map.insert((*name).to_owned(), (*value).to_owned());
        }
        Request {
            method: method.to_owned(),
            path: path.to_owned(),
            headers: map,
            body: body.to_vec(),
        }
    }

    fn json_body(response: &Response) -> Value {
        serde_json::from_slice(&response.body).expect("response JSON")
    }

    #[test]
    fn listener_is_literal_ipv4_loopback() {
        let listener = bind_loopback().expect("bind loopback");
        match listener.local_addr().expect("listener address") {
            SocketAddr::V4(address) => assert_eq!(*address.ip(), Ipv4Addr::LOCALHOST),
            SocketAddr::V6(_) => panic!("IPv6 listener was not requested"),
        }
    }

    #[test]
    fn snapshot_exposes_current_truth_and_unknown_unexposed_gates() {
        let mut bridge = bridge(fake_native());
        let response = bridge.handle(request("GET", "/bridge/v1/snapshot", &[], b""));
        let body = json_body(&response);
        assert_eq!(response.status, 200);
        assert_eq!(body["provenance"], "LIVE");
        assert_eq!(body["max_age_ms"], SNAPSHOT_MAX_AGE_MS);
        assert_eq!(body["local_daemon"]["state"], "REACHABLE");
        assert_eq!(body["authenticated_session"]["state"], "AUTHENTICATED");
        assert_eq!(body["discovery"]["state"], "UNKNOWN");
        assert_eq!(body["controller_lease"]["state"], "UNKNOWN");
        assert_eq!(body["resource_guard"]["state"], "UNKNOWN");
        assert_eq!(body["provider_readiness"]["state"], "AVAILABLE");
        assert_eq!(bridge.native.status_calls.get(), 1);
    }

    #[test]
    fn daemon_absent_is_never_live() {
        let mut native = fake_native();
        native.status = Err(NativeUnavailable);
        let mut bridge = bridge(native);
        let response = bridge.handle(request("GET", "/bridge/v1/snapshot", &[], b""));
        let body = json_body(&response);
        assert_eq!(response.status, 503);
        assert_eq!(body["provenance"], "UNAVAILABLE");
        assert_eq!(body["reason"], "LOCAL_RUNTIME_UNAVAILABLE");
    }

    #[test]
    fn reconnecting_and_android_unavailable_remain_distinct() {
        let mut native = fake_native();
        native.status = Ok(NativeStatus {
            remote_worker_state: "NOT_CONFIGURED".to_owned(),
            auto_use_state: "RECONNECTING".to_owned(),
            auto_use_reason: "RECONNECTING".to_owned(),
            remote_blake3_available: false,
            ..ready_status()
        });
        let mut bridge = bridge(native);
        let response = bridge.handle(request("GET", "/bridge/v1/snapshot", &[], b""));
        let body = json_body(&response);
        assert_eq!(body["provenance"], "LIVE");
        assert_eq!(body["authenticated_session"]["state"], "UNAVAILABLE");
        assert_eq!(
            body["authenticated_session"]["remote_worker_state"],
            "NOT_CONFIGURED"
        );
        assert_eq!(body["auto_use"]["state"], "RECONNECTING");
        assert_eq!(body["provider_readiness"]["state"], "UNAVAILABLE");
        assert_eq!(body["remote_blake3_available"], false);
    }

    #[test]
    fn compute_schema_is_exact_before_native_call() {
        for invalid in [
            b"{}".as_slice(),
            br#"{"fixture":"wrong"}"#,
            br#"{"fixture":"c10-abc-v1","extra":true}"#,
            br#"{"fixture":3}"#,
            br#"[]"#,
            b"{",
        ] {
            let mut bridge = bridge(fake_native());
            let response = bridge.handle(request(
                "POST",
                "/bridge/v1/compute/blake3",
                &[("content-type", "application/json")],
                invalid,
            ));
            assert_eq!(response.status, 400);
            assert_eq!(bridge.native.compute_calls.get(), 0);
        }
    }

    #[test]
    fn compute_preserves_every_execution_source_exactly() {
        for (source, reason) in [
            ("REMOTE_SUCCESS", "READY"),
            ("LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE", "TRANSPORT_LOST"),
            ("LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE", "RESOURCE_REFUSED"),
        ] {
            let mut native = fake_native();
            native.compute = Ok(remote_compute(source, reason));
            let mut bridge = bridge(native);
            let response = bridge.handle(request(
                "POST",
                "/bridge/v1/compute/blake3",
                &[("content-type", "application/json")],
                br#"{"fixture":"c10-abc-v1"}"#,
            ));
            let body = json_body(&response);
            assert_eq!(response.status, 200);
            assert_eq!(body["execution_source"], source);
            assert_eq!(body["auto_use_reason"], reason);
            assert_eq!(bridge.native.compute_calls.get(), 1);
            assert_eq!(
                bridge
                    .last_execution
                    .as_ref()
                    .expect("last execution")
                    .execution_source,
                source
            );
        }
    }

    #[test]
    fn browser_cannot_forge_authority_in_compute_body() {
        let forged = br#"{"fixture":"c10-abc-v1","authenticated":true,"controller_lease":"ACTIVE","resource_guard":"READY"}"#;
        let mut bridge = bridge(fake_native());
        let response = bridge.handle(request(
            "POST",
            "/bridge/v1/compute/blake3",
            &[("content-type", "application/json")],
            forged,
        ));
        assert_eq!(response.status, 400);
        assert_eq!(bridge.native.compute_calls.get(), 0);
    }

    #[test]
    fn host_origin_fetch_metadata_and_capability_fail_closed() {
        let cases = [
            ("host", "localhost:32123"),
            ("origin", "https://attacker.example"),
            ("sec-fetch-site", "cross-site"),
            ("x-phoneboost-bridge-token", "wrong"),
        ];
        for (name, value) in cases {
            let mut bridge = bridge(fake_native());
            let response =
                bridge.handle(request("GET", "/bridge/v1/snapshot", &[(name, value)], b""));
            assert!(matches!(response.status, 401 | 403));
            assert_eq!(bridge.native.status_calls.get(), 0);
        }

        let mut bridge = bridge(fake_native());
        let mut missing_origin = request(
            "POST",
            "/bridge/v1/compute/blake3",
            &[("content-type", "application/json")],
            br#"{"fixture":"c10-abc-v1"}"#,
        );
        missing_origin.headers.remove("origin");
        let response = bridge.handle(missing_origin);
        assert_eq!(response.status, 403);
        assert_eq!(bridge.native.compute_calls.get(), 0);
    }

    #[test]
    fn same_origin_browser_request_is_accepted() {
        let mut bridge = bridge(fake_native());
        let response = bridge.handle(request(
            "GET",
            "/bridge/v1/snapshot",
            &[
                ("origin", "http://127.0.0.1:32123"),
                ("sec-fetch-site", "same-origin"),
            ],
            b"",
        ));
        assert_eq!(response.status, 200);
    }

    #[test]
    fn unsupported_methods_paths_and_generic_forwarding_are_rejected() {
        for (method, path) in [
            ("OPTIONS", "/bridge/v1/snapshot"),
            ("POST", "/bridge/v1/snapshot"),
            ("GET", "/bridge/v1/compute/blake3"),
            ("POST", "/bridge/v1/c12"),
            ("POST", "/bridge/v1/system.status"),
            ("DELETE", "/bridge/v1/compute/blake3"),
        ] {
            let mut bridge = bridge(fake_native());
            let response = bridge.handle(request(method, path, &[], b""));
            assert!(matches!(response.status, 404 | 405));
            assert_eq!(bridge.native.status_calls.get(), 0);
            assert_eq!(bridge.native.compute_calls.get(), 0);
        }
    }

    #[test]
    fn parser_enforces_bounds_and_rejects_pipelining() {
        let host = "Host: 127.0.0.1:32123\r\n";
        let oversized_header = format!(
            "GET / HTTP/1.1\r\n{host}X-Fill: {}\r\n\r\n",
            "a".repeat(HEADER_LIMIT)
        );
        assert_eq!(
            read_request(&mut Cursor::new(oversized_header))
                .unwrap_err()
                .status,
            413
        );
        let oversized_body = format!(
            "POST /bridge/v1/compute/blake3 HTTP/1.1\r\n{host}Content-Length: {}\r\n\r\n",
            BODY_LIMIT + 1
        );
        assert_eq!(
            read_request(&mut Cursor::new(oversized_body))
                .unwrap_err()
                .status,
            413
        );
        let pipelined = format!("GET / HTTP/1.1\r\n{host}\r\nGET / HTTP/1.1\r\n{host}\r\n");
        assert_eq!(
            read_request(&mut Cursor::new(pipelined))
                .unwrap_err()
                .status,
            400
        );
    }

    #[test]
    fn parser_rejects_transfer_encoding_duplicate_headers_and_too_many_fields() {
        let transfer = b"POST /bridge/v1/compute/blake3 HTTP/1.1\r\nHost: 127.0.0.1:32123\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(
            read_request(&mut Cursor::new(transfer)).unwrap_err().status,
            400
        );
        let duplicate = b"GET / HTTP/1.1\r\nHost: 127.0.0.1:32123\r\nHost: 127.0.0.1:32123\r\n\r\n";
        assert_eq!(
            read_request(&mut Cursor::new(duplicate))
                .unwrap_err()
                .status,
            400
        );
        let mut fields = String::from("GET / HTTP/1.1\r\n");
        for index in 0..=HEADER_FIELD_LIMIT {
            fields.push_str(&format!("X-{index}: v\r\n"));
        }
        fields.push_str("\r\n");
        assert_eq!(
            read_request(&mut Cursor::new(fields)).unwrap_err().status,
            400
        );
        for invalid_length in ["+1", "-1", "1 0", ""] {
            let request = format!(
                "POST /bridge/v1/compute/blake3 HTTP/1.1\r\nHost: 127.0.0.1:32123\r\nContent-Length: {invalid_length}\r\n\r\n"
            );
            assert_eq!(
                read_request(&mut Cursor::new(request)).unwrap_err().status,
                400
            );
        }
    }

    #[test]
    fn static_path_cannot_escape_frontend_root() {
        for path in [
            "/../etc/passwd",
            "/%2e%2e/etc/passwd",
            "/static\\..\\secret",
            "/a?b",
        ] {
            assert!(safe_relative_path(path).is_none());
        }
        assert_eq!(
            safe_relative_path("/static/js/main.js"),
            Some(Path::new("static/js/main.js"))
        );
        assert_eq!(content_type(Path::new("secret.pem")), None);
    }

    #[test]
    fn response_has_no_cors_and_has_browser_hardening() {
        let response = Response::json(200, json!({"ok": true}));
        let mut bytes = Vec::new();
        write_response(&mut bytes, &response).expect("write response");
        let text = String::from_utf8(bytes).expect("response utf8");
        assert!(text.contains("Content-Security-Policy:"));
        assert!(text.contains("frame-ancestors 'none'"));
        assert!(text.contains("X-Frame-Options: DENY"));
        assert!(text.contains("Cache-Control: no-store"));
        assert!(!text.to_ascii_lowercase().contains("access-control-allow"));
    }

    #[test]
    fn json_response_limit_fails_closed() {
        let response = Response::json(200, json!({"value": "a".repeat(RESPONSE_LIMIT)}));
        assert_eq!(response.status, 503);
        assert!(response.body.len() <= RESPONSE_LIMIT);
        assert_eq!(json_body(&response)["reason"], "BRIDGE_RESPONSE_TOO_LARGE");
    }
}
