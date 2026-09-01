use std::fmt;
use std::os::fd::{BorrowedFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rustix::net::sockopt::socket_peercred;
use rustix::net::{SocketFlags, accept_with};
use rustix::process::getuid;

mod framing;
pub(crate) mod validation;

pub use framing::{FramedLocalClient, LocalFrameOutcome, LocalFramingError, LocalFramingErrorKind};
pub use validation::{
    LocalMethod, LocalMethodClass, LocalValidationCause, LocalValidationError,
    LocalValidationErrorKind, LocalValidationScope, ValidatedLocalRequest,
};

/// Canonical local-client policy carried by every authenticated connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalClientLimits {
    max_clients: usize,
    max_line_bytes: usize,
    max_outstanding: usize,
    idle_timeout: Duration,
}

impl LocalClientLimits {
    pub const CANONICAL: Self = Self {
        max_clients: 8,
        max_line_bytes: 65_536,
        max_outstanding: 16,
        idle_timeout: Duration::from_secs(60),
    };

    pub const fn max_clients(self) -> usize {
        self.max_clients
    }

    pub const fn max_line_bytes(self) -> usize {
        self.max_line_bytes
    }

    pub const fn max_outstanding(self) -> usize {
        self.max_outstanding
    }

    pub const fn idle_timeout(self) -> Duration {
        self.idle_timeout
    }
}

/// Canonical local-connection states reached through LC-007.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalConnectionState {
    AuthenticatedLocal,
    Active,
    Closed,
}

/// Scope of all PASS 3A admission failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalAdmissionScope {
    Connection,
}

/// Typed admission failure categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalAdmissionErrorKind {
    AcceptFailed,
    LocalAuthFailed,
    LocalBusy,
}

/// The only canonical event introduced by this admission path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalAdmissionEventKind {
    LocalAuthFailed,
}

impl LocalAdmissionEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalAuthFailed => "LOCAL_AUTH_FAILED",
        }
    }
}

/// Redacted event with no peer UID, PID, payload, or socket dump.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalAdmissionEvent {
    kind: LocalAdmissionEventKind,
}

impl LocalAdmissionEvent {
    pub const fn kind(self) -> LocalAdmissionEventKind {
        self.kind
    }
}

/// A typed, connection-scoped admission failure.
#[derive(Debug)]
pub struct LocalAdmissionError {
    kind: LocalAdmissionErrorKind,
    event: Option<LocalAdmissionEvent>,
}

impl LocalAdmissionError {
    pub const fn kind(&self) -> LocalAdmissionErrorKind {
        self.kind
    }

    pub const fn scope(&self) -> LocalAdmissionScope {
        LocalAdmissionScope::Connection
    }

    pub const fn state_changed(&self) -> bool {
        false
    }

    pub const fn event(&self) -> Option<LocalAdmissionEvent> {
        self.event
    }
}

impl fmt::Display for LocalAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            LocalAdmissionErrorKind::AcceptFailed => formatter.write_str("local accept failed"),
            LocalAdmissionErrorKind::LocalAuthFailed => formatter.write_str("LOCAL_AUTH_FAILED"),
            LocalAdmissionErrorKind::LocalBusy => formatter.write_str("LOCAL_BUSY"),
        }
    }
}

impl std::error::Error for LocalAdmissionError {}

/// A connection after kernel peer credentials and the client cap are accepted.
///
/// The descriptor is deliberately private and this type implements neither
/// `Read`, `BufRead`, nor `AsFd`. It must be consumed into the bounded framing
/// type before any application byte can be read.
pub struct AuthenticatedLocalClient {
    _connection: OwnedFd,
    peer_uid: u32,
    limits: LocalClientLimits,
    _slot: ClientSlotPermit,
}

