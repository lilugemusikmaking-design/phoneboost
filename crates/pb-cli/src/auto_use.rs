//! Typed preference client. One fresh connection, one command, no replay.
use std::fmt;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use rustix::net::{AddressFamily, SocketAddrUnix, SocketFlags, SocketType, connect, socket_with};
use serde_json::{Value, json};

use crate::{
    CliError, MAX_NDJSON_LINE_BYTES, canonical_control_socket, next_request_id, validate_response,
};

const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(serde::Deserialize)]
struct ControlEnvelope {
    #[serde(rename = "result")]
    _result: Option<ControlResult>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlResult {
    #[serde(rename = "enabled")]
    _enabled: bool,
    #[serde(rename = "persistence")]
    _persistence: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AutoUseError {
    Unavailable,
    InvalidResponse,
    Timeout,
    OutcomeUnknown,
    WriteFailed,
    BadRequest,
}

impl fmt::Display for AutoUseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "auto-use control unavailable",
            Self::InvalidResponse => {
                "invalid auto-use response; mutation outcome may be unknown; not retried"
            }
            Self::Timeout => "auto-use status deadline exceeded",
            Self::OutcomeUnknown => {
                "auto-use outcome unknown; read status to reconcile; not retried"
            }
            Self::WriteFailed => {
                "auto-use disabled but preference save failed; OFF across restart is not confirmed"
            }
            Self::BadRequest => "auto-use request rejected",
        })
    }
}
impl std::error::Error for AutoUseError {}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AutoUseView {
    enabled: bool,
    persistence: String,
}
impl AutoUseView {
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn persistence(&self) -> &str {
        &self.persistence
    }
}
impl fmt::Display for AutoUseView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Auto-use: {}\nPersistence: {}",
            if self.enabled { "ON" } else { "OFF" },
            self.persistence
        )?;
        if self.persistence == "ERROR" {
            write!(
                f,
                "\nWarning: saved preference is unconfirmed; restart may restore ON"
            )?;
        }
        Ok(())
    }
}

pub fn auto_use_get() -> Result<AutoUseView, AutoUseError> {
    call(None)
}
pub fn auto_use_set(enabled: bool) -> Result<AutoUseView, AutoUseError> {
    call(Some(enabled))
}

fn call(enabled: Option<bool>) -> Result<AutoUseView, AutoUseError> {
    let deadline = Instant::now() + CONTROL_TIMEOUT;
    let path = canonical_control_socket(std::env::var_os("XDG_RUNTIME_DIR"))
        .map_err(|_| AutoUseError::Unavailable)?;
    // Nonblocking AF_UNIX connect cannot stall indefinitely on a full backlog.
    // A full queue is unavailable, not a reason to retry a mutation.
    let fd = socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
        None,
    )
    .map_err(|_| AutoUseError::Unavailable)?;
    let address = SocketAddrUnix::new(&path).map_err(|_| AutoUseError::Unavailable)?;
    connect(&fd, &address).map_err(|_| AutoUseError::Unavailable)?;
    let mut stream = UnixStream::from(fd);
    stream
        .set_nonblocking(false)
        .map_err(|_| AutoUseError::Unavailable)?;
    exchange(&mut stream, enabled, deadline)
}

fn remaining(stream: &UnixStream, deadline: Instant) -> std::io::Result<()> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(std::io::ErrorKind::TimedOut.into());
    }
    stream.set_read_timeout(Some(remaining))?;
    stream.set_write_timeout(Some(remaining))
}

fn exchange(
    stream: &mut UnixStream,
    enabled: Option<bool>,
    deadline: Instant,
) -> Result<AutoUseView, AutoUseError> {
    let id = next_request_id();
    let (method, params) = match enabled {
        Some(enabled) => ("auto_use.set", json!({"enabled":enabled})),
        None => ("auto_use.get", json!({})),
    };
    let mut bytes = serde_json::to_vec(&json!({"api":1,"id":id,"method":method,"params":params}))
        .map_err(|_| AutoUseError::BadRequest)?;
    bytes.push(b'\n');
    let io_error = |error: std::io::Error| {
        if enabled.is_some() {
            AutoUseError::OutcomeUnknown
        } else if matches!(
            error.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        ) {
            AutoUseError::Timeout
        } else {
            AutoUseError::Unavailable
        }
    };
    let mut sent = 0;
    while sent < bytes.len() {
        remaining(stream, deadline).map_err(io_error)?;
        let count = stream.write(&bytes[sent..]).map_err(io_error)?;
        if count == 0 {
            return Err(io_error(std::io::Error::from(
                std::io::ErrorKind::WriteZero,
            )));
        }
        sent += count;
    }
    let mut response = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        remaining(stream, deadline).map_err(io_error)?;
        let count = stream.read(&mut chunk).map_err(io_error)?;
        if Instant::now() >= deadline {
            return Err(io_error(std::io::ErrorKind::TimedOut.into()));
        }
        if count == 0 {
            return Err(if enabled.is_some() {
                AutoUseError::OutcomeUnknown
            } else {
                AutoUseError::InvalidResponse
            });
        }
        response.extend_from_slice(&chunk[..count]);
        if response.len() > MAX_NDJSON_LINE_BYTES {
            return Err(AutoUseError::InvalidResponse);
        }
        if let Some(end) = response.iter().position(|b| *b == b'\n') {
            if end + 1 != response.len() {
                return Err(AutoUseError::InvalidResponse);
            }
            // Preserve duplicate-field evidence before serde_json::Value can
            // collapse it. No ambiguous preference reply becomes confirmed.
            serde_json::from_slice::<ControlEnvelope>(&response[..end])
                .map_err(|_| AutoUseError::InvalidResponse)?;
            let value = validate_response(&response[..end], &id).map_err(|error| match error {
                CliError::Remote { code, .. } => match code.as_str() {
                    "LOCAL_UNSUPPORTED_METHOD" => AutoUseError::Unavailable,
                    "LOCAL_STATE_WRITE_FAILED" => AutoUseError::WriteFailed,
                    "LOCAL_BAD_REQUEST" => AutoUseError::BadRequest,
                    _ => AutoUseError::InvalidResponse,
                },
                _ => AutoUseError::InvalidResponse,
            })?;
            return parse_view(value, enabled);
        }
    }
}

