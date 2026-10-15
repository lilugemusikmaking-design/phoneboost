//! Approved P14 ingress: authenticate headers, reserve, stream, seal, submit once.
use super::*;
use rustix::fs::{MemfdFlags, Mode, SealFlags, fchmod, fcntl_add_seals, memfd_create};
use std::io;
use std::os::fd::AsFd;
use std::time::Instant;

pub(super) const PATH: &str = "/bridge/v1/compute/blake3-file";
pub(super) const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30);
const INPUT_LIMIT: usize = 134_217_728;
const COPY_BUFFER: usize = 65_536;

pub(super) struct FileResult {
    pub input_bytes: u64,
    pub compute: NativeCompute,
}

pub(super) trait UploadRead: Read {
    fn finish(&mut self) -> Result<(), Response>;
}

/// Socket inactivity bounds alone allow a slow trickle to live forever.
/// Each actual syscall gets min(idle bound, absolute remaining budget).
pub(super) struct DeadlineIo<'a> {
    stream: &'a mut TcpStream,
    deadline: Instant,
}
impl<'a> DeadlineIo<'a> {
    pub fn new(stream: &'a mut TcpStream, budget: Duration) -> Self {
        Self {
            stream,
            deadline: Instant::now() + budget,
        }
    }
    fn remaining(&self) -> io::Result<Duration> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(remaining.min(SOCKET_TIMEOUT))
    }
}
impl Read for DeadlineIo<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(self.remaining()?))?;
        self.stream.read(buffer)
    }
}
impl UploadRead for DeadlineIo<'_> {
    fn finish(&mut self) -> Result<(), Response> {
        self.remaining()
            .map_err(|_| control_error(408, "UPLOAD_TIMEOUT"))?;
        self.stream
            .set_nonblocking(true)
            .map_err(|_| control_error(400, "BAD_REQUEST"))?;
        let peek = self.stream.peek(&mut [0u8]);
        self.stream
            .set_nonblocking(false)
            .map_err(|_| control_error(400, "BAD_REQUEST"))?;
        match peek {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(()),
            Ok(0) => Err(control_error(400, "BAD_REQUEST")),
            Ok(_) => Err(Response::error(400, "REQUEST_PIPELINING_UNSUPPORTED")),
            Err(_) => Err(control_error(400, "BAD_REQUEST")),
        }
    }
}

#[cfg(test)]
impl<T: AsRef<[u8]>> UploadRead for io::Cursor<T> {
    fn finish(&mut self) -> Result<(), Response> {
        if self.position() < self.get_ref().as_ref().len() as u64 {
            Err(Response::error(400, "REQUEST_PIPELINING_UNSUPPORTED"))
        } else {
            Ok(())
        }
    }
}
impl Write for DeadlineIo<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.write(buffer)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.remaining()?;
        self.stream.flush()
    }
}

impl<N: NativeBackend> Bridge<N> {
    pub(super) fn file_request(
        &mut self,
        stream: &mut impl UploadRead,
        request: Request,
        length: usize,
    ) -> Response {
        if !self.valid_browser_boundary(&request) {
            return Response::error(403, "BROWSER_BOUNDARY_REJECTED");
        }
        if request.method != "POST" {
            return Response::error(405, "METHOD_NOT_ALLOWED");
        }
        if !self.valid_token(&request) {
            return Response::error(401, "BRIDGE_CAPABILITY_REQUIRED");
        }
        if let Err(response) = validate_headers(&request, length) {
            return response;
        }
        let lanes = Arc::clone(&self.lanes);
        let Ok(_permit) = lanes.compute.try_lock() else {
            return control_error(503, "BUSY");
        };
        let input = match receive(stream, &request.body, length) {
            Ok(file) => file,
            Err(response) => return response,
        };
        if let Err(response) = stream.finish() {
            return response;
        }
        let result = match self.native.compute_file(input.as_fd()) {
            Ok(value) if value.input_bytes == length as u64 => value,
            Ok(_) => return control_error(504, "OUTCOME_UNKNOWN"),
            Err(error) => return native_error(error),
        };
        // Deliberately do not overwrite the fixture-specific snapshot history.
        Response::json(
            200,
            json!({
                "state":"COMPLETED", "op_id":"pb.native.blake3/1",
                "input_kind":"PRIVATE_IMPORT_V1", "input_bytes":result.input_bytes,
                "digest_blake3_hex":result.compute.digest_blake3_hex,
                "execution_source":result.compute.execution_source,
                "auto_use_reason":result.compute.auto_use_reason,
            }),
        )
    }
}