impl fmt::Debug for AuthenticatedLocalClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthenticatedLocalClient")
            .field("state", &LocalConnectionState::AuthenticatedLocal)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl AuthenticatedLocalClient {
    pub const fn peer_uid(&self) -> u32 {
        self.peer_uid
    }

    pub const fn limits(&self) -> LocalClientLimits {
        self.limits
    }

    pub const fn state(&self) -> LocalConnectionState {
        LocalConnectionState::AuthenticatedLocal
    }

    /// Consume the authenticated authority and enable only bounded byte framing.
    pub fn into_framed(self) -> Result<FramedLocalClient, LocalFramingError> {
        FramedLocalClient::from_authenticated(self)
    }
}

/// A raw accepted FD. It has no public surface and no byte-reading method.
struct AcceptedRawClient {
    connection: OwnedFd,
}

impl AcceptedRawClient {
    fn authenticate(
        self,
        gate: &LocalAdmissionGate,
        #[cfg(test)] hooks: &mut TestAdmissionHooks,
    ) -> Result<AuthenticatedLocalClient, LocalAdmissionError> {
        #[cfg(test)]
        hooks.trace.push(AdmissionTrace::PeercredStarted);

        #[cfg(test)]
        if hooks.force_peercred_failure {
            return Err(local_auth_failed());
        }

        let kernel_peer = socket_peercred(&self.connection).map_err(|_| local_auth_failed())?;

        #[cfg(test)]
        hooks.trace.push(AdmissionTrace::PeercredSucceeded);

        let observed_uid = kernel_peer.uid.as_raw();
        #[cfg(test)]
        let observed_uid = hooks.override_uid.unwrap_or(observed_uid);

        if observed_uid != getuid().as_raw() {
            return Err(local_auth_failed());
        }

        #[cfg(test)]
        hooks.trace.push(AdmissionTrace::SlotAttempted);

        let slot = gate.try_acquire().ok_or_else(local_busy)?;

        #[cfg(test)]
        hooks.trace.push(AdmissionTrace::Authenticated);

        Ok(AuthenticatedLocalClient {
            _connection: self.connection,
            peer_uid: observed_uid,
            limits: LocalClientLimits::CANONICAL,
            _slot: slot,
        })
    }
}

#[derive(Debug)]
pub(crate) struct LocalAdmissionGate {
    active: Arc<AtomicUsize>,
}

impl LocalAdmissionGate {
    pub(crate) fn new() -> Self {
        Self {
            active: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn try_acquire(&self) -> Option<ClientSlotPermit> {
        let mut active = self.active.load(Ordering::Acquire);
        loop {
            if active >= LocalClientLimits::CANONICAL.max_clients() {
                return None;
            }
            match self.active.compare_exchange_weak(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(ClientSlotPermit {
                        active: Arc::clone(&self.active),
                    });
                }
                Err(observed) => active = observed,
            }
        }
    }

    pub(crate) fn active_clients(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }
}

struct ClientSlotPermit {
    active: Arc<AtomicUsize>,
}

impl ClientSlotPermit {
    fn active_clients(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }
}

impl Drop for ClientSlotPermit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) fn admit_local_client(
    listener: BorrowedFd<'_>,
    gate: &LocalAdmissionGate,
) -> Result<AuthenticatedLocalClient, LocalAdmissionError> {
    #[cfg(test)]
    {
        let mut hooks = TestAdmissionHooks::default();
        admit_local_client_with_hooks(listener, gate, &mut hooks)
    }

    #[cfg(not(test))]
    {
        accept_raw_client(listener)?.authenticate(gate)
    }
}

#[cfg(test)]
pub(crate) fn authenticated_test_client(connection: OwnedFd) -> AuthenticatedLocalClient {
    let active = Arc::new(AtomicUsize::new(1));
    AuthenticatedLocalClient {
        _connection: connection,
        peer_uid: getuid().as_raw(),
        limits: LocalClientLimits::CANONICAL,
        _slot: ClientSlotPermit { active },
    }
}

fn accept_raw_client(listener: BorrowedFd<'_>) -> Result<AcceptedRawClient, LocalAdmissionError> {
    let connection = accept_with(listener, SocketFlags::CLOEXEC).map_err(|_| accept_failed())?;
    Ok(AcceptedRawClient { connection })
}