fn parse_view(value: Value, desired: Option<bool>) -> Result<AutoUseView, AutoUseError> {
    let invalid = AutoUseError::InvalidResponse;
    let object = value.as_object().ok_or(invalid)?;
    let enabled = object
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or(invalid)?;
    let persistence = object
        .get("persistence")
        .and_then(Value::as_str)
        .ok_or(invalid)?;
    if object.len() != 2
        || !matches!(persistence, "DEFAULT" | "SAVED" | "ERROR")
        || (persistence == "DEFAULT" && !enabled)
        || (persistence == "ERROR" && enabled)
        || desired.is_some_and(|wanted| wanted != enabled || persistence != "SAVED")
    {
        return Err(invalid);
    }
    Ok(AutoUseView {
        enabled,
        persistence: persistence.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::thread;

    #[test]
    fn typed_control_views_reject_false_success() {
        for enabled in [false, true] {
            assert!(
                parse_view(
                    json!({"enabled":enabled,"persistence":"SAVED"}),
                    Some(enabled)
                )
                .is_ok()
            );
            assert!(
                parse_view(
                    json!({"enabled":enabled,"persistence":"SAVED"}),
                    Some(!enabled)
                )
                .is_err()
            );
        }
        for value in [
            json!({}),
            json!({"enabled":1,"persistence":"SAVED"}),
            json!({"enabled":true,"persistence":"ERROR"}),
            json!({"enabled":false,"persistence":"DEFAULT"}),
            json!({"enabled":true,"persistence":"READY"}),
            json!({"enabled":false,"persistence":"SAVED","extra":true}),
        ] {
            assert!(parse_view(value, None).is_err());
        }
    }

    #[test]
    fn control_exact_request_and_no_replay_after_lost_reply() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let mut line = String::new();
            std::io::BufReader::new(&mut server)
                .read_line(&mut line)
                .unwrap();
            let value: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(value["method"], "auto_use.set");
            assert_eq!(value["params"], json!({"enabled":false}));
            server.shutdown(std::net::Shutdown::Write).unwrap();
            server
                .set_read_timeout(Some(Duration::from_millis(50)))
                .unwrap();
            assert!(server.read(&mut [0_u8; 1]).is_err());
        });
        assert_eq!(
            exchange(
                &mut client,
                Some(false),
                Instant::now() + Duration::from_secs(1)
            ),
            Err(AutoUseError::OutcomeUnknown)
        );
        worker.join().unwrap();
    }

    #[test]
    fn control_deadline_is_absolute_under_trickling() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            for _ in 0..30 {
                if server.write_all(b" ").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
        let start = Instant::now();
        assert_eq!(
            exchange(&mut client, None, start + Duration::from_millis(60)),
            Err(AutoUseError::Timeout)
        );
        assert!(start.elapsed() < Duration::from_millis(250));
        drop(client);
        worker.join().unwrap();
    }

    #[test]
    fn control_validates_terminal_envelope_and_sanitizes_errors() {
        for (result, expected) in [
            (json!({"enabled":false,"persistence":"SAVED"}), None),
            (
                json!({"enabled":true,"persistence":"SAVED"}),
                Some(AutoUseError::InvalidResponse),
            ),
        ] {
            let (mut client, mut server) = UnixStream::pair().unwrap();
            let worker = thread::spawn(move || {
                let mut line = String::new();
                std::io::BufReader::new(&mut server)
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                writeln!(
                    server,
                    "{}",
                    json!({"api":1,"id":request["id"],"ok":true,"result":result})
                )
                .unwrap();
            });
            let response = exchange(
                &mut client,
                Some(false),
                Instant::now() + Duration::from_secs(1),
            );
            assert_eq!(response.err(), expected);
            worker.join().unwrap();
        }
    }

    #[test]
    fn control_closed_errors_never_forward_daemon_text() {
        for (code, expected) in [
            ("LOCAL_UNSUPPORTED_METHOD", AutoUseError::Unavailable),
            ("LOCAL_STATE_WRITE_FAILED", AutoUseError::WriteFailed),
            ("LOCAL_BAD_REQUEST", AutoUseError::BadRequest),
            ("UNEXPECTED_CODE", AutoUseError::InvalidResponse),
        ] {
            let (mut client, mut server) = UnixStream::pair().unwrap();
            let worker = thread::spawn(move || {
                let mut line = String::new();
                std::io::BufReader::new(&mut server)
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                writeln!(server, "{}", json!({"api":1,"id":request["id"],"ok":false,"error":{"code":code,"scope":"REQUEST","message_safe":"PRIVATE_PATH_SENTINEL"}})).unwrap();
            });
            let error = exchange(
                &mut client,
                Some(false),
                Instant::now() + Duration::from_secs(1),
            )
            .unwrap_err();
            assert_eq!(error, expected);
            assert!(!error.to_string().contains("PRIVATE_PATH_SENTINEL"));
            worker.join().unwrap();
        }
        assert!(
            serde_json::from_str::<ControlEnvelope>(
                r#"{"result":{"enabled":false,"enabled":true,"persistence":"SAVED"}}"#
            )
            .is_err()
        );
    }
}
