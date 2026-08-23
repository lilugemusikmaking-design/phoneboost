use std::fmt;
use std::os::fd::OwnedFd;
use std::time::Duration;

use rustix::io::Errno;
use rustix::net::sockopt::{Timeout, set_socket_timeout};
use rustix::net::{RecvFlags, SendFlags, recv, send};

use super::validation::{LocalValidationError, ValidatedLocalRequest, validate_local_request};
use super::{
    AuthenticatedLocalClient, ClientSlotPermit, LocalAdmissionScope, LocalClientLimits,
    LocalConnectionState,
};

const READ_CHUNK_BYTES: usize = 4_096;
const FRAMING_MEMORY_BOUND_BYTES: usize =
    LocalClientLimits::CANONICAL.max_line_bytes() + READ_CHUNK_BYTES;

/// A bounded byte-framing result. No UTF-8 or JSON interpretation occurs.
#[derive(Debug, Eq, PartialEq)]
pub enum LocalFrameOutcome {
    Line(Vec<u8>),
    CleanEof,
    IdleTimeout,
}

/// Framing failures. `LocalBadRequest` is the canonical framing rejection;
/// transport failure remains an internal lifecycle category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalFramingErrorKind {
    LocalBadRequest,
    TransportFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalResponseWriteError {
    InvalidResponseLine,
    TransportFailure,
}

/// Connection-scoped framing failure; no C12 response envelope exists yet.
#[derive(Debug)]
pub struct LocalFramingError {
    kind: LocalFramingErrorKind,
}

impl LocalFramingError {
    pub const fn kind(&self) -> LocalFramingErrorKind {
        self.kind
    }

    pub const fn scope(&self) -> LocalAdmissionScope {
        LocalAdmissionScope::Connection
    }

    pub const fn state_changed(&self) -> bool {
        false
    }
}

impl fmt::Display for LocalFramingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            LocalFramingErrorKind::LocalBadRequest => formatter.write_str("LOCAL_BAD_REQUEST"),
            LocalFramingErrorKind::TransportFailure => {
                formatter.write_str("local framing transport failure")
            }
        }
    }
}

impl std::error::Error for LocalFramingError {}

/// Bounded NDJSON byte framing after local authentication.
///
/// The stream and slot permit remain private. The only application-facing
/// primitive is `next_line_bytes`, which enforces the canonical byte limit.
pub struct FramedLocalClient {
    connection: Option<OwnedFd>,
    peer_uid: u32,
    limits: LocalClientLimits,
    slot: Option<ClientSlotPermit>,
    line: Vec<u8>,
    carry: [u8; READ_CHUNK_BYTES],
    carry_start: usize,
    carry_end: usize,
    read_chunk_limit: usize,
    active: bool,
    #[cfg(test)]
    max_buffer_observed: usize,
    #[cfg(test)]
    bytes_received: usize,
}

impl fmt::Debug for FramedLocalClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FramedLocalClient")
            .field("state", &self.state())
            .field("limits", &self.limits)
            .field("buffered_line_bytes", &self.line.len())
            .field("pending_carry_bytes", &self.pending_carry_bytes())
            .finish_non_exhaustive()
    }
}

impl FramedLocalClient {
    pub(super) fn from_authenticated(
        client: AuthenticatedLocalClient,
    ) -> Result<Self, LocalFramingError> {
        Self::build(
            client,
            LocalClientLimits::CANONICAL.idle_timeout(),
            READ_CHUNK_BYTES,
        )
    }

    #[cfg(test)]
    fn from_authenticated_with_config(
        client: AuthenticatedLocalClient,
        idle_timeout: Duration,
        read_chunk_limit: usize,
    ) -> Result<Self, LocalFramingError> {
        Self::build(client, idle_timeout, read_chunk_limit)
    }

