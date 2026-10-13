//! Native requester for the locked C12 import/consumer contracts.
//! One connection owns the handle; dropping it invokes daemon cleanup. Mutating
//! requests are never retried, including partial writes or missing responses.
use std::fmt;
use std::io::{IoSlice, Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::BorrowedFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
use serde_json::{Value, json};

use crate::{
    BLAKE3_OP_ID, CliError, IO_TIMEOUT, MAX_NDJSON_LINE_BYTES, REMOTE_SUCCESS, connect_local_api,
    next_request_id, required_string, valid_auto_use_reason, valid_execution_source,
    validate_response,
};

const MAX_IMPORT_BYTES: u64 = 134_217_728;
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Terminal result only. No private handle or source descriptor is retained.
#[derive(Eq, PartialEq)]
pub struct ImportedComputeView {
    input_bytes: u64,
    digest_blake3_hex: String,
    execution_source: String,
    auto_use_reason: String,
}

impl ImportedComputeView {
    pub fn input_bytes(&self) -> u64 {
        self.input_bytes
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
    pub fn exit_code(&self) -> i32 {
        if self.execution_source == REMOTE_SUCCESS {
            0
        } else {
            3
        }
    }
}

// Debug output is safe for diagnostics; the requested digest is only exposed
// explicitly through its accessor or operator-facing Display result.
impl fmt::Debug for ImportedComputeView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportedComputeView")
            .field("execution_source", &self.execution_source)
            .field("auto_use_reason", &self.auto_use_reason)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for ImportedComputeView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "BLAKE3 input: PRIVATE_IMPORT_V1")?;
        writeln!(f, "Input bytes: {}", self.input_bytes)?;
        writeln!(f, "BLAKE3 digest: {}", self.digest_blake3_hex)?;
        writeln!(f, "Execution source: {}", self.execution_source)?;
        write!(f, "Auto-use reason: {}", self.auto_use_reason)
    }
}

/// Compute BLAKE3 over a caller-opened descriptor using only approved C12.
/// The daemon enforces descriptor ownership/type, copy limits and authority.
/// No paths, payload copies, remote destinations or placement overrides enter
/// this client. Readiness and terminal response each have a 60-second bound.
pub fn compute_blake3_import(source: BorrowedFd<'_>) -> Result<ImportedComputeView, CliError> {
    let stream = connect_local_api(IO_TIMEOUT)?;
    compute_over_stream(stream, source, IO_TIMEOUT)
}

fn compute_over_stream(
    mut stream: UnixStream,
    source: BorrowedFd<'_>,
    timeout: Duration,
) -> Result<ImportedComputeView, CliError> {
    let ready_deadline = Instant::now() + timeout;
    let created = request(
        &mut stream,
        "imports.create",
        json!({}),
        Some(source),
        ready_deadline,
    )?;
    let created = created.as_object().ok_or(CliError::MalformedResponse)?;
    let handle = required_string(created, "import_handle")?;
    if created.len() != 2
        || created.get("state").and_then(Value::as_str) != Some("RECEIVING")
        || !lower_hex_64(&handle)
    {
        return Err(CliError::MalformedResponse);
    }
    let input_bytes = loop {
        if Instant::now() >= ready_deadline {
            return Err(CliError::ImportTimedOut);
        }
        let response = request(
            &mut stream,
            "imports.get",
            json!({"import_handle": handle}),
            None,
            ready_deadline,
        )
        .map_err(|error| {
            if Instant::now() >= ready_deadline {
                CliError::ImportTimedOut
            } else {
                error
            }
        })?;
        let status = response.as_object().ok_or(CliError::MalformedResponse)?;
        match status.get("state").and_then(Value::as_str) {
            Some("RECEIVING") if status.len() == 1 => {
                std::thread::sleep(
                    POLL_INTERVAL.min(ready_deadline.saturating_duration_since(Instant::now())),
                );
            }
            Some("READY") if status.len() == 2 => {
                let bytes = status
                    .get("input_bytes")
                    .and_then(Value::as_u64)
                    .ok_or(CliError::MalformedResponse)?;
                if !(1..=MAX_IMPORT_BYTES).contains(&bytes) {
                    return Err(CliError::ImportNotReady);
                }
                break bytes;
            }
            Some("CANCELLED" | "CONSUMED" | "EXPIRED") if status.len() == 1 => {
                return Err(CliError::ImportNotReady);
            }
            _ => return Err(CliError::MalformedResponse),
        }
    };
    // A lost or malformed terminal reply cannot tell us whether the single-use
    // consume point was crossed. Never re-import, resubmit or synthesize success.
    let result = request(
        &mut stream,
        "compute.submit_import",
        json!({"op_id": BLAKE3_OP_ID, "import_handle": handle}),
        None,
        Instant::now() + timeout,
    )
    .map_err(|error| match error {
        CliError::Remote { .. } => error,
        _ => CliError::OutcomeUnknown,
    })?;
    terminal_view(result, input_bytes).map_err(|_| CliError::OutcomeUnknown)
}