fn validate_headers(request: &Request, length: usize) -> Result<(), Response> {
    if !request.headers.contains_key("content-length") {
        return Err(control_error(411, "CONTENT_LENGTH_REQUIRED"));
    }
    if length > INPUT_LIMIT {
        return Err(control_error(413, "INPUT_TOO_LARGE"));
    }
    if length == 0
        || ["content-encoding", "expect"]
            .iter()
            .any(|name| request.headers.contains_key(*name))
    {
        return Err(control_error(400, "BAD_REQUEST"));
    }
    if !request
        .headers
        .get("content-type")
        .is_some_and(|v| v.eq_ignore_ascii_case("application/octet-stream"))
    {
        return Err(control_error(415, "CONTENT_TYPE_REQUIRED"));
    }
    Ok(())
}

fn native_error(error: pb_cli::CliError) -> Response {
    match error {
        pb_cli::CliError::ImportTimedOut => control_error(504, "IMPORT_TIMEOUT"),
        pb_cli::CliError::OutcomeUnknown => control_error(504, "OUTCOME_UNKNOWN"),
        _ => control_error(503, "NATIVE_REQUEST_FAILED"),
    }
}

fn storage_error(_: impl std::fmt::Debug) -> Response {
    control_error(503, "INPUT_STORAGE_UNAVAILABLE")
}

fn receive(stream: &mut impl Read, prefix: &[u8], length: usize) -> Result<File, Response> {
    let fd = memfd_create(
        "phoneboost-browser-input",
        MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
    )
    .map_err(storage_error)?;
    fchmod(&fd, Mode::RUSR | Mode::WUSR).map_err(storage_error)?;
    let mut file = File::from(fd);
    copy_exact(stream, &mut file, prefix, length)?;
    seal_input(&file)?;
    Ok(file)
}