    fn build(
        client: AuthenticatedLocalClient,
        idle_timeout: Duration,
        read_chunk_limit: usize,
    ) -> Result<Self, LocalFramingError> {
        let AuthenticatedLocalClient {
            _connection: connection,
            peer_uid,
            limits,
            _slot: slot,
        } = client;
        set_socket_timeout(&connection, Timeout::Recv, Some(idle_timeout))
            .map_err(|_| transport_failure())?;
        Ok(Self {
            connection: Some(connection),
            peer_uid,
            limits,
            slot: Some(slot),
            line: Vec::with_capacity(limits.max_line_bytes()),
            carry: [0; READ_CHUNK_BYTES],
            carry_start: 0,
            carry_end: 0,
            read_chunk_limit: read_chunk_limit.clamp(1, READ_CHUNK_BYTES),
            active: false,
            #[cfg(test)]
            max_buffer_observed: 0,
            #[cfg(test)]
            bytes_received: 0,
        })
    }

    pub const fn peer_uid(&self) -> u32 {
        self.peer_uid
    }

    pub const fn limits(&self) -> LocalClientLimits {
        self.limits
    }

    pub fn state(&self) -> LocalConnectionState {
        if self.connection.is_none() {
            LocalConnectionState::Closed
        } else if self.active {
            LocalConnectionState::Active
        } else {
            LocalConnectionState::AuthenticatedLocal
        }
    }

    pub const fn framing_memory_bound_bytes() -> usize {
        FRAMING_MEMORY_BOUND_BYTES
    }

    pub fn active_clients(&self) -> usize {
        self.slot
            .as_ref()
            .map_or(0, ClientSlotPermit::active_clients)
    }

    /// Validate one complete framed line and enter `ACTIVE` only on success.
    pub fn validate_request_line(
        &mut self,
        line: Vec<u8>,
    ) -> Result<ValidatedLocalRequest, LocalValidationError> {
        let request = validate_local_request(line)?;
        self.active = true;
        Ok(request)
    }

    pub(crate) fn write_response_ndjson(
        &mut self,
        response: &[u8],
    ) -> Result<(), LocalResponseWriteError> {
        if response.is_empty()
            || !response.ends_with(b"\n")
            || response.len() > self.limits.max_line_bytes()
        {
            self.close();
            return Err(LocalResponseWriteError::InvalidResponseLine);
        }

        let mut written = 0;
        while written < response.len() {
            let Some(connection) = self.connection.as_ref() else {
                return Err(LocalResponseWriteError::TransportFailure);
            };
            match send(connection, &response[written..], SendFlags::NOSIGNAL) {
                Ok(0) => {
                    self.close();
                    return Err(LocalResponseWriteError::TransportFailure);
                }
                Ok(count) => written += count,
                Err(Errno::INTR) => {}
                Err(_) => {
                    self.close();
                    return Err(LocalResponseWriteError::TransportFailure);
                }
            }
        }
        Ok(())
    }

    /// Read the next LF-delimited byte frame without UTF-8 or JSON parsing.
    pub fn next_line_bytes(&mut self) -> Result<LocalFrameOutcome, LocalFramingError> {
        if self.connection.is_none() {
            return Ok(LocalFrameOutcome::CleanEof);
        }

        loop {
            if self.carry_start < self.carry_end
                && let Some(outcome) = self.consume_carry()?
            {
                return Ok(outcome);
            }

            let received = {
                let Some(connection) = self.connection.as_ref() else {
                    return Ok(LocalFrameOutcome::CleanEof);
                };
                match recv(
                    connection,
                    &mut self.carry[..self.read_chunk_limit],
                    RecvFlags::empty(),
                ) {
                    Ok((initialized, _reported)) => initialized,
                    Err(Errno::INTR) => continue,
                    Err(Errno::AGAIN) => {
                        self.close();
                        return Ok(LocalFrameOutcome::IdleTimeout);
                    }
                    Err(_) => {
                        self.close();
                        return Err(transport_failure());
                    }
                }
            };

            if received == 0 {
                let had_partial = !self.line.is_empty();
                self.close();
                return if had_partial {
                    Err(local_bad_request())
                } else {
                    Ok(LocalFrameOutcome::CleanEof)
                };
            }

            #[cfg(test)]
            {
                self.bytes_received += received;
            }
            self.carry_start = 0;
            self.carry_end = received;
            self.observe_buffer_bound();
        }
    }