#[cfg(test)]
fn admit_local_client_with_hooks(
    listener: BorrowedFd<'_>,
    gate: &LocalAdmissionGate,
    hooks: &mut TestAdmissionHooks,
) -> Result<AuthenticatedLocalClient, LocalAdmissionError> {
    let raw = accept_raw_client(listener)?;
    hooks.trace.push(AdmissionTrace::Accepted);
    raw.authenticate(gate, hooks)
}

fn accept_failed() -> LocalAdmissionError {
    LocalAdmissionError {
        kind: LocalAdmissionErrorKind::AcceptFailed,
        event: None,
    }
}

fn local_auth_failed() -> LocalAdmissionError {
    LocalAdmissionError {
        kind: LocalAdmissionErrorKind::LocalAuthFailed,
        event: Some(LocalAdmissionEvent {
            kind: LocalAdmissionEventKind::LocalAuthFailed,
        }),
    }
}

fn local_busy() -> LocalAdmissionError {
    LocalAdmissionError {
        kind: LocalAdmissionErrorKind::LocalBusy,
        event: None,
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AdmissionTrace {
    Accepted,
    PeercredStarted,
    PeercredSucceeded,
    SlotAttempted,
    Authenticated,
}

#[cfg(test)]
#[derive(Default)]
struct TestAdmissionHooks {
    force_peercred_failure: bool,
    override_uid: Option<u32>,
    trace: Vec<AdmissionTrace>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{Read, Write};
    use std::os::fd::AsFd;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

    use rustix::net::{
        AddressFamily, RecvFlags, SocketAddrUnix, SocketType, bind, listen, recv, socket_with,
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct AdmissionHarness {
        listener: OwnedFd,
        gate: LocalAdmissionGate,
        root: PathBuf,
        socket_path: PathBuf,
    }

    impl AdmissionHarness {
        fn new(label: &str) -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "phoneboost-local-admission-{}-{label}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("create admission test directory");
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .expect("secure admission test directory");
            let socket_path = root.join("control.sock");
            let listener = socket_with(
                AddressFamily::UNIX,
                SocketType::STREAM,
                SocketFlags::CLOEXEC,
                None,
            )
            .expect("create admission test listener");
            let address = SocketAddrUnix::new(&socket_path).expect("create pathname address");
            bind(&listener, &address).expect("bind admission test listener");
            listen(&listener, 32).expect("listen for admission tests");
            Self {
                listener,
                gate: LocalAdmissionGate::new(),
                root,
                socket_path,
            }
        }

        fn connect(&self) -> UnixStream {
            UnixStream::connect(&self.socket_path).expect("connect admission test client")
        }

        fn admit(
            &self,
            hooks: &mut TestAdmissionHooks,
        ) -> Result<AuthenticatedLocalClient, LocalAdmissionError> {
            admit_local_client_with_hooks(self.listener.as_fd(), &self.gate, hooks)
        }

        fn active(&self) -> usize {
            self.gate.active_clients()
        }
    }

    impl Drop for AdmissionHarness {
        fn drop(&mut self) {
            let _socket_cleanup = fs::remove_file(&self.socket_path);
            let _directory_cleanup = fs::remove_dir(&self.root);
        }
    }

    fn assert_auth_failed(error: LocalAdmissionError) {
        assert_eq!(error.kind(), LocalAdmissionErrorKind::LocalAuthFailed);
        assert_eq!(error.scope(), LocalAdmissionScope::Connection);
        assert!(!error.state_changed());
        let event = error.event().expect("auth failure has redacted event");
        assert_eq!(event.kind().as_str(), "LOCAL_AUTH_FAILED");
    }

    fn preload(client: &mut UnixStream, bytes: &[u8]) {
        client
            .write_all(bytes)
            .expect("preload arbitrary client bytes");
    }

    fn assert_peer_closed(mut client: UnixStream) {
        let mut byte = [0_u8; 1];
        match client.read(&mut byte) {
            Ok(0) => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            Ok(count) => panic!("closed server FD unexpectedly returned {count} byte(s)"),
            Err(error) => panic!("unexpected closed-FD observation error: {error}"),
        }
    }

    fn admit_same_uid(
        harness: &AdmissionHarness,
    ) -> (UnixStream, AuthenticatedLocalClient, TestAdmissionHooks) {
        let client = harness.connect();
        let mut hooks = TestAdmissionHooks::default();
        let authenticated = harness
            .admit(&mut hooks)
            .expect("same UID must authenticate");
        (client, authenticated, hooks)
    }

    #[test]
    fn la_t01_real_same_uid_accepts_authenticated_client() {
        let harness = AdmissionHarness::new("same-uid");
        let (_client, authenticated, _) = admit_same_uid(&harness);
        assert_eq!(
            authenticated.state(),
            LocalConnectionState::AuthenticatedLocal
        );
        assert_eq!(harness.active(), 1);
    }

    #[test]
    fn la_t02_real_peercred_uid_equals_getuid() {
        let harness = AdmissionHarness::new("peercred-uid");
        let (_client, authenticated, _) = admit_same_uid(&harness);
        assert_eq!(authenticated.peer_uid(), getuid().as_raw());
    }

    #[test]
    fn la_t03_preloaded_bytes_survive_peercred_gate_unconsumed() {
        let harness = AdmissionHarness::new("bytes-preserved");
        let mut client = harness.connect();
        let payload = b"arbitrary pre-auth bytes\n";
        preload(&mut client, payload);
        let mut hooks = TestAdmissionHooks::default();
        let authenticated = harness.admit(&mut hooks).expect("same UID authenticates");
        assert_eq!(
            hooks.trace,
            vec![
                AdmissionTrace::Accepted,
                AdmissionTrace::PeercredStarted,
                AdmissionTrace::PeercredSucceeded,
                AdmissionTrace::SlotAttempted,
                AdmissionTrace::Authenticated,
            ]
        );
        let mut observed = [0_u8; 25];
        let (peeked, received) = recv(
            &authenticated._connection,
            &mut observed[..],
            RecvFlags::PEEK,
        )
        .expect("peek preserved bytes after authentication");
        assert_eq!(received, payload.len());
        assert_eq!(peeked, payload.len());
        assert_eq!(&observed[..peeked], payload);
    }

    #[test]
    fn la_t04_peercred_failure_closes_fd_without_slot() {
        let harness = AdmissionHarness::new("peercred-failure");
        let client = harness.connect();
        let mut hooks = TestAdmissionHooks {
            force_peercred_failure: true,
            ..TestAdmissionHooks::default()
        };
        let error = harness
            .admit(&mut hooks)
            .expect_err("peercred failure must close");
        assert_auth_failed(error);
        assert_eq!(
            hooks.trace,
            vec![AdmissionTrace::Accepted, AdmissionTrace::PeercredStarted]
        );
        assert_eq!(harness.active(), 0);
        assert_peer_closed(client);
    }

    #[test]
    fn la_t05_wrong_uid_logic_is_local_auth_failed() {
        let harness = AdmissionHarness::new("wrong-uid-logic");
        let mut client = harness.connect();
        preload(&mut client, b"must-not-be-interpreted");
        let mut hooks = TestAdmissionHooks {
            override_uid: Some(getuid().as_raw().wrapping_add(1)),
            ..TestAdmissionHooks::default()
        };
        let error = harness.admit(&mut hooks).expect_err("wrong UID must fail");
        assert_auth_failed(error);
        assert_eq!(
            hooks.trace,
            vec![
                AdmissionTrace::Accepted,
                AdmissionTrace::PeercredStarted,
                AdmissionTrace::PeercredSucceeded,
            ]
        );
        assert_eq!(harness.active(), 0);
        assert_peer_closed(client);
    }

    #[test]
    fn la_t07_eight_clients_are_admitted_simultaneously() {
        let harness = AdmissionHarness::new("eight-clients");
        let mut clients = Vec::new();
        let mut authenticated = Vec::new();
        for _ in 0..LocalClientLimits::CANONICAL.max_clients() {
            let (client, admitted, _) = admit_same_uid(&harness);
            clients.push(client);
            authenticated.push(admitted);
        }
        assert_eq!(authenticated.len(), 8);
        assert_eq!(harness.active(), 8);
    }

    #[test]
    fn la_t08_ninth_client_is_local_busy_and_count_stays_eight() {
        let harness = AdmissionHarness::new("ninth-busy");
        let mut held = Vec::new();
        let mut peers = Vec::new();
        for _ in 0..8 {
            let (peer, admitted, _) = admit_same_uid(&harness);
            peers.push(peer);
            held.push(admitted);
        }
        let ninth = harness.connect();
        let mut hooks = TestAdmissionHooks::default();
        let error = harness
            .admit(&mut hooks)
            .expect_err("ninth client must be busy");
        assert_eq!(error.kind(), LocalAdmissionErrorKind::LocalBusy);
        assert_eq!(hooks.trace.last(), Some(&AdmissionTrace::SlotAttempted));
        assert_eq!(harness.active(), 8);
        assert_peer_closed(ninth);
    }

    #[test]
    fn la_t09_drop_releases_slot() {
        let harness = AdmissionHarness::new("drop-slot");
        let (_peer, admitted, _) = admit_same_uid(&harness);
        assert_eq!(harness.active(), 1);
        drop(admitted);
        assert_eq!(harness.active(), 0);
    }

    #[test]
    fn la_t10_new_client_is_admitted_after_drop() {
        let harness = AdmissionHarness::new("admit-after-drop");
        let (_first_peer, first, _) = admit_same_uid(&harness);
        drop(first);
        let (_second_peer, second, _) = admit_same_uid(&harness);
        assert_eq!(second.state(), LocalConnectionState::AuthenticatedLocal);
        assert_eq!(harness.active(), 1);
    }

    #[test]
    fn la_t11_wrong_uid_rejection_never_consumes_slot() {
        let harness = AdmissionHarness::new("wrong-uid-slot");
        let peer = harness.connect();
        let mut hooks = TestAdmissionHooks {
            override_uid: Some(getuid().as_raw().wrapping_add(1)),
            ..TestAdmissionHooks::default()
        };
        let error = harness.admit(&mut hooks).expect_err("wrong UID rejected");
        assert_auth_failed(error);
        assert_eq!(harness.active(), 0);
        assert_peer_closed(peer);
    }

    #[test]
    fn la_t12_local_busy_never_consumes_preloaded_bytes() {
        let harness = AdmissionHarness::new("busy-no-parse");
        let mut held = Vec::new();
        let mut peers = Vec::new();
        for _ in 0..8 {
            let (peer, admitted, _) = admit_same_uid(&harness);
            peers.push(peer);
            held.push(admitted);
        }
        let mut ninth = harness.connect();
        preload(&mut ninth, b"not-json-and-never-read");
        let mut hooks = TestAdmissionHooks::default();
        let error = harness
            .admit(&mut hooks)
            .expect_err("client cap rejects ninth");
        assert_eq!(error.kind(), LocalAdmissionErrorKind::LocalBusy);
        assert_eq!(
            hooks.trace,
            vec![
                AdmissionTrace::Accepted,
                AdmissionTrace::PeercredStarted,
                AdmissionTrace::PeercredSucceeded,
                AdmissionTrace::SlotAttempted,
            ]
        );
        assert_peer_closed(ninth);
    }

    #[test]
    fn la_t13_listener_remains_ready_after_auth_failure() {
        let harness = AdmissionHarness::new("ready-after-auth-fail");
        let rejected_peer = harness.connect();
        let mut rejected_hooks = TestAdmissionHooks {
            override_uid: Some(getuid().as_raw().wrapping_add(1)),
            ..TestAdmissionHooks::default()
        };
        let error = harness
            .admit(&mut rejected_hooks)
            .expect_err("wrong UID rejected");
        assert_auth_failed(error);
        assert_peer_closed(rejected_peer);

        let (_valid_peer, valid, _) = admit_same_uid(&harness);
        assert_eq!(valid.state(), LocalConnectionState::AuthenticatedLocal);
    }

    #[test]
    fn la_t14_listener_remains_ready_after_local_busy() {
        let harness = AdmissionHarness::new("ready-after-busy");
        let mut held = Vec::new();
        let mut peers = Vec::new();
        for _ in 0..8 {
            let (peer, admitted, _) = admit_same_uid(&harness);
            peers.push(peer);
            held.push(admitted);
        }
        let busy_peer = harness.connect();
        let mut hooks = TestAdmissionHooks::default();
        let error = harness.admit(&mut hooks).expect_err("ninth is busy");
        assert_eq!(error.kind(), LocalAdmissionErrorKind::LocalBusy);
        assert_peer_closed(busy_peer);

        drop(held.pop());
        let (_replacement_peer, replacement, _) = admit_same_uid(&harness);
        assert_eq!(
            replacement.state(),
            LocalConnectionState::AuthenticatedLocal
        );
        assert_eq!(harness.active(), 8);
    }

    #[test]
    fn la_t15_canonical_limits_are_exact() {
        let limits = LocalClientLimits::CANONICAL;
        assert_eq!(limits.max_clients(), 8);
        assert_eq!(limits.max_line_bytes(), 65_536);
        assert_eq!(limits.max_outstanding(), 16);
        assert_eq!(limits.idle_timeout(), Duration::from_secs(60));
    }

    #[test]
    fn la_t16_authenticated_type_requires_valid_peercred_and_slot() {
        let harness = AdmissionHarness::new("type-gate");
        let failed_peer = harness.connect();
        let mut failed_hooks = TestAdmissionHooks {
            force_peercred_failure: true,
            ..TestAdmissionHooks::default()
        };
        assert!(harness.admit(&mut failed_hooks).is_err());
        assert_peer_closed(failed_peer);
        assert_eq!(harness.active(), 0);

        let (_valid_peer, authenticated, trace) = admit_same_uid(&harness);
        assert_eq!(
            authenticated.state(),
            LocalConnectionState::AuthenticatedLocal
        );
        assert_eq!(trace.trace.last(), Some(&AdmissionTrace::Authenticated));
        assert_eq!(harness.active(), 1);
    }

    #[test]
    fn oracle_rejects_false_kernel_wrong_uid_claim() {
        let harness = AdmissionHarness::new("oracle-uid-claim");
        let (_peer, authenticated, _) = admit_same_uid(&harness);
        assert_eq!(authenticated.peer_uid(), getuid().as_raw());
        drop(authenticated);

        let simulated_peer = harness.connect();
        let mut simulated = TestAdmissionHooks {
            override_uid: Some(getuid().as_raw().wrapping_add(1)),
            ..TestAdmissionHooks::default()
        };
        let error = harness
            .admit(&mut simulated)
            .expect_err("simulated branch rejects");
        assert_auth_failed(error);
        assert_peer_closed(simulated_peer);
        assert!(simulated.override_uid.is_some());
    }

    #[test]
    fn oracle_client_cap_never_exceeds_eight_under_repeated_attempts() {
        let harness = AdmissionHarness::new("oracle-cap");
        let mut held = Vec::new();
        let mut peers = Vec::new();
        for _ in 0..8 {
            let (peer, admitted, _) = admit_same_uid(&harness);
            peers.push(peer);
            held.push(admitted);
        }
        for _ in 0..4 {
            let peer = harness.connect();
            let mut hooks = TestAdmissionHooks::default();
            let error = harness.admit(&mut hooks).expect_err("cap remains closed");
            assert_eq!(error.kind(), LocalAdmissionErrorKind::LocalBusy);
            assert_eq!(harness.active(), 8);
            assert_peer_closed(peer);
        }
    }

    #[test]
    fn admitted_client_carries_policy_without_active_state() {
        let harness = AdmissionHarness::new("policy-state");
        let (_peer, authenticated, _) = admit_same_uid(&harness);
        assert_eq!(authenticated.limits(), LocalClientLimits::CANONICAL);
        assert_eq!(
            authenticated.state(),
            LocalConnectionState::AuthenticatedLocal
        );
    }

    fn _assert_path_is_not_used_as_identity(_path: &Path) {}
}