fn lower_hex_64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn terminal_view(response: Value, input_bytes: u64) -> Result<ImportedComputeView, CliError> {
    let result = response.as_object().ok_or(CliError::MalformedResponse)?;
    let digest = required_string(result, "digest_blake3_hex")?;
    let source = required_string(result, "execution_source")?;
    let reason = required_string(result, "auto_use_reason")?;
    if result.len() != 7
        || result.get("state").and_then(Value::as_str) != Some("COMPLETED")
        || result.get("op_id").and_then(Value::as_str) != Some(BLAKE3_OP_ID)
        || result.get("input_kind").and_then(Value::as_str) != Some("PRIVATE_IMPORT_V1")
        || result.get("input_bytes").and_then(Value::as_u64) != Some(input_bytes)
        || !lower_hex_64(&digest)
        || !valid_execution_source(&source)
        || !valid_auto_use_reason(&reason)
        || (source == REMOTE_SUCCESS && reason != "READY")
    {
        return Err(CliError::MalformedResponse);
    }
    Ok(ImportedComputeView {
        input_bytes,
        digest_blake3_hex: digest,
        execution_source: source,
        auto_use_reason: reason,
    })
}

fn set_remaining(stream: &UnixStream, deadline: Instant) -> Result<(), CliError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(CliError::IoFailed);
    }
    stream
        .set_read_timeout(Some(remaining))
        .map_err(|_| CliError::IoFailed)?;
    stream
        .set_write_timeout(Some(remaining))
        .map_err(|_| CliError::IoFailed)
}