    fn consume_carry(&mut self) -> Result<Option<LocalFrameOutcome>, LocalFramingError> {
        let pending = &self.carry[self.carry_start..self.carry_end];
        let newline_offset = pending.iter().position(|byte| *byte == b'\n');
        let bytes_before_newline = newline_offset.unwrap_or(pending.len());
        let remaining_capacity = self.limits.max_line_bytes() - self.line.len();

        if bytes_before_newline > remaining_capacity {
            self.close();
            return Err(local_bad_request());
        }

        self.line
            .extend_from_slice(&pending[..bytes_before_newline]);
        self.carry_start += bytes_before_newline;
        self.observe_buffer_bound();

        if newline_offset.is_some() {
            self.carry_start += 1;
            if self.carry_start == self.carry_end {
                self.carry_start = 0;
                self.carry_end = 0;
            }
            let line = std::mem::take(&mut self.line);
            return Ok(Some(LocalFrameOutcome::Line(line)));
        }

        self.carry_start = 0;
        self.carry_end = 0;
        Ok(None)
    }

    fn pending_carry_bytes(&self) -> usize {
        self.carry_end - self.carry_start
    }

    fn close(&mut self) {
        self.connection.take();
        self.slot.take();
        self.line.clear();
        self.carry_start = 0;
        self.carry_end = 0;
    }

    #[cfg(test)]
    fn observe_buffer_bound(&mut self) {
        let observed = self.line.len() + self.pending_carry_bytes();
        self.max_buffer_observed = self.max_buffer_observed.max(observed);
    }

    #[cfg(not(test))]
    fn observe_buffer_bound(&mut self) {}

    #[cfg(test)]
    fn max_buffer_observed(&self) -> usize {
        self.max_buffer_observed
    }

    #[cfg(test)]
    fn bytes_received(&self) -> usize {
        self.bytes_received
    }
}

fn local_bad_request() -> LocalFramingError {
    LocalFramingError {
        kind: LocalFramingErrorKind::LocalBadRequest,
    }
}