fn seal_input(file: &File) -> Result<(), Response> {
    fcntl_add_seals(
        file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .map_err(storage_error)
}

fn copy_exact(
    stream: &mut impl Read,
    sink: &mut impl Write,
    prefix: &[u8],
    length: usize,
) -> Result<(), Response> {
    if length == 0 || length > INPUT_LIMIT || prefix.len() > length {
        return Err(control_error(400, "BAD_REQUEST"));
    }
    sink.write_all(prefix).map_err(storage_error)?;
    let mut remaining = length - prefix.len();
    let mut buffer = [0u8; COPY_BUFFER];
    while remaining != 0 {
        let count = stream
            .read(&mut buffer[..remaining.min(COPY_BUFFER)])
            .map_err(|error| {
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) {
                    control_error(408, "UPLOAD_TIMEOUT")
                } else {
                    control_error(400, "BAD_REQUEST")
                }
            })?;
        if count == 0 {
            return Err(control_error(400, "BAD_REQUEST"));
        }
        sink.write_all(&buffer[..count]).map_err(storage_error)?;
        remaining -= count;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{bridge, fake_native, request};
    use super::*;
    use rustix::fs::{fcntl_get_seals, fstat};
    use std::io::{Cursor, Read};

    fn bulk(headers: &[(&str, &str)], body: &[u8], length: usize) -> Request {
        let mut req = request("POST", PATH, headers, body);
        req.headers
            .entry("content-type".into())
            .or_insert("application/octet-stream".into());
        req.headers
            .insert("content-length".into(), length.to_string());
        req
    }

    struct NoRead;
    impl Read for NoRead {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("must reject before payload read");
        }
    }
    impl UploadRead for NoRead {
        fn finish(&mut self) -> Result<(), Response> {
            panic!("no upload admitted");
        }
    }

    #[test]
    fn rejection_precedes_bulk_read_or_native_call() {
        for (headers, length, status) in [
            (vec![("host", "attacker.invalid")], 1, 403),
            (vec![("origin", "https://attacker.invalid")], 1, 403),
            (vec![("sec-fetch-site", "cross-site")], 1, 403),
            (vec![("x-phoneboost-bridge-token", "wrong")], 1, 401),
            (vec![], 0, 400),
            (vec![], INPUT_LIMIT + 1, 413),
            (vec![("content-type", "multipart/form-data")], 1, 415),
            (
                vec![("content-type", "application/octet-stream; x=y")],
                1,
                415,
            ),
            (vec![("content-type", "application/json")], 1, 415),
            (vec![("expect", "100-continue")], 1, 400),
            (vec![("content-encoding", "gzip")], 1, 400),
        ] {
            let mut bridge = bridge(fake_native());
            assert_eq!(
                bridge
                    .file_request(&mut NoRead, bulk(&headers, b"", length), length)
                    .status,
                status
            );
            assert!(bridge.lanes.compute.try_lock().is_ok());
        }
    }

    #[test]
    fn busy_precedes_upload_and_control_lane_remains_independent() {
        let mut bridge = bridge(fake_native());
        let lanes = Arc::clone(&bridge.lanes);
        let permit = lanes.compute.lock().unwrap();
        assert_eq!(
            bridge
                .file_request(&mut NoRead, bulk(&[], b"", 3), 3)
                .status,
            503
        );
        assert_eq!(
            bridge
                .handle(request(
                    "POST",
                    "/bridge/v1/auto-use",
                    &[("content-type", "application/json")],
                    br#"{"enabled":false}"#
                ))
                .status,
            200
        );
        drop(permit);
        assert!(lanes.compute.try_lock().is_ok());
    }

    #[test]
    fn received_memfd_is_exact_private_and_fully_sealed() {
        use std::os::unix::fs::FileExt;
        let file = receive(&mut Cursor::new(b"bc"), b"a", 3).unwrap();
        let mut bytes = [0; 3];
        file.read_exact_at(&mut bytes, 0).unwrap();
        assert_eq!(&bytes, b"abc");
        assert_eq!(fstat(&file).unwrap().st_mode & 0o777, 0o600);
        assert_eq!(fstat(&file).unwrap().st_nlink, 0);
        assert!(
            fcntl_get_seals(&file)
                .unwrap()
                .contains(SealFlags::WRITE | SealFlags::SHRINK | SealFlags::GROW | SealFlags::SEAL)
        );
        assert!(file.write_at(b"x", 0).is_err());
        assert!(file.set_len(4).is_err());
    }

    #[test]
    fn input_boundaries_are_streamed_without_input_sized_buffers() {
        let mut file = receive(
            &mut io::repeat(7).take(INPUT_LIMIT as u64),
            &[],
            INPUT_LIMIT,
        )
        .unwrap();
        assert_eq!(file.metadata().unwrap().len(), INPUT_LIMIT as u64);
        assert!(file.write_all(b"x").is_err());
        let file = receive(&mut Cursor::new(b"x"), &[], 1).unwrap();
        assert_eq!(file.metadata().unwrap().len(), 1);
        assert!(receive(&mut Cursor::new(b""), b"ab", 1).is_err());
        assert!(receive(&mut Cursor::new(b"x"), &[], 2).is_err());
    }

    #[test]
    fn incomplete_upload_storage_failure_and_timeout_do_not_submit() {
        struct Fail;
        impl Write for Fail {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::StorageFull.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        impl Read for Fail {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::TimedOut.into())
            }
        }
        impl UploadRead for Fail {
            fn finish(&mut self) -> Result<(), Response> {
                panic!("failed body");
            }
        }
        assert_eq!(
            copy_exact(&mut Cursor::new(b"a"), &mut Fail, b"", 1)
                .unwrap_err()
                .status,
            503
        );
        let mut bridge = bridge(fake_native());
        assert_eq!(
            bridge.file_request(&mut Fail, bulk(&[], b"", 3), 3).status,
            408
        );
        assert_eq!(
            bridge
                .file_request(&mut Cursor::new(b"x"), bulk(&[], b"", 3), 3)
                .status,
            400
        );
        assert!(bridge.lanes.compute.try_lock().is_ok());
        let unsealable = File::from(memfd_create("test-unsealable", MemfdFlags::CLOEXEC).unwrap());
        assert_eq!(seal_input(&unsealable).unwrap_err().status, 503);
    }

    #[test]
    fn native_error_map_never_reports_success_or_leaks_messages() {
        for (error, status, code) in [
            (pb_cli::CliError::OutcomeUnknown, 504, "OUTCOME_UNKNOWN"),
            (pb_cli::CliError::ImportTimedOut, 504, "IMPORT_TIMEOUT"),
            (
                pb_cli::CliError::Remote {
                    code: "secret".into(),
                    message_safe: "/secret/path".into(),
                },
                503,
                "NATIVE_REQUEST_FAILED",
            ),
        ] {
            let response = native_error(error);
            assert_eq!(response.status, status);
            assert_eq!(
                serde_json::from_slice::<Value>(&response.body).unwrap(),
                json!({"error":code})
            );
        }
    }

    #[test]
    fn deadline_is_absolute_even_after_progress_and_applies_to_writes() {
        let listener = bind_loopback().unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        client.write_all(b"x").unwrap();
        let mut reader = DeadlineIo::new(&mut server, Duration::from_millis(100));
        reader.read_exact(&mut [0]).unwrap();
        // Deterministically expire, without sleeping or changing production bounds.
        reader.deadline = Instant::now();
        assert_eq!(
            reader.read(&mut [0]).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            reader.write(b"x").unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }

    struct FileNative {
        calls: Arc<AtomicUsize>,
        source: &'static str,
        bytes: u64,
    }
    impl NativeBackend for FileNative {
        fn compute_file(&mut self, source: BorrowedFd<'_>) -> Result<FileResult, pb_cli::CliError> {
            assert!(
                fcntl_get_seals(source)
                    .unwrap()
                    .contains(SealFlags::WRITE | SealFlags::SEAL)
            );
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(FileResult {
                input_bytes: self.bytes,
                compute: NativeCompute {
                    digest_blake3_hex: "a".repeat(64),
                    execution_source: self.source.into(),
                    auto_use_reason: if self.source == "REMOTE_SUCCESS" {
                        "READY"
                    } else {
                        "TRANSPORT_LOST"
                    }
                    .into(),
                },
            })
        }
        fn compute_blake3(&mut self) -> Result<NativeCompute, NativeUnavailable> {
            Err(NativeUnavailable)
        }
        fn status(&mut self) -> Result<NativeStatus, NativeUnavailable> {
            Err(NativeUnavailable)
        }
        fn auto_use(
            &mut self,
            enabled: Option<bool>,
        ) -> Result<NativePreference, pb_cli::AutoUseError> {
            Ok(NativePreference {
                enabled: enabled.unwrap_or(true),
                persistence: "SAVED".into(),
            })
        }
    }
    fn file_bridge(source: &'static str, bytes: u64) -> Bridge<FileNative> {
        let base = bridge(fake_native());
        Bridge {
            native: FileNative {
                calls: Arc::new(AtomicUsize::new(0)),
                source,
                bytes,
            },
            expected_host: base.expected_host,
            token: base.token,
            frontend_root: base.frontend_root,
            last_execution: base.last_execution,
            lanes: base.lanes,
        }
    }

    #[test]
    fn typed_terminal_preserves_sources_and_rejects_mismatched_length() {
        for source in [
            "REMOTE_SUCCESS",
            "LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE",
            "LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE",
        ] {
            let mut b = file_bridge(source, 3);
            let response = b.file_request(&mut Cursor::new(b"bc"), bulk(&[], b"a", 3), 3);
            assert_eq!(response.status, 200);
            let value: Value = serde_json::from_slice(&response.body).unwrap();
            assert_eq!(value.as_object().unwrap().len(), 7);
            assert_eq!(value["execution_source"], source);
            assert_eq!(value["input_bytes"], 3);
            assert_eq!(value["input_kind"], "PRIVATE_IMPORT_V1");
            assert_eq!(b.native.calls.load(Ordering::SeqCst), 1);
            assert!(b.last_execution.lock().unwrap().is_none());
        }
        let mut b = file_bridge("REMOTE_SUCCESS", 4);
        assert_eq!(
            b.file_request(&mut Cursor::new(b"abc"), bulk(&[], b"", 3), 3)
                .status,
            504
        );
        assert_eq!(b.native.calls.load(Ordering::SeqCst), 1);
        assert!(b.lanes.compute.try_lock().is_ok());
        let mut b = file_bridge("REMOTE_SUCCESS", 3);
        assert_eq!(
            b.file_request(&mut Cursor::new(b"abcEXTRA"), bulk(&[], b"", 3), 3)
                .status,
            400
        );
        assert_eq!(b.native.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn off_completes_during_an_admitted_blocked_upload() {
        use std::sync::mpsc;
        struct Blocked {
            started: mpsc::Sender<()>,
            release: mpsc::Receiver<()>,
        }
        impl Read for Blocked {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                self.started.send(()).unwrap();
                self.release.recv_timeout(Duration::from_secs(3)).unwrap();
                buffer[..3].copy_from_slice(b"abc");
                Ok(3)
            }
        }
        impl UploadRead for Blocked {
            fn finish(&mut self) -> Result<(), Response> {
                Ok(())
            }
        }
        let mut upload = file_bridge("LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE", 3);
        let mut control = bridge(fake_native());
        control.lanes = Arc::clone(&upload.lanes);
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            upload.file_request(
                &mut Blocked {
                    started: started_tx,
                    release: release_rx,
                },
                bulk(&[], b"", 3),
                3,
            )
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(
            control
                .handle(request(
                    "POST",
                    "/bridge/v1/auto-use",
                    &[("content-type", "application/json")],
                    br#"{"enabled":false}"#
                ))
                .status,
            200
        );
        assert_eq!(
            control
                .file_request(&mut NoRead, bulk(&[], b"", 3), 3)
                .status,
            503
        );
        release_tx.send(()).unwrap();
        assert_eq!(thread.join().unwrap().status, 200);
        assert!(control.lanes.compute.try_lock().is_ok());
    }

    #[test]
    fn socket_finish_rejects_disconnect_and_buffered_excess() {
        use std::net::Shutdown;
        for trailing in [false, true] {
            let listener = bind_loopback().unwrap();
            let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            if trailing {
                client.write_all(b"x").unwrap();
            }
            client.shutdown(Shutdown::Write).unwrap();
            // Wait for readability (data or EOF) without consuming input.
            server
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            server.peek(&mut [0]).unwrap();
            assert_eq!(
                DeadlineIo::new(&mut server, UPLOAD_TIMEOUT)
                    .finish()
                    .unwrap_err()
                    .status,
                400
            );
        }
    }
}