fn request(
    stream: &mut UnixStream,
    method: &'static str,
    params: Value,
    source: Option<BorrowedFd<'_>>,
    deadline: Instant,
) -> Result<Value, CliError> {
    let id = next_request_id();
    let mut bytes = serde_json::to_vec(&json!({"api":1,"id":id,"method":method,"params":params}))
        .map_err(|_| CliError::MalformedResponse)?;
    bytes.push(b'\n');
    set_remaining(stream, deadline)?;
    let mut sent = if let Some(source) = source {
        let fds = [source];
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut ancillary = SendAncillaryBuffer::new(&mut space);
        if !ancillary.push(SendAncillaryMessage::ScmRights(&fds)) {
            return Err(CliError::IoFailed);
        }
        let count = sendmsg(
            &*stream,
            &[IoSlice::new(&bytes)],
            &mut ancillary,
            SendFlags::NOSIGNAL,
        )
        .map_err(|_| CliError::IoFailed)?;
        if count == 0 {
            return Err(CliError::IoFailed);
        }
        count
    } else {
        0
    };
    // Complete a partial byte write without sending the descriptor twice.
    while sent < bytes.len() {
        set_remaining(stream, deadline)?;
        let count = stream
            .write(&bytes[sent..])
            .map_err(|_| CliError::IoFailed)?;
        if count == 0 {
            return Err(CliError::IoFailed);
        }
        sent += count;
    }
    let mut line = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        set_remaining(stream, deadline)?;
        let count = stream.read(&mut chunk).map_err(|_| CliError::IoFailed)?;
        if count == 0 {
            return Err(CliError::MalformedResponse);
        }
        if line.len() + count > MAX_NDJSON_LINE_BYTES {
            return Err(CliError::ResponseTooLarge);
        }
        line.extend_from_slice(&chunk[..count]);
        if let Some(newline) = line.iter().position(|b| *b == b'\n') {
            if newline + 1 != line.len() {
                return Err(CliError::MalformedResponse);
            }
            return match validate_response(&line[..newline], &id) {
                Err(CliError::Remote { code, .. })
                    if matches!(
                        code.as_str(),
                        "LOCAL_BAD_REQUEST"
                            | "LOCAL_UNSUPPORTED_METHOD"
                            | "IMPORT_NOT_FOUND"
                            | "IMPORT_LIMIT_REACHED"
                            | "IMPORT_SOURCE_REJECTED"
                            | "IMPORT_RECEIVING_TIMEOUT"
                            | "IMPORT_CLEANUP_UNCONFIRMED"
                            | "IMPORT_UNAVAILABLE"
                            | "IMPORT_NOT_READY"
                            | "IMPORT_INPUT_REJECTED"
                    ) =>
                {
                    Err(CliError::Remote {
                        code,
                        message_safe: "C12 import request rejected".to_owned(),
                    })
                }
                Err(CliError::Remote { .. }) => Err(CliError::MalformedResponse),
                result => result,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags, recvmsg};
    use std::fs::File;
    use std::io::IoSliceMut;
    use std::os::fd::AsFd;
    use std::thread;

    const HANDLE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const DIGEST: &str = "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85";

    fn receive(server: &mut UnixStream, method: &str, descriptors: usize) -> Value {
        let mut data = [0_u8; 4096];
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
        let mut ancillary = RecvAncillaryBuffer::new(&mut space);
        let received = recvmsg(
            &*server,
            &mut [IoSliceMut::new(&mut data)],
            &mut ancillary,
            RecvFlags::CMSG_CLOEXEC,
        )
        .unwrap();
        assert!(!received.flags.contains(ReturnFlags::CTRUNC));
        let count: usize = ancillary
            .drain()
            .map(|message| match message {
                RecvAncillaryMessage::ScmRights(fds) => fds.count(),
                _ => panic!("unexpected ancillary data"),
            })
            .sum();
        assert_eq!(count, descriptors);
        let mut line = data[..received.bytes].to_vec();
        while !line.ends_with(b"\n") {
            let mut byte = [0];
            server.read_exact(&mut byte).unwrap();
            line.push(byte[0]);
        }
        let value: Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(value["method"], method);
        assert_eq!(value["api"], 1);
        let expected = match method {
            "imports.create" => json!({}),
            "imports.get" => json!({"import_handle": HANDLE}),
            "compute.submit_import" => json!({"import_handle": HANDLE,"op_id": BLAKE3_OP_ID}),
            _ => panic!("unexpected request"),
        };
        assert_eq!(value["params"], expected);
        value["id"].clone()
    }

    fn reply(server: &mut UnixStream, id: Value, result: Value) {
        writeln!(
            server,
            "{}",
            json!({"api":1,"id":id,"ok":true,"result":result})
        )
        .unwrap();
    }

    fn terminal(source: &str, reason: &str) -> Value {
        json!({"state":"COMPLETED","op_id":BLAKE3_OP_ID,"input_kind":"PRIVATE_IMPORT_V1",
            "input_bytes":3,"digest_blake3_hex":DIGEST,"execution_source":source,"auto_use_reason":reason})
    }

    fn run_server(
        server: impl FnOnce(UnixStream) + Send + 'static,
    ) -> Result<ImportedComputeView, CliError> {
        let (client, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let worker = thread::spawn(move || server(peer));
        let source = File::open("/dev/null").unwrap();
        let result = compute_over_stream(client, source.as_fd(), Duration::from_secs(2));
        worker.join().unwrap();
        result
    }

    fn ready(server: &mut UnixStream, bytes: u64) {
        let id = receive(server, "imports.create", 1);
        reply(
            server,
            id,
            json!({"state":"RECEIVING","import_handle":HANDLE}),
        );
        let id = receive(server, "imports.get", 0);
        reply(server, id, json!({"state":"RECEIVING"}));
        let id = receive(server, "imports.get", 0);
        reply(server, id, json!({"state":"READY","input_bytes":bytes}));
    }

    #[test]
    fn same_connection_one_descriptor_and_truthful_terminal_results() {
        for (source, reason, exit_code) in [
            (REMOTE_SUCCESS, "READY", 0),
            ("LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE", "OFF", 3),
            ("LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE", "TRANSPORT_LOST", 3),
        ] {
            let view = run_server(move |mut server| {
                ready(&mut server, 3);
                let id = receive(&mut server, "compute.submit_import", 0);
                reply(&mut server, id, terminal(source, reason));
                assert_eq!(server.read(&mut [0]).unwrap(), 0);
            })
            .unwrap();
            assert_eq!(view.exit_code(), exit_code);
            assert_eq!(view.input_bytes(), 3);
            assert_eq!(view.digest_blake3_hex(), DIGEST);
            assert_eq!(view.execution_source(), source);
            assert_eq!(view.auto_use_reason(), reason);
            assert!(!format!("{view:?}").contains(DIGEST));
            assert!(!view.to_string().contains(HANDLE));
        }
    }

    #[test]
    fn lost_terminal_response_is_unknown_and_never_replayed() {
        assert_eq!(
            run_server(|mut server| {
                ready(&mut server, 3);
                receive(&mut server, "compute.submit_import", 0);
                server.shutdown(std::net::Shutdown::Write).unwrap();
                assert_eq!(server.read(&mut [0]).unwrap(), 0);
            }),
            Err(CliError::OutcomeUnknown)
        );
    }

    #[test]
    fn malformed_terminal_response_is_unknown_and_never_replayed() {
        assert_eq!(
            run_server(|mut server| {
                ready(&mut server, 3);
                let id = receive(&mut server, "compute.submit_import", 0);
                reply(&mut server, id, terminal(REMOTE_SUCCESS, "TRANSPORT_LOST"));
                assert_eq!(server.read(&mut [0]).unwrap(), 0);
            }),
            Err(CliError::OutcomeUnknown)
        );
    }

    #[test]
    fn empty_and_oversized_inputs_disconnect_without_compute() {
        for bytes in [0, MAX_IMPORT_BYTES + 1] {
            assert_eq!(
                run_server(move |mut server| {
                    ready(&mut server, bytes);
                    assert_eq!(server.read(&mut [0]).unwrap(), 0);
                }),
                Err(CliError::ImportNotReady)
            );
        }
    }

    #[test]
    fn invalid_create_or_status_never_reaches_compute() {
        for created in [
            json!({"state":"READY","import_handle":HANDLE}),
            json!({"state":"RECEIVING","import_handle":"../private"}),
            json!({"state":"RECEIVING","import_handle":HANDLE,"extra":true}),
        ] {
            assert_eq!(
                run_server(move |mut server| {
                    let id = receive(&mut server, "imports.create", 1);
                    reply(&mut server, id, created);
                    assert_eq!(server.read(&mut [0]).unwrap(), 0);
                }),
                Err(CliError::MalformedResponse)
            );
        }
        for status in [
            json!({"state":"READY","input_bytes":"3"}),
            json!({"state":"READY","input_bytes":3,"extra":true}),
            json!({"state":"UNKNOWN"}),
        ] {
            assert_eq!(
                run_server(move |mut server| {
                    let id = receive(&mut server, "imports.create", 1);
                    reply(
                        &mut server,
                        id,
                        json!({"state":"RECEIVING","import_handle":HANDLE}),
                    );
                    let id = receive(&mut server, "imports.get", 0);
                    reply(&mut server, id, status);
                    assert_eq!(server.read(&mut [0]).unwrap(), 0);
                }),
                Err(CliError::MalformedResponse)
            );
        }
    }

    #[test]
    fn terminal_shape_and_bounds_are_strict() {
        for (key, value) in [
            ("state", json!("RUNNING")),
            ("op_id", json!("shell")),
            ("input_kind", json!("PATH")),
            ("input_bytes", json!(4)),
            ("digest_blake3_hex", json!("A".repeat(64))),
            ("execution_source", json!("LOCAL")),
            ("auto_use_reason", json!("invented")),
            ("import_handle", json!(HANDLE)),
        ] {
            let mut result = terminal(REMOTE_SUCCESS, "READY");
            result[key] = value;
            assert_eq!(terminal_view(result, 3), Err(CliError::MalformedResponse));
        }
        for bytes in [1, MAX_IMPORT_BYTES] {
            let mut result = terminal(REMOTE_SUCCESS, "READY");
            result["input_bytes"] = json!(bytes);
            assert_eq!(terminal_view(result, bytes).unwrap().input_bytes(), bytes);
        }
    }

    #[test]
    fn rejection_diagnostics_do_not_echo_untrusted_messages() {
        let error = run_server(|mut server| {
            let id = receive(&mut server, "imports.create", 1);
            writeln!(
                server,
                "{}",
                json!({"api":1,"id":id,"ok":false,"error":{
                "code":"IMPORT_SOURCE_REJECTED","message_safe":"secret path /home/private payload",
                "scope":"REQUEST","state_changed":false}})
            )
            .unwrap();
        })
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "IMPORT_SOURCE_REJECTED: C12 import request rejected"
        );
    }

    #[test]
    fn readiness_deadline_closes_without_submission() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let id = receive(&mut server, "imports.create", 1);
            reply(
                &mut server,
                id,
                json!({"state":"RECEIVING","import_handle":HANDLE}),
            );
            receive(&mut server, "imports.get", 0);
            assert_eq!(server.read(&mut [0]).unwrap(), 0);
        });
        let source = File::open("/dev/null").unwrap();
        let started = Instant::now();
        let error =
            compute_over_stream(client, source.as_fd(), Duration::from_millis(150)).unwrap_err();
        assert_eq!(error, CliError::ImportTimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
        worker.join().unwrap();
    }

    #[test]
    fn terminal_timeout_is_unknown_and_closes_without_retry() {
        let (client, mut server) = UnixStream::pair().unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let worker = thread::spawn(move || {
            ready(&mut server, 3);
            receive(&mut server, "compute.submit_import", 0);
            assert_eq!(server.read(&mut [0]).unwrap(), 0);
        });
        let source = File::open("/dev/null").unwrap();
        assert_eq!(
            compute_over_stream(client, source.as_fd(), Duration::from_millis(200)),
            Err(CliError::OutcomeUnknown)
        );
        worker.join().unwrap();
    }

    #[test]
    fn trickling_response_cannot_extend_absolute_deadline() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            receive(&mut server, "imports.get", 0);
            for _ in 0..50 {
                if server.write_all(b" ").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
        });
        let started = Instant::now();
        let result = request(
            &mut client,
            "imports.get",
            json!({"import_handle":HANDLE}),
            None,
            started + Duration::from_millis(150),
        );
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_millis(750));
        drop(client);
        worker.join().unwrap();
    }
}