fn transport_failure() -> LocalFramingError {
    LocalFramingError {
        kind: LocalFramingErrorKind::TransportFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::net::Shutdown;
    use std::os::fd::{AsFd, OwnedFd};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
    use std::thread;

    use rustix::net::sockopt::socket_timeout;
    use rustix::net::{
        AddressFamily, SocketAddrUnix, SocketFlags, SocketType, bind, listen, socket_with,
    };

    use crate::admission::{LocalAdmissionGate, admit_local_client};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    const TEST_IDLE_TIMEOUT: Duration = Duration::from_millis(25);

    struct FramingHarness {
        listener: OwnedFd,
        gate: LocalAdmissionGate,
        root: PathBuf,
        socket_path: PathBuf,
    }

    impl FramingHarness {
        fn new(label: &str) -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "phoneboost-framing-{}-{label}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("create framing test directory");
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .expect("secure framing test directory");
            let socket_path = root.join("control.sock");
            let listener = socket_with(
                AddressFamily::UNIX,
                SocketType::STREAM,
                SocketFlags::CLOEXEC,
                None,
            )
            .expect("create framing listener");
            let address = SocketAddrUnix::new(&socket_path).expect("create framing address");
            bind(&listener, &address).expect("bind framing listener");
            listen(&listener, 32).expect("listen for framing tests");
            Self {
                listener,
                gate: LocalAdmissionGate::new(),
                root,
                socket_path,
            }
        }

        fn connect(&self) -> UnixStream {
            UnixStream::connect(&self.socket_path).expect("connect framing peer")
        }

        fn admit(&self, peer: &UnixStream) -> FramedLocalClient {
            let authenticated = admit_local_client(self.listener.as_fd(), &self.gate)
                .expect("admit same-UID framing peer");
            let _peer_must_remain_alive = peer;
            authenticated
                .into_framed()
                .expect("configure canonical framing")
        }

        fn admit_with_config(
            &self,
            peer: &UnixStream,
            timeout: Duration,
            chunk: usize,
        ) -> FramedLocalClient {
            let authenticated = admit_local_client(self.listener.as_fd(), &self.gate)
                .expect("admit configured framing peer");
            let _peer_must_remain_alive = peer;
            FramedLocalClient::from_authenticated_with_config(authenticated, timeout, chunk)
                .expect("configure test framing")
        }

        fn active(&self) -> usize {
            self.gate.active_clients()
        }
    }

    impl Drop for FramingHarness {
        fn drop(&mut self) {
            let _socket_cleanup = fs::remove_file(&self.socket_path);
            let _directory_cleanup = fs::remove_dir(&self.root);
        }
    }

    fn line(outcome: LocalFrameOutcome) -> Vec<u8> {
        match outcome {
            LocalFrameOutcome::Line(bytes) => bytes,
            other => panic!("expected byte line, got {other:?}"),
        }
    }

    fn assert_bad_request(result: Result<LocalFrameOutcome, LocalFramingError>) {
        let error = result.expect_err("framing must reject input");
        assert_eq!(error.kind(), LocalFramingErrorKind::LocalBadRequest);
        assert_eq!(error.scope(), LocalAdmissionScope::Connection);
        assert!(!error.state_changed());
    }

    #[test]
    fn fr_t01_simple_line_returns_bytes_without_lf() {
        let harness = FramingHarness::new("simple");
        let mut peer = harness.connect();
        peer.write_all(b"abc\n").expect("send simple line");
        let mut framed = harness.admit(&peer);
        assert_eq!(
            line(framed.next_line_bytes().expect("read simple line")),
            b"abc"
        );
    }

    #[test]
    fn fr_t02_empty_line_is_valid_byte_frame() {
        let harness = FramingHarness::new("empty");
        let mut peer = harness.connect();
        peer.write_all(b"\n").expect("send empty line");
        let mut framed = harness.admit(&peer);
        assert!(line(framed.next_line_bytes().expect("read empty line")).is_empty());
    }

    #[test]
    fn fr_t03_two_lines_in_one_write_remain_ordered() {
        let harness = FramingHarness::new("two-lines");
        let mut peer = harness.connect();
        peer.write_all(b"first\nsecond\n").expect("send two lines");
        let mut framed = harness.admit(&peer);
        assert_eq!(
            line(framed.next_line_bytes().expect("first line")),
            b"first"
        );
        assert_eq!(
            line(framed.next_line_bytes().expect("second line")),
            b"second"
        );
    }

    #[test]
    fn fr_t04_complete_line_returns_while_partial_next_is_retained() {
        let harness = FramingHarness::new("line-partial");
        let mut peer = harness.connect();
        peer.write_all(b"complete\npartial")
            .expect("send line and partial");
        let mut framed = harness.admit(&peer);
        assert_eq!(
            line(framed.next_line_bytes().expect("complete line")),
            b"complete"
        );
        peer.write_all(b"-done\n").expect("finish partial line");
        assert_eq!(
            line(framed.next_line_bytes().expect("completed retained line")),
            b"partial-done"
        );
    }

    #[test]
    fn fr_t05_one_byte_recv_chunks_reconstruct_exact_line() {
        let harness = FramingHarness::new("one-byte");
        let mut peer = harness.connect();
        peer.write_all(b"one-byte-at-a-time\n")
            .expect("send test line");
        let mut framed = harness.admit_with_config(&peer, Duration::from_secs(1), 1);
        assert_eq!(
            line(
                framed
                    .next_line_bytes()
                    .expect("reconstruct one-byte chunks")
            ),
            b"one-byte-at-a-time"
        );
    }

    #[test]
    fn fr_t06_65535_bytes_plus_lf_is_accepted() {
        let harness = FramingHarness::new("boundary-minus-one");
        let mut peer = harness.connect();
        let mut payload = vec![b'a'; 65_535];
        payload.push(b'\n');
        peer.write_all(&payload).expect("send 65535-byte line");
        let mut framed = harness.admit(&peer);
        assert_eq!(
            line(framed.next_line_bytes().expect("accept 65535 bytes")).len(),
            65_535
        );
    }

    #[test]
    fn fr_t07_65536_bytes_plus_lf_is_accepted() {
        let harness = FramingHarness::new("boundary-exact");
        let mut peer = harness.connect();
        let mut payload = vec![b'b'; 65_536];
        payload.push(b'\n');
        peer.write_all(&payload).expect("send exact maximum line");
        let mut framed = harness.admit(&peer);
        assert_eq!(
            line(framed.next_line_bytes().expect("accept exact maximum")).len(),
            65_536
        );
    }

    #[test]
    fn fr_t08_65537th_non_lf_byte_is_bad_request_and_closes() {
        let harness = FramingHarness::new("boundary-over");
        let mut peer = harness.connect();
        let mut payload = vec![b'c'; 65_537];
        payload.push(b'\n');
        peer.write_all(&payload).expect("send oversized line");
        let mut framed = harness.admit(&peer);
        assert_bad_request(framed.next_line_bytes());
        assert_eq!(framed.state(), LocalConnectionState::Closed);
        assert_eq!(harness.active(), 0);
    }

    #[test]
    fn fr_t09_multimegabyte_no_lf_stays_within_exact_buffer_bound() {
        let harness = FramingHarness::new("hostile-memory");
        let peer = harness.connect();
        let mut framed = harness.admit(&peer);
        let writer = thread::spawn(move || {
            let mut peer = peer;
            let hostile = vec![b'x'; 2 * 1_024 * 1_024];
            let _write_result = peer.write_all(&hostile);
        });
        assert_bad_request(framed.next_line_bytes());
        writer.join().expect("hostile writer joins");
        assert!(framed.max_buffer_observed() <= FramedLocalClient::framing_memory_bound_bytes());
        assert_eq!(FramedLocalClient::framing_memory_bound_bytes(), 69_632);
        assert!(framed.bytes_received() <= FramedLocalClient::framing_memory_bound_bytes());
        assert!(framed.bytes_received() < 2 * 1_024 * 1_024);
        assert_eq!(framed.state(), LocalConnectionState::Closed);
    }

    #[test]
    fn fr_t10_partial_short_line_then_eof_is_bad_request() {
        let harness = FramingHarness::new("partial-eof");
        let mut peer = harness.connect();
        peer.write_all(b"partial").expect("send partial line");
        peer.shutdown(Shutdown::Write)
            .expect("half-close peer write");
        let mut framed = harness.admit(&peer);
        assert_bad_request(framed.next_line_bytes());
        assert_eq!(framed.state(), LocalConnectionState::Closed);
    }

    #[test]
    fn fr_t11_65536_bytes_then_eof_without_lf_is_bad_request() {
        let harness = FramingHarness::new("max-partial-eof");
        let mut peer = harness.connect();
        peer.write_all(&vec![b'm'; 65_536])
            .expect("send max partial line");
        peer.shutdown(Shutdown::Write)
            .expect("half-close max partial");
        let mut framed = harness.admit(&peer);
        assert_bad_request(framed.next_line_bytes());
    }

    #[test]
    fn fr_t12_clean_eof_without_pending_is_clean_close() {
        let harness = FramingHarness::new("clean-eof");
        let peer = harness.connect();
        peer.shutdown(Shutdown::Write)
            .expect("half-close empty peer");
        let mut framed = harness.admit(&peer);
        assert_eq!(
            framed.next_line_bytes().expect("clean EOF"),
            LocalFrameOutcome::CleanEof
        );
        assert_eq!(framed.state(), LocalConnectionState::Closed);
        assert_eq!(harness.active(), 0);
    }

    #[test]
    fn fr_t13_invalid_utf8_is_framing_success() {
        let harness = FramingHarness::new("invalid-utf8");
        let mut peer = harness.connect();
        peer.write_all(&[0xff, 0xfe, 0xfd, b'\n'])
            .expect("send invalid UTF-8 bytes");
        let mut framed = harness.admit(&peer);
        assert_eq!(
            line(framed.next_line_bytes().expect("byte framing succeeds")),
            [0xff, 0xfe, 0xfd]
        );
    }

    #[test]
    fn fr_t14_json_invalid_bytes_are_framing_success() {
        let harness = FramingHarness::new("json-invalid");
        let mut peer = harness.connect();
        peer.write_all(b"not-json\n")
            .expect("send JSON-invalid bytes");
        let mut framed = harness.admit(&peer);
        assert_eq!(
            line(framed.next_line_bytes().expect("no JSON parsing")),
            b"not-json"
        );
    }

    #[test]
    fn fr_t15_crlf_preserves_cr_byte() {
        let harness = FramingHarness::new("crlf");
        let mut peer = harness.connect();
        peer.write_all(b"abc\r\n").expect("send CRLF bytes");
        let mut framed = harness.admit(&peer);
        assert_eq!(
            line(framed.next_line_bytes().expect("read CR-preserving line")),
            b"abc\r"
        );
    }

    #[test]
    fn fr_t16_bytes_after_first_lf_are_never_lost() {
        let harness = FramingHarness::new("no-loss");
        let mut peer = harness.connect();
        peer.write_all(b"a\nbc\ndef\n").expect("send carried lines");
        let mut framed = harness.admit(&peer);
        assert_eq!(line(framed.next_line_bytes().expect("line a")), b"a");
        assert_eq!(line(framed.next_line_bytes().expect("line bc")), b"bc");
        assert_eq!(line(framed.next_line_bytes().expect("line def")), b"def");
    }

    #[test]
    fn fr_t17_successive_calls_never_duplicate_bytes() {
        let harness = FramingHarness::new("no-duplication");
        let mut peer = harness.connect();
        peer.write_all(b"123\n4567\n89\n")
            .expect("send distinct lines");
        let mut framed = harness.admit(&peer);
        let joined = [
            line(framed.next_line_bytes().expect("first distinct line")),
            line(framed.next_line_bytes().expect("second distinct line")),
            line(framed.next_line_bytes().expect("third distinct line")),
        ]
        .concat();
        assert_eq!(joined, b"123456789");
    }

    #[test]
    fn fr_t18_idle_timeout_closes_and_releases_slot_without_bad_request() {
        let harness = FramingHarness::new("idle");
        let mut peer = harness.connect();
        peer.write_all(b"partial").expect("start partial line");
        let mut framed = harness.admit_with_config(&peer, TEST_IDLE_TIMEOUT, READ_CHUNK_BYTES);
        assert_eq!(
            framed.next_line_bytes().expect("idle is lifecycle outcome"),
            LocalFrameOutcome::IdleTimeout
        );
        assert_eq!(framed.state(), LocalConnectionState::Closed);
        assert_eq!(harness.active(), 0);
    }

    #[test]
    fn fr_t19_new_client_acquires_slot_after_timeout() {
        let harness = FramingHarness::new("after-idle");
        let first_peer = harness.connect();
        let mut first = harness.admit_with_config(&first_peer, TEST_IDLE_TIMEOUT, READ_CHUNK_BYTES);
        assert_eq!(
            first.next_line_bytes().expect("first client times out"),
            LocalFrameOutcome::IdleTimeout
        );
        assert_eq!(harness.active(), 0);

        let mut second_peer = harness.connect();
        second_peer
            .write_all(b"next\n")
            .expect("send next client line");
        let mut second = harness.admit(&second_peer);
        assert_eq!(
            line(second.next_line_bytes().expect("new client admitted")),
            b"next"
        );
        assert_eq!(harness.active(), 1);
    }

    #[test]
    fn fr_t20_production_idle_timeout_is_exactly_60_seconds() {
        assert_eq!(
            LocalClientLimits::CANONICAL.idle_timeout(),
            Duration::from_secs(60)
        );
        let harness = FramingHarness::new("production-timeout");
        let peer = harness.connect();
        let framed = harness.admit(&peer);
        let configured = socket_timeout(
            framed
                .connection
                .as_ref()
                .expect("framed connection is open"),
            Timeout::Recv,
        )
        .expect("read configured receive timeout");
        assert_eq!(configured, Some(Duration::from_secs(60)));
    }

    #[test]
    fn fr_t21_max_line_is_exactly_65536() {
        assert_eq!(LocalClientLimits::CANONICAL.max_line_bytes(), 65_536);
    }

    #[test]
    fn fr_t22_max_outstanding_is_carried_but_not_enforced_here() {
        assert_eq!(LocalClientLimits::CANONICAL.max_outstanding(), 16);
    }

    #[test]
    fn fr_t23_client_cap_remains_eight() {
        assert_eq!(LocalClientLimits::CANONICAL.max_clients(), 8);
    }

    #[test]
    fn fr_t24_preloaded_bytes_flow_from_peercred_gate_into_framing() {
        let harness = FramingHarness::new("preloaded-transition");
        let mut peer = harness.connect();
        peer.write_all(b"preloaded\n")
            .expect("preload before admission");
        let mut framed = harness.admit(&peer);
        assert_eq!(framed.peer_uid(), rustix::process::getuid().as_raw());
        assert_eq!(
            line(framed.next_line_bytes().expect("frame preloaded bytes")),
            b"preloaded"
        );
    }

    #[test]
    fn fr_t25_only_authenticated_client_can_construct_framing() {
        let harness = FramingHarness::new("type-flow");
        let peer = harness.connect();
        let authenticated = admit_local_client(harness.listener.as_fd(), &harness.gate)
            .expect("authenticate before framing");
        assert_eq!(
            authenticated.state(),
            LocalConnectionState::AuthenticatedLocal
        );
        let framed = authenticated
            .into_framed()
            .expect("consume authenticated client");
        assert_eq!(framed.state(), LocalConnectionState::AuthenticatedLocal);
        let _peer_stays_connected = peer;
    }

    #[test]
    fn oracle_exact_boundary_accepts_equal_and_rejects_greater() {
        assert_eq!(LocalClientLimits::CANONICAL.max_line_bytes(), 65_536);
        assert_eq!(
            FramedLocalClient::framing_memory_bound_bytes(),
            65_536 + READ_CHUNK_BYTES
        );
    }

    #[test]
    fn val_t29_invalid_utf8_frames_then_validation_rejects_without_activation() {
        let harness = FramingHarness::new("validation-layering");
        let mut peer = harness.connect();
        peer.write_all(&[0xff, 0xfe, b'\n'])
            .expect("send framed invalid UTF-8");
        let mut framed = harness.admit(&peer);
        let bytes = line(
            framed
                .next_line_bytes()
                .expect("framing accepts invalid UTF-8 bytes"),
        );
        assert_eq!(framed.state(), LocalConnectionState::AuthenticatedLocal);
        let error = framed
            .validate_request_line(bytes)
            .expect_err("LC-007 rejects invalid UTF-8");
        assert_eq!(
            error.kind(),
            super::super::LocalValidationErrorKind::LocalBadRequest
        );
        assert_eq!(error.scope(), super::super::LocalValidationScope::Request);
        assert!(!error.state_changed());
        assert_eq!(framed.state(), LocalConnectionState::AuthenticatedLocal);
        assert_eq!(harness.active(), 1);
    }

    #[test]
    fn val_t30_exact_max_valid_json_activates_only_after_complete_validation() {
        let harness = FramingHarness::new("validation-max-line");
        let mut peer = harness.connect();
        let prefix = br#"{"api":1,"id":"max","method":"system.status","params":""#;
        let suffix = br#""}"#;
        let filler_len =
            LocalClientLimits::CANONICAL.max_line_bytes() - prefix.len() - suffix.len();
        let mut payload = Vec::with_capacity(65_537);
        payload.extend_from_slice(prefix);
        payload.extend(std::iter::repeat_n(b'x', filler_len));
        payload.extend_from_slice(suffix);
        assert_eq!(payload.len(), 65_536);
        payload.push(b'\n');
        peer.write_all(&payload)
            .expect("send exact maximum JSON line");

        let mut framed = harness.admit(&peer);
        let bytes = line(
            framed
                .next_line_bytes()
                .expect("framing accepts exact maximum"),
        );
        assert_eq!(bytes.len(), 65_536);
        assert_eq!(framed.state(), LocalConnectionState::AuthenticatedLocal);
        let validated = framed
            .validate_request_line(bytes)
            .expect("exact maximum still passes all LC-007 validators");
        assert_eq!(validated.source_len(), 65_536);
        assert_eq!(validated.method(), super::super::LocalMethod::SystemStatus);
        assert_eq!(framed.state(), LocalConnectionState::Active);
    }
}
