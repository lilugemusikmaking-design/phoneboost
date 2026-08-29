use std::fs::File;
use std::io::Read;
use std::net::{Shutdown, TcpStream};
use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use pb_pbmux::{
    AckPayload, BufferResult, CommandPayload, ComputeRequest, ComputeResponse, DispatchMode, Frame,
    Header, PbmuxErrorKind, Reassembler, RemoteBufferRequest, RemoteBufferResponseKind,
    ResourceRequest, ResourceResponseKind, ResourceResult, SequenceTracker, authorize_dispatch,
    build_command_ack_frame, build_command_frame, build_compute_request_frame,
    build_compute_response_frame, build_remote_buffer_request_frames,
    build_remote_buffer_result_frames, build_resource_request_frame, build_resource_result_frame,
    decode, encode, pair_confirm_frame, parse_command_ack_frame, parse_command_frame,
    parse_compute_request_frame, parse_compute_response_frame, parse_heartbeat_frame,
    parse_remote_buffer_request_payload, parse_remote_buffer_result_payload,
    parse_resource_request_frame, parse_resource_result_frame, validate_pair_confirm,
    validate_remote_buffer_request_fragment, validate_remote_buffer_result_fragment,
};
use pb_secure::{
    NOISE_IK_NAME, PROLOGUE, PairingActor, PersistOutcome, derive_sas, production_xx_initiator,
    production_xx_responder,
};
use pb_types::{
    Channel, ControlType, FLAG_END, FLAG_START, PAIRING_TIMEOUT_MS, PairingState, PeerId,
};
use snow::{Builder, HandshakeState, TransportState, params::NoiseParams};

use crate::storage::{Identity, PeerRecord, StateStore, StorageError, wall_clock_ms};
use crate::wire::{SecureWireError, read_encrypted, read_record, write_encrypted, write_record};
use crate::{
    InitiatorClientError, InitiatorSessionDriver, initiator::InitiatorRequest,
    initiator::InitiatorResponse, initiator::PendingInitiatorRequest,
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const SESSION_POLL: Duration = Duration::from_millis(20);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
const DEVICE_OFFLINE_TIMEOUT: Duration = Duration::from_secs(10);
const XX_FIRST_MESSAGE_BYTES: usize = 32;

struct SessionScope {
    live: AtomicBool,
}

impl SessionScope {
    const fn new() -> Self {
        Self {
            live: AtomicBool::new(true),
        }
    }
}

/// Proof that a peer identity came from the authenticated, committed secure session.
///
/// The proof is scoped to one live secure session and cannot be constructed from a
/// [`PeerId`] by downstream code.
///
/// ```compile_fail
/// use pb_runtime_secure::VerifiedPeerSession;
/// use pb_types::PeerId;
/// use std::marker::PhantomData;
///
/// let peer_id = PeerId::from_sha256_digest([0; 32]);
/// let _proof = VerifiedPeerSession {
///     peer_id,
///     _scope: PhantomData,
/// };
/// ```
///
/// ```compile_fail
/// use pb_runtime_secure::VerifiedPeerSession;
///
/// let _proof: VerifiedPeerSession<'static> = Default::default();
/// ```
///
/// ```compile_fail
/// use pb_runtime_secure::VerifiedPeerSession;
///
/// fn duplicate(proof: VerifiedPeerSession<'_>) {
///     let _copy = proof.clone();
/// }
/// ```
pub struct VerifiedPeerSession<'session> {
    peer_id: PeerId,
    session_id: VerifiedSessionId,
    live: &'session AtomicBool,
    _scope: std::marker::PhantomData<&'session SessionScope>,
}

/// Opaque identity of one authenticated SecureSession lifetime.
///
/// This value is not authentication proof. Production authority APIs continue
/// to require `VerifiedPeerSession`; the identifier exists only to bind
/// volatile provider records to the exact session that created them.
///
/// ```compile_fail
/// use pb_runtime_secure::VerifiedSessionId;
///
/// let _forged = VerifiedSessionId([0; 16]);
/// ```
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct VerifiedSessionId([u8; 16]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticatedCommandHandlerError {
    Unavailable,
    Failed,
}

pub trait AuthenticatedCommandHandler: Send + Sync {
    fn handle_authenticated_command(
        &self,
        verified_session: &VerifiedPeerSession<'_>,
        request_id: u64,
        command: CommandPayload,
    ) -> Result<AckPayload, AuthenticatedCommandHandlerError>;

    fn handle_authenticated_resource(
        &self,
        _verified_session: &VerifiedPeerSession<'_>,
        _request_id: u64,
        _request: ResourceRequest,
    ) -> Result<(ResourceResponseKind, ResourceResult), AuthenticatedCommandHandlerError> {
        Err(AuthenticatedCommandHandlerError::Unavailable)
    }

    fn handle_authenticated_remote_buffer(
        &self,
        _verified_session: &VerifiedPeerSession<'_>,
        _request_id: u64,
        _request: RemoteBufferRequest,
    ) -> Result<(RemoteBufferResponseKind, BufferResult), AuthenticatedCommandHandlerError> {
        Err(AuthenticatedCommandHandlerError::Unavailable)
    }

    fn handle_authenticated_compute(
        &self,
        _verified_session: &VerifiedPeerSession<'_>,
        _request_id: u64,
        _request: ComputeRequest,
    ) -> Result<ComputeResponse, AuthenticatedCommandHandlerError> {
        Err(AuthenticatedCommandHandlerError::Unavailable)
    }

    fn authenticated_session_ended(
        &self,
        _verified_session: &VerifiedPeerSession<'_>,
    ) -> Result<(), AuthenticatedCommandHandlerError> {
        Ok(())
    }
}

struct NoAuthenticatedCommandHandler;

impl AuthenticatedCommandHandler for NoAuthenticatedCommandHandler {
    fn handle_authenticated_command(
        &self,
        _verified_session: &VerifiedPeerSession<'_>,
        _request_id: u64,
        _command: CommandPayload,
    ) -> Result<AckPayload, AuthenticatedCommandHandlerError> {
        Err(AuthenticatedCommandHandlerError::Unavailable)
    }
}

static NO_AUTHENTICATED_COMMAND_HANDLER: NoAuthenticatedCommandHandler =
    NoAuthenticatedCommandHandler;

impl VerifiedPeerSession<'_> {
    pub const fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    pub const fn session_id(&self) -> VerifiedSessionId {
        self.session_id
    }

    /// Returns whether the authenticated transport still owns this session.
    ///
    /// This is revocation state, not authentication authority: production
    /// mutation APIs must still require the opaque session proof itself.
    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }

    fn revoke(&self) {
        self.live.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointRole {
    LinuxInitiator,
    AndroidResponder,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeState {
    Unpaired,
    PairingXx,
    SasPending,
    LocalConfirmed,
    PeerConfirmed,
    MutualConfirmed,
    TrustCommitting,
    CommittedWaitingPeer,
    Paired,
    Authenticated,
    PairRejected,
    PairingFailed,
    Cooldown,
}

impl RuntimeState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unpaired => "UNPAIRED",
            Self::PairingXx => "PAIRING_XX",
            Self::SasPending => "SAS_PENDING",
            Self::LocalConfirmed => "LOCAL_CONFIRMED",
            Self::PeerConfirmed => "PEER_CONFIRMED",
            Self::MutualConfirmed => "MUTUAL_CONFIRMED",
            Self::TrustCommitting => "TRUST_COMMITTING",
            Self::CommittedWaitingPeer => "COMMITTED_WAITING_PEER",
            Self::Paired => "PAIRED",
            Self::Authenticated => "AUTHENTICATED",
            Self::PairRejected => "PAIR_REJECTED",
            Self::PairingFailed => "PAIRING_FAILED",
            Self::Cooldown => "PAIRING_COOLDOWN",
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct RuntimeSnapshot {
    pub state: RuntimeState,
    pub sas: Option<String>,
    pub authenticated: bool,
    pub heartbeat_count: u64,
    pub committed_peer_count: usize,
    pub mismatch_count: u8,
}

impl std::fmt::Debug for RuntimeSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeSnapshot")
            .field("state", &self.state)
            .field("sas", &self.sas.as_ref().map(|_| "REDACTED"))
            .field("authenticated", &self.authenticated)
            .field("heartbeat_count", &self.heartbeat_count)
            .field("committed_peer_count", &self.committed_peer_count)
            .field("mismatch_count", &self.mismatch_count)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PairingActionResult {
    Accepted,
    Duplicate,
    InvalidState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionOutcome {
    Lost,
    Cancelled,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeError {
    Storage(StorageError),
    Noise,
    Wire(SecureWireError),
    PairPersistFailed,
    PeerKeyMismatch,
    UnknownInitiatorIkRejected,
    PairingTimeout,
    PairCancelled,
    SasRejected,
    PairingCooldown,
    Pbmux,
    SessionLost,
    SessionBusy,
    NoConnectedTransport,
    CommandHandlerUnavailable,
    CommandHandlerFailed,
}

impl RuntimeError {
    pub const fn reason_code(&self) -> &'static str {
        match self {
            Self::Storage(StorageError::CorruptIdentity) => "STATE_CORRUPT",
            Self::Storage(StorageError::CorruptPeer | StorageError::CorruptGuard) => {
                "STATE_CORRUPT"
            }
            Self::Storage(_) | Self::PairPersistFailed => "PAIR_PERSIST_FAILED",
            Self::Noise | Self::Wire(SecureWireError::Crypto) => "SESSION_CRYPTO_ERROR",
            Self::Wire(_) | Self::SessionLost => "DEVICE_LOST",
            Self::PeerKeyMismatch => "PEER_KEY_MISMATCH",
            Self::UnknownInitiatorIkRejected => "UNKNOWN_INITIATOR_IK_REJECTED",
            Self::PairingTimeout => "PAIRING_TIMEOUT",
            Self::PairCancelled => "PAIR_CANCELLED",
            Self::SasRejected => "SAS_REJECTED",
            Self::PairingCooldown => "PAIRING_COOLDOWN",
            Self::Pbmux => "PAIRING_NOT_COMMITTED",
            Self::SessionBusy => "CONTROLLER_BUSY",
            Self::NoConnectedTransport => "DEVICE_LOST",
            Self::CommandHandlerUnavailable => "COMMAND_HANDLER_UNAVAILABLE",
            Self::CommandHandlerFailed => "COMMAND_HANDLER_FAILED",
        }
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.reason_code())
    }
}

impl std::error::Error for RuntimeError {}

impl From<StorageError> for RuntimeError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

impl From<SecureWireError> for RuntimeError {
    fn from(error: SecureWireError) -> Self {
        Self::Wire(error)
    }
}

struct RuntimeInner {
    state: RuntimeState,
    sas: Option<String>,
    begin_requested: bool,
    confirm_requested: bool,
    cancel_requested: bool,
    mismatch_requested: bool,
    authenticated: bool,
    heartbeat_count: u64,
    session_active: bool,
    mismatch_count: u8,
    #[cfg(test)]
    verified_peer_id: Option<PeerId>,
    #[cfg(test)]
    verified_session_mints: u64,
}

pub struct SecureRuntime {
    role: EndpointRole,
    identity: Identity,
    store: Mutex<StateStore>,
    peers: Mutex<Vec<PeerRecord>>,
    guard: Mutex<pb_secure::PairingGuard>,
    inner: Mutex<RuntimeInner>,
    changed: Condvar,
}

impl std::fmt::Debug for SecureRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SecureRuntime")
            .field("role", &self.role)
            .field("identity", &"REDACTED")
            .finish_non_exhaustive()
    }
}

impl SecureRuntime {
    pub fn initialize(role: EndpointRole, store: StateStore) -> Result<Self, RuntimeError> {
        let identity = store.load_or_create_identity()?;
        let peers = store.load_peers()?;
        let now = wall_clock_ms()?;
        let mut guard = store.load_guard(now)?;
        let admission = guard.admit(now);
        if admission.state_changed {
            store.persist_guard(&guard)?;
        }
        let state = if admission.value {
            if peers.is_empty() {
                RuntimeState::Unpaired
            } else {
                RuntimeState::Paired
            }
        } else {
            RuntimeState::Cooldown
        };
        let mismatch_count = guard.mismatch_count;
        Ok(Self {
            role,
            identity,
            store: Mutex::new(store),
            peers: Mutex::new(peers),
            guard: Mutex::new(guard),
            inner: Mutex::new(RuntimeInner {
                state,
                sas: None,
                begin_requested: false,
                confirm_requested: false,
                cancel_requested: false,
                mismatch_requested: false,
                authenticated: false,
                heartbeat_count: 0,
                session_active: false,
                mismatch_count,
                #[cfg(test)]
                verified_peer_id: None,
                #[cfg(test)]
                verified_session_mints: 0,
            }),
            changed: Condvar::new(),
        })
    }

    pub fn snapshot(&self) -> RuntimeSnapshot {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let committed_peer_count = self
            .peers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len();
        RuntimeSnapshot {
            state: inner.state,
            sas: inner.sas.clone(),
            authenticated: inner.authenticated,
            heartbeat_count: inner.heartbeat_count,
            committed_peer_count,
            mismatch_count: inner.mismatch_count,
        }
    }

    pub fn begin_pairing(&self, wait: Duration) -> Result<RuntimeSnapshot, RuntimeError> {
        if self.has_committed_peer() {
            return Ok(self.snapshot());
        }
        self.ensure_guard_admits()?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| RuntimeError::PairPersistFailed)?;
        if !inner.begin_requested {
            inner.begin_requested = true;
            inner.state = RuntimeState::PairingXx;
            self.changed.notify_all();
        }
        let deadline = Instant::now() + wait;
        while matches!(
            inner.state,
            RuntimeState::PairingXx | RuntimeState::Unpaired
        ) {
            let now = Instant::now();
            if now >= deadline {
                return Err(RuntimeError::NoConnectedTransport);
            }
            let remaining = deadline.saturating_duration_since(now);
            let (next, _) = self
                .changed
                .wait_timeout(inner, remaining)
                .map_err(|_| RuntimeError::PairPersistFailed)?;
            inner = next;
        }
        drop(inner);
        Ok(self.snapshot())
    }

    pub fn local_confirm(&self) -> PairingActionResult {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !matches!(
            inner.state,
            RuntimeState::SasPending
                | RuntimeState::PeerConfirmed
                | RuntimeState::LocalConfirmed
                | RuntimeState::MutualConfirmed
        ) {
            return PairingActionResult::InvalidState;
        }
        if inner.confirm_requested || matches!(inner.state, RuntimeState::LocalConfirmed) {
            return PairingActionResult::Duplicate;
        }
        inner.confirm_requested = true;
        self.changed.notify_all();
        PairingActionResult::Accepted
    }

    pub fn cancel(&self) -> PairingActionResult {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.sas.is_none() || inner.authenticated {
            return PairingActionResult::InvalidState;
        }
        if inner.cancel_requested {
            return PairingActionResult::Duplicate;
        }
        inner.cancel_requested = true;
        self.changed.notify_all();
        PairingActionResult::Accepted
    }

    pub fn mismatch(&self) -> PairingActionResult {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.sas.is_none() || inner.authenticated {
            return PairingActionResult::InvalidState;
        }
        if inner.mismatch_requested {
            return PairingActionResult::Duplicate;
        }
        inner.mismatch_requested = true;
        self.changed.notify_all();
        PairingActionResult::Accepted
    }

    pub fn has_committed_peer(&self) -> bool {
        !self
            .peers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty()
    }

    pub fn session_requested(&self) -> bool {
        if self.has_committed_peer() {
            return true;
        }
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .begin_requested
    }

    fn one_committed_peer(&self) -> Result<Option<PeerRecord>, RuntimeError> {
        let peers = self
            .peers
            .lock()
            .map_err(|_| RuntimeError::PairPersistFailed)?;
        match peers.as_slice() {
            [] => Ok(None),
            [peer] => Ok(Some(peer.clone())),
            _ => Err(RuntimeError::PeerKeyMismatch),
        }
    }

    fn wait_for_begin(&self) -> Result<(), RuntimeError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| RuntimeError::PairPersistFailed)?;
        while !inner.begin_requested {
            inner = self
                .changed
                .wait(inner)
                .map_err(|_| RuntimeError::PairPersistFailed)?;
        }
        Ok(())
    }

    fn begin_session(&self, xx: bool) -> Result<(), RuntimeError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| RuntimeError::PairPersistFailed)?;
        if inner.session_active {
            return Err(RuntimeError::SessionBusy);
        }
        inner.session_active = true;
        inner.authenticated = false;
        #[cfg(test)]
        {
            inner.verified_peer_id = None;
        }
        inner.sas = None;
        inner.confirm_requested = false;
        inner.cancel_requested = false;
        inner.mismatch_requested = false;
        inner.state = if xx {
            RuntimeState::PairingXx
        } else {
            RuntimeState::Paired
        };
        self.changed.notify_all();
        Ok(())
    }

    fn end_session(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.session_active = false;
        inner.authenticated = false;
        #[cfg(test)]
        {
            inner.verified_peer_id = None;
        }
        inner.sas = None;
        inner.begin_requested = false;
        inner.confirm_requested = false;
        inner.cancel_requested = false;
        inner.mismatch_requested = false;
        if !matches!(
            inner.state,
            RuntimeState::PairRejected | RuntimeState::PairingFailed
        ) {
            inner.state = if self.has_committed_peer() {
                RuntimeState::Paired
            } else {
                RuntimeState::Unpaired
            };
        }
        self.changed.notify_all();
    }

    fn set_sas(&self, sas: String) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.sas = Some(sas);
        inner.state = RuntimeState::SasPending;
        self.changed.notify_all();
    }

    fn set_pairing_state(&self, state: PairingState) {
        let state = match state {
            PairingState::Unpaired => RuntimeState::Unpaired,
            PairingState::PairingXx => RuntimeState::PairingXx,
            PairingState::SasPending => RuntimeState::SasPending,
            PairingState::LocalConfirmed => RuntimeState::LocalConfirmed,
            PairingState::PeerConfirmed => RuntimeState::PeerConfirmed,
            PairingState::MutualConfirmed => RuntimeState::MutualConfirmed,
            PairingState::TrustCommitting => RuntimeState::TrustCommitting,
            PairingState::Paired => RuntimeState::CommittedWaitingPeer,
            PairingState::PairRejected => RuntimeState::PairRejected,
            PairingState::PairingFailed => RuntimeState::PairingFailed,
        };
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.state = state;
        self.changed.notify_all();
    }

    fn take_actions(&self) -> (bool, bool, bool) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (
            std::mem::take(&mut inner.confirm_requested),
            std::mem::take(&mut inner.cancel_requested),
            std::mem::take(&mut inner.mismatch_requested),
        )
    }

    fn commit_peer(&self, key: [u8; 32]) -> Result<(), RuntimeError> {
        let now = wall_clock_ms()?;
        let alias = match self.role {
            EndpointRole::LinuxInitiator => "Android worker",
            EndpointRole::AndroidResponder => "Linux host",
        };
        let record = PeerRecord::new(key, alias, now);
        self.store
            .lock()
            .map_err(|_| RuntimeError::PairPersistFailed)?
            .commit_peer(&record)
            .map_err(|_| RuntimeError::PairPersistFailed)?;
        let mut peers = self
            .peers
            .lock()
            .map_err(|_| RuntimeError::PairPersistFailed)?;
        if !peers.iter().any(|peer| peer.peer_id == record.peer_id) {
            peers.push(record);
        }
        Ok(())
    }

    fn mark_authenticated<'session>(
        &self,
        remote: &[u8; 32],
        _scope: &'session SessionScope,
    ) -> Result<VerifiedPeerSession<'session>, RuntimeError> {
        let now = wall_clock_ms()?;
        let mut guard = self
            .guard
            .lock()
            .map_err(|_| RuntimeError::PairPersistFailed)?;
        guard.paired(now);
        self.store
            .lock()
            .map_err(|_| RuntimeError::PairPersistFailed)?
            .persist_guard(&guard)?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| RuntimeError::PairPersistFailed)?;
        inner.state = RuntimeState::Authenticated;
        inner.authenticated = true;
        inner.mismatch_count = guard.mismatch_count;
        let verified = VerifiedPeerSession {
            peer_id: PeerId::from_static_public_key(remote),
            session_id: VerifiedSessionId(random_nonzero_128()?),
            live: &_scope.live,
            _scope: std::marker::PhantomData,
        };
        #[cfg(test)]
        {
            inner.verified_peer_id = Some(*verified.peer_id());
            inner.verified_session_mints = inner.verified_session_mints.saturating_add(1);
        }
        self.changed.notify_all();
        Ok(verified)
    }

    fn heartbeat(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.heartbeat_count = inner.heartbeat_count.saturating_add(1);
        self.changed.notify_all();
    }

    fn record_mismatch(&self) -> Result<(), RuntimeError> {
        let now = wall_clock_ms()?;
        let mut guard = self
            .guard
            .lock()
            .map_err(|_| RuntimeError::PairPersistFailed)?;
        guard.record_mismatch(now);
        self.store
            .lock()
            .map_err(|_| RuntimeError::PairPersistFailed)?
            .persist_guard(&guard)?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| RuntimeError::PairPersistFailed)?;
        inner.mismatch_count = guard.mismatch_count;
        inner.state = if guard.cooldown_until_wall_ms.is_some() {
            RuntimeState::Cooldown
        } else {
            RuntimeState::PairRejected
        };
        self.changed.notify_all();
        Ok(())
    }

    fn ensure_guard_admits(&self) -> Result<(), RuntimeError> {
        let now = wall_clock_ms()?;
        let mut guard = self
            .guard
            .lock()
            .map_err(|_| RuntimeError::PairPersistFailed)?;
        let admission = guard.admit(now);
        if admission.state_changed {
            self.store
                .lock()
                .map_err(|_| RuntimeError::PairPersistFailed)?
                .persist_guard(&guard)?;
        }
        if !admission.value {
            return Err(RuntimeError::PairingCooldown);
        }
        Ok(())
    }

    fn peer_is_committed(&self, key: &[u8; 32]) -> bool {
        self.peers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .any(|peer| &peer.static_public_key == key)
    }
}

struct SessionGuard<'a> {
    runtime: &'a SecureRuntime,
}

impl Drop for SessionGuard<'_> {
    fn drop(&mut self) {
        self.runtime.end_session();
    }
}

pub fn run_initiator_session(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
) -> Result<SessionOutcome, RuntimeError> {
    run_initiator_session_internal(stream, runtime, None)
}

pub fn run_initiator_session_with_client(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
    mut driver: InitiatorSessionDriver,
) -> Result<SessionOutcome, RuntimeError> {
    run_initiator_session_internal(stream, runtime, Some(&mut driver))
}

fn run_initiator_session_internal(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
    mut initiator_driver: Option<&mut InitiatorSessionDriver>,
) -> Result<SessionOutcome, RuntimeError> {
    let committed = runtime.one_committed_peer()?;
    if committed.is_none() {
        runtime.wait_for_begin()?;
        runtime.ensure_guard_admits()?;
    }
    runtime.begin_session(committed.is_none())?;
    let _session = SessionGuard { runtime };
    configure_handshake_stream(stream)?;
    match committed {
        Some(peer) => {
            let (transport, remote) = initiator_ik(stream, runtime.identity.private(), &peer)?;
            run_committed_loop(
                stream,
                runtime,
                &NO_AUTHENTICATED_COMMAND_HANDLER,
                transport,
                remote,
                true,
                initiator_driver.as_deref_mut(),
            )
        }
        None => {
            let (transport, remote) = initiator_xx(stream, runtime)?;
            run_pairing_loop(
                stream,
                runtime,
                &NO_AUTHENTICATED_COMMAND_HANDLER,
                transport,
                remote,
                true,
                initiator_driver.as_deref_mut(),
            )
        }
    }
}

pub fn run_responder_session(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
    prefix: &[u8],
) -> Result<SessionOutcome, RuntimeError> {
    run_responder_session_with_handler(stream, runtime, prefix, &NO_AUTHENTICATED_COMMAND_HANDLER)
}

pub fn run_responder_session_with_handler(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
    prefix: &[u8],
    command_handler: &dyn AuthenticatedCommandHandler,
) -> Result<SessionOutcome, RuntimeError> {
    configure_handshake_stream(stream)?;
    let first = read_record_prefixed(stream, prefix)?;
    let xx = first.len() == XX_FIRST_MESSAGE_BYTES;
    if xx {
        runtime.ensure_guard_admits()?;
    }
    runtime.begin_session(xx)?;
    let _session = SessionGuard { runtime };
    if xx {
        let (transport, remote) = responder_xx(stream, runtime, &first)?;
        run_pairing_loop(
            stream,
            runtime,
            command_handler,
            transport,
            remote,
            false,
            None,
        )
    } else {
        let (transport, remote) = responder_ik(stream, runtime, &first)?;
        run_committed_loop(
            stream,
            runtime,
            command_handler,
            transport,
            remote,
            false,
            None,
        )
    }
}

fn configure_handshake_stream(stream: &TcpStream) -> Result<(), RuntimeError> {
    stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT)))
        .and_then(|()| stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT)))
        .map_err(|_| RuntimeError::SessionLost)
}

fn initiator_xx(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
) -> Result<(TransportState, [u8; 32]), RuntimeError> {
    let mut handshake =
        production_xx_initiator(runtime.identity.private()).map_err(|_| RuntimeError::Noise)?;
    write_handshake_message(stream, &mut handshake)?;
    read_handshake_message(stream, &mut handshake, None)?;
    write_handshake_message(stream, &mut handshake)?;
    finish_xx(runtime, handshake)
}

fn responder_xx(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
    first: &[u8],
) -> Result<(TransportState, [u8; 32]), RuntimeError> {
    let mut handshake =
        production_xx_responder(runtime.identity.private()).map_err(|_| RuntimeError::Noise)?;
    read_handshake_message(stream, &mut handshake, Some(first))?;
    write_handshake_message(stream, &mut handshake)?;
    read_handshake_message(stream, &mut handshake, None)?;
    finish_xx(runtime, handshake)
}

fn finish_xx(
    runtime: &SecureRuntime,
    handshake: HandshakeState,
) -> Result<(TransportState, [u8; 32]), RuntimeError> {
    if !handshake.is_handshake_finished() {
        return Err(RuntimeError::Noise);
    }
    let remote: [u8; 32] = handshake
        .get_remote_static()
        .ok_or(RuntimeError::Noise)?
        .try_into()
        .map_err(|_| RuntimeError::Noise)?;
    let hash: [u8; 32] = handshake
        .get_handshake_hash()
        .try_into()
        .map_err(|_| RuntimeError::Noise)?;
    let sas = derive_sas(&hash).map_err(|_| RuntimeError::Noise)?;
    runtime.set_sas(sas);
    let transport = handshake
        .into_transport_mode()
        .map_err(|_| RuntimeError::Noise)?;
    Ok((transport, remote))
}

fn ik_params() -> Result<NoiseParams, RuntimeError> {
    NOISE_IK_NAME.parse().map_err(|_| RuntimeError::Noise)
}

fn initiator_ik(
    stream: &mut TcpStream,
    private: &[u8; 32],
    peer: &PeerRecord,
) -> Result<(TransportState, [u8; 32]), RuntimeError> {
    let mut handshake = Builder::new(ik_params()?)
        .local_private_key(private)
        .remote_public_key(&peer.static_public_key)
        .prologue(PROLOGUE)
        .build_initiator()
        .map_err(|_| RuntimeError::PeerKeyMismatch)?;
    write_handshake_message(stream, &mut handshake).map_err(|_| RuntimeError::PeerKeyMismatch)?;
    read_handshake_message(stream, &mut handshake, None)
        .map_err(|_| RuntimeError::PeerKeyMismatch)?;
    if !handshake.is_handshake_finished() {
        return Err(RuntimeError::PeerKeyMismatch);
    }
    let remote: [u8; 32] = handshake
        .get_remote_static()
        .ok_or(RuntimeError::PeerKeyMismatch)?
        .try_into()
        .map_err(|_| RuntimeError::PeerKeyMismatch)?;
    if remote != peer.static_public_key {
        return Err(RuntimeError::PeerKeyMismatch);
    }
    let transport = handshake
        .into_transport_mode()
        .map_err(|_| RuntimeError::PeerKeyMismatch)?;
    Ok((transport, remote))
}

fn responder_ik(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
    first: &[u8],
) -> Result<(TransportState, [u8; 32]), RuntimeError> {
    let mut handshake = Builder::new(ik_params()?)
        .local_private_key(runtime.identity.private())
        .prologue(PROLOGUE)
        .build_responder()
        .map_err(|_| RuntimeError::UnknownInitiatorIkRejected)?;
    read_handshake_message(stream, &mut handshake, Some(first))
        .map_err(|_| RuntimeError::UnknownInitiatorIkRejected)?;
    let remote: [u8; 32] = handshake
        .get_remote_static()
        .ok_or(RuntimeError::UnknownInitiatorIkRejected)?
        .try_into()
        .map_err(|_| RuntimeError::UnknownInitiatorIkRejected)?;
    if !runtime.peer_is_committed(&remote) {
        return Err(RuntimeError::UnknownInitiatorIkRejected);
    }
    write_handshake_message(stream, &mut handshake)
        .map_err(|_| RuntimeError::UnknownInitiatorIkRejected)?;
    if !handshake.is_handshake_finished() {
        return Err(RuntimeError::UnknownInitiatorIkRejected);
    }
    let transport = handshake
        .into_transport_mode()
        .map_err(|_| RuntimeError::UnknownInitiatorIkRejected)?;
    Ok((transport, remote))
}

fn write_handshake_message(
    stream: &mut TcpStream,
    handshake: &mut HandshakeState,
) -> Result<(), RuntimeError> {
    let mut output = [0_u8; u16::MAX as usize];
    let length = handshake
        .write_message(&[], &mut output)
        .map_err(|_| RuntimeError::Noise)?;
    write_record(stream, &output[..length])?;
    Ok(())
}

fn read_handshake_message(
    stream: &mut TcpStream,
    handshake: &mut HandshakeState,
    supplied: Option<&[u8]>,
) -> Result<(), RuntimeError> {
    let owned;
    let message = if let Some(message) = supplied {
        message
    } else {
        owned = read_record(stream)?;
        &owned
    };
    let mut plaintext = [0_u8; u16::MAX as usize];
    handshake
        .read_message(message, &mut plaintext)
        .map_err(|_| RuntimeError::Noise)?;
    Ok(())
}

fn read_record_prefixed(stream: &mut TcpStream, prefix: &[u8]) -> Result<Vec<u8>, RuntimeError> {
    if prefix.is_empty() {
        return read_record(stream).map_err(Into::into);
    }
    if prefix.len() > 2 {
        return Err(RuntimeError::SessionLost);
    }
    let mut length_bytes = [0_u8; 2];
    length_bytes[..prefix.len()].copy_from_slice(prefix);
    if prefix.len() < 2 {
        stream
            .read_exact(&mut length_bytes[prefix.len()..])
            .map_err(|_| RuntimeError::SessionLost)?;
    }
    let length = usize::from(u16::from_be_bytes(length_bytes));
    if length == 0 {
        return Err(RuntimeError::SessionLost);
    }
    let mut message = vec![0_u8; length];
    stream
        .read_exact(&mut message)
        .map_err(|_| RuntimeError::SessionLost)?;
    Ok(message)
}

fn run_pairing_loop(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
    command_handler: &dyn AuthenticatedCommandHandler,
    mut transport: TransportState,
    remote: [u8; 32],
    initiator: bool,
    mut initiator_driver: Option<&mut InitiatorSessionDriver>,
) -> Result<SessionOutcome, RuntimeError> {
    let mut actor = PairingActor::new();
    let started = Instant::now();
    let mut send_sequence = 0_u64;
    let mut receive_sequence = SequenceTracker::default();
    let mut committed = false;
    let mut commit_ping_request = None;
    configure_session_stream(stream)?;

    loop {
        if started.elapsed() >= Duration::from_millis(PAIRING_TIMEOUT_MS) {
            runtime.set_pairing_state(PairingState::PairingFailed);
            return Err(RuntimeError::PairingTimeout);
        }
        let (confirm, cancel, mismatch) = runtime.take_actions();
        if cancel {
            runtime.set_pairing_state(PairingState::Unpaired);
            return Ok(SessionOutcome::Cancelled);
        }
        if mismatch {
            runtime.record_mismatch()?;
            return Ok(SessionOutcome::Rejected);
        }
        if confirm {
            let transition = actor.local_confirm();
            runtime.set_pairing_state(transition.value.state);
            if transition.value.send_pair_confirm {
                let frame = pair_confirm_frame(random_nonzero_u64()?, send_sequence)
                    .map_err(|_| RuntimeError::Pbmux)?;
                send_sequence = send_sequence.checked_add(1).ok_or(RuntimeError::Pbmux)?;
                send_frame(stream, &mut transport, &frame)?;
            }
        }

        if let Some(frame) = receive_frame_if_available(stream, &mut transport)? {
            receive_sequence
                .accept(frame.header.sequence)
                .map_err(|_| RuntimeError::Pbmux)?;
            authorize_dispatch(&frame, DispatchMode::PairingControlOnly)
                .map_err(|_| RuntimeError::Pbmux)?;
            match ControlType::try_from(frame.header.message_type) {
                Ok(ControlType::PairConfirm) => {
                    validate_pair_confirm(&frame).map_err(|_| RuntimeError::Pbmux)?;
                    let transition = actor.peer_confirm();
                    runtime.set_pairing_state(transition.value.state);
                }
                Ok(ControlType::Ping) if committed && !initiator => {
                    let pong =
                        control_frame(ControlType::Pong, frame.header.request_id, send_sequence);
                    send_sequence = send_sequence.checked_add(1).ok_or(RuntimeError::Pbmux)?;
                    send_frame(stream, &mut transport, &pong)?;
                    return enter_authenticated_loop(
                        stream,
                        runtime,
                        command_handler,
                        transport,
                        remote,
                        send_sequence,
                        receive_sequence,
                        false,
                        None,
                    );
                }
                Ok(ControlType::Pong) if committed && initiator => {
                    if Some(frame.header.request_id) != commit_ping_request {
                        return Err(RuntimeError::Pbmux);
                    }
                    return enter_authenticated_loop(
                        stream,
                        runtime,
                        command_handler,
                        transport,
                        remote,
                        send_sequence,
                        receive_sequence,
                        true,
                        initiator_driver.as_deref_mut(),
                    );
                }
                Ok(ControlType::SessionClose) => return Ok(SessionOutcome::Lost),
                _ => {}
            }
        }

        if actor.state() == PairingState::MutualConfirmed && !committed {
            let transition = actor.begin_trust_commit();
            runtime.set_pairing_state(transition.value.state);
            if transition.value.persist_commit {
                if runtime.commit_peer(remote).is_err() {
                    let _ = actor.persist_result(PersistOutcome::Failed);
                    runtime.set_pairing_state(PairingState::PairingFailed);
                    return Err(RuntimeError::PairPersistFailed);
                }
                actor
                    .persist_result(PersistOutcome::Succeeded)
                    .map_err(|_| RuntimeError::PairPersistFailed)?;
                runtime.set_pairing_state(PairingState::Paired);
                committed = true;
                if initiator {
                    let request = random_nonzero_u64()?;
                    let ping = control_frame(ControlType::Ping, request, send_sequence);
                    send_sequence = send_sequence.checked_add(1).ok_or(RuntimeError::Pbmux)?;
                    send_frame(stream, &mut transport, &ping)?;
                    commit_ping_request = Some(request);
                }
            }
        } else {
            std::thread::sleep(SESSION_POLL);
        }
    }
}

fn run_committed_loop(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
    command_handler: &dyn AuthenticatedCommandHandler,
    mut transport: TransportState,
    remote: [u8; 32],
    initiator: bool,
    mut initiator_driver: Option<&mut InitiatorSessionDriver>,
) -> Result<SessionOutcome, RuntimeError> {
    configure_session_stream(stream)?;
    let mut send_sequence = 0_u64;
    let mut receive_sequence = SequenceTracker::default();
    if initiator {
        let request = random_nonzero_u64()?;
        send_frame(
            stream,
            &mut transport,
            &control_frame(ControlType::Ping, request, send_sequence),
        )?;
        send_sequence += 1;
        loop {
            if let Some(frame) = receive_frame_if_available(stream, &mut transport)? {
                receive_sequence
                    .accept(frame.header.sequence)
                    .map_err(|_| RuntimeError::Pbmux)?;
                if frame.header.message_type == ControlType::Pong as u16
                    && frame.header.request_id == request
                {
                    return enter_authenticated_loop(
                        stream,
                        runtime,
                        command_handler,
                        transport,
                        remote,
                        send_sequence,
                        receive_sequence,
                        true,
                        initiator_driver.as_deref_mut(),
                    );
                }
                return Err(RuntimeError::Pbmux);
            }
            std::thread::sleep(SESSION_POLL);
        }
    }
    loop {
        if let Some(frame) = receive_frame_if_available(stream, &mut transport)? {
            receive_sequence
                .accept(frame.header.sequence)
                .map_err(|_| RuntimeError::Pbmux)?;
            if frame.header.message_type != ControlType::Ping as u16 {
                return Err(RuntimeError::Pbmux);
            }
            let pong = control_frame(ControlType::Pong, frame.header.request_id, send_sequence);
            send_frame(stream, &mut transport, &pong)?;
            send_sequence += 1;
            return enter_authenticated_loop(
                stream,
                runtime,
                command_handler,
                transport,
                remote,
                send_sequence,
                receive_sequence,
                false,
                None,
            );
        }
        std::thread::sleep(SESSION_POLL);
    }
}

fn enter_authenticated_loop(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
    command_handler: &dyn AuthenticatedCommandHandler,
    transport: TransportState,
    remote: [u8; 32],
    send_sequence: u64,
    receive_sequence: SequenceTracker,
    initiator: bool,
    mut initiator_driver: Option<&mut InitiatorSessionDriver>,
) -> Result<SessionOutcome, RuntimeError> {
    let scope = SessionScope::new();
    let verified_peer = runtime.mark_authenticated(&remote, &scope)?;
    let initiator_generation = initiator_driver
        .as_deref_mut()
        .map(|driver| driver.begin_session(*verified_peer.peer_id()));
    runtime.heartbeat();
    let result = run_authenticated_loop(
        stream,
        runtime,
        command_handler,
        transport,
        &verified_peer,
        send_sequence,
        receive_sequence,
        initiator,
        initiator_driver.as_deref_mut(),
        initiator_generation,
    );
    verified_peer.revoke();
    if let Some(driver) = initiator_driver.as_deref_mut() {
        driver.end_session();
    }
    command_handler
        .authenticated_session_ended(&verified_peer)
        .map_err(|_| RuntimeError::CommandHandlerFailed)?;
    result
}

fn run_authenticated_loop(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
    command_handler: &dyn AuthenticatedCommandHandler,
    mut transport: TransportState,
    verified_peer: &VerifiedPeerSession<'_>,
    mut send_sequence: u64,
    mut receive_sequence: SequenceTracker,
    initiator: bool,
    mut initiator_driver: Option<&mut InitiatorSessionDriver>,
    initiator_generation: Option<u64>,
) -> Result<SessionOutcome, RuntimeError> {
    let mut next_heartbeat = Instant::now() + HEARTBEAT_INTERVAL;
    let mut last_authenticated_traffic = Instant::now();
    let mut heartbeat_outstanding = None;
    let mut application_outstanding = None;
    let mut reassembler = Reassembler::default();
    loop {
        if initiator_driver
            .as_deref()
            .is_some_and(InitiatorSessionDriver::cancelled)
        {
            return Ok(SessionOutcome::Cancelled);
        }
        if last_authenticated_traffic.elapsed() >= DEVICE_OFFLINE_TIMEOUT {
            verified_peer.revoke();
            return Err(RuntimeError::SessionLost);
        }
        if initiator
            && Instant::now() >= next_heartbeat
            && heartbeat_outstanding.is_none()
            && application_outstanding.is_none()
        {
            let request = random_nonzero_u64()?;
            send_frame(
                stream,
                &mut transport,
                &control_frame(ControlType::Ping, request, send_sequence),
            )?;
            send_sequence = send_sequence.checked_add(1).ok_or(RuntimeError::Pbmux)?;
            heartbeat_outstanding = Some(request);
            next_heartbeat = Instant::now() + HEARTBEAT_INTERVAL;
        } else if initiator && heartbeat_outstanding.is_none() && application_outstanding.is_none()
        {
            if let (Some(driver), Some(generation)) =
                (initiator_driver.as_deref_mut(), initiator_generation)
                && let Some(pending) = driver.next_request(generation)
            {
                application_outstanding =
                    start_initiator_request(stream, &mut transport, &mut send_sequence, pending)?;
            }
        }
        if let Some(frame) = receive_frame_if_available(stream, &mut transport)? {
            receive_sequence
                .accept(frame.header.sequence)
                .map_err(|_| RuntimeError::Pbmux)?;
            authorize_dispatch(&frame, DispatchMode::Committed).map_err(|_| RuntimeError::Pbmux)?;
            last_authenticated_traffic = Instant::now();
            if frame.header.channel == Channel::Control {
                match ControlType::try_from(frame.header.message_type) {
                    Ok(ControlType::Ping) if !initiator => {
                        let pong = control_frame(
                            ControlType::Pong,
                            frame.header.request_id,
                            send_sequence,
                        );
                        send_sequence = send_sequence.checked_add(1).ok_or(RuntimeError::Pbmux)?;
                        send_frame(stream, &mut transport, &pong)?;
                        runtime.heartbeat();
                    }
                    Ok(ControlType::Pong) if initiator => {
                        if heartbeat_outstanding != Some(frame.header.request_id) {
                            return Err(RuntimeError::Pbmux);
                        }
                        heartbeat_outstanding = None;
                        runtime.heartbeat();
                        if let Some(driver) = initiator_driver.as_deref() {
                            driver.mark_liveness();
                        }
                    }
                    Ok(ControlType::Command) => {
                        let command = parse_authenticated_command(&frame)?;
                        let ack = command_handler
                            .handle_authenticated_command(
                                verified_peer,
                                frame.header.request_id,
                                command,
                            )
                            .map_err(|error| match error {
                                AuthenticatedCommandHandlerError::Unavailable => {
                                    RuntimeError::CommandHandlerUnavailable
                                }
                                AuthenticatedCommandHandlerError::Failed => {
                                    RuntimeError::CommandHandlerFailed
                                }
                            })?;
                        if ack.command_seq != command.command_seq || !matches!(ack.ack_state, 2 | 3)
                        {
                            return Err(RuntimeError::CommandHandlerFailed);
                        }
                        let ack_frame =
                            build_command_ack_frame(&ack, frame.header.request_id, send_sequence)
                                .map_err(|_| RuntimeError::Pbmux)?;
                        send_sequence = send_sequence.checked_add(1).ok_or(RuntimeError::Pbmux)?;
                        send_frame(stream, &mut transport, &ack_frame)?;
                    }
                    Ok(ControlType::CommandAck) => {
                        let ack =
                            parse_command_ack_frame(&frame).map_err(|_| RuntimeError::Pbmux)?;
                        if initiator {
                            let expected = take_initiator_outstanding(
                                &mut application_outstanding,
                                frame.header.request_id,
                            )?;
                            if !matches!(
                                expected.expected,
                                ExpectedInitiatorResponse::Command(command_seq)
                                    if command_seq == ack.command_seq
                            ) {
                                return Err(RuntimeError::Pbmux);
                            }
                            expected.complete(InitiatorResponse::Command(ack));
                        }
                    }
                    Ok(ControlType::SessionClose) => return Ok(SessionOutcome::Lost),
                    _ => return Err(RuntimeError::Pbmux),
                }
            } else if frame.header.channel == Channel::Resource {
                if initiator {
                    let (kind, result) =
                        parse_resource_result_frame(&frame).map_err(|_| RuntimeError::Pbmux)?;
                    let expected = take_initiator_outstanding(
                        &mut application_outstanding,
                        frame.header.request_id,
                    )?;
                    if expected.expected != ExpectedInitiatorResponse::Resource(kind) {
                        return Err(RuntimeError::Pbmux);
                    }
                    expected.complete(InitiatorResponse::Resource(result));
                } else {
                    let request =
                        parse_resource_request_frame(&frame).map_err(|_| RuntimeError::Pbmux)?;
                    let expected_kind = match &request {
                        ResourceRequest::Reserve { .. } => ResourceResponseKind::ReserveAck,
                        ResourceRequest::Commit { .. } => ResourceResponseKind::Commit,
                        ResourceRequest::Release { .. } => ResourceResponseKind::Release,
                    };
                    let (kind, result) = command_handler
                        .handle_authenticated_resource(
                            verified_peer,
                            frame.header.request_id,
                            request,
                        )
                        .map_err(handler_runtime_error)?;
                    if kind != expected_kind {
                        return Err(RuntimeError::CommandHandlerFailed);
                    }
                    let response = build_resource_result_frame(
                        kind,
                        &result,
                        frame.header.request_id,
                        send_sequence,
                    )
                    .map_err(|_| RuntimeError::CommandHandlerFailed)?;
                    send_sequence = send_sequence.checked_add(1).ok_or(RuntimeError::Pbmux)?;
                    send_frame(stream, &mut transport, &response)?;
                }
            } else if frame.header.channel == Channel::RemoteBuffer {
                if initiator {
                    validate_remote_buffer_result_fragment(&frame)
                        .map_err(|_| RuntimeError::Pbmux)?;
                    let message_type = frame.header.message_type;
                    let request_id = frame.header.request_id;
                    let expected = application_outstanding
                        .as_ref()
                        .ok_or(RuntimeError::Pbmux)?;
                    if expected.request_id != request_id {
                        return Err(RuntimeError::Pbmux);
                    }
                    if let Some(payload) =
                        reassembler.accept(frame).map_err(|_| RuntimeError::Pbmux)?
                    {
                        let kind = remote_response_kind(message_type)?;
                        let result = parse_remote_buffer_result_payload(kind, &payload)
                            .map_err(|_| RuntimeError::Pbmux)?;
                        let expected =
                            take_initiator_outstanding(&mut application_outstanding, request_id)?;
                        if expected.expected != ExpectedInitiatorResponse::RemoteBuffer(kind) {
                            return Err(RuntimeError::Pbmux);
                        }
                        expected.complete(InitiatorResponse::RemoteBuffer(result));
                    }
                } else {
                    validate_remote_buffer_request_fragment(&frame)
                        .map_err(|_| RuntimeError::Pbmux)?;
                    let message_type = frame.header.message_type;
                    let request_id = frame.header.request_id;
                    if let Some(payload) =
                        reassembler.accept(frame).map_err(|_| RuntimeError::Pbmux)?
                    {
                        let request = parse_remote_buffer_request_payload(message_type, &payload)
                            .map_err(|_| RuntimeError::Pbmux)?;
                        let expected_kind = match &request {
                            RemoteBufferRequest::Alloc { .. } => RemoteBufferResponseKind::AllocAck,
                            RemoteBufferRequest::Put { .. } => RemoteBufferResponseKind::Put,
                            RemoteBufferRequest::Get { .. } => RemoteBufferResponseKind::Data,
                            RemoteBufferRequest::Free { .. } => RemoteBufferResponseKind::Free,
                            RemoteBufferRequest::Stat { .. } => RemoteBufferResponseKind::Stat,
                            RemoteBufferRequest::Touch { .. } => RemoteBufferResponseKind::Touch,
                        };
                        let (kind, result) = handle_remote_buffer_with_transport_liveness(
                            stream,
                            command_handler,
                            verified_peer,
                            request_id,
                            request,
                            last_authenticated_traffic,
                        )?;
                        if kind != expected_kind {
                            return Err(RuntimeError::CommandHandlerFailed);
                        }
                        let responses = build_remote_buffer_result_frames(
                            kind,
                            &result,
                            request_id,
                            send_sequence,
                        )
                        .map_err(|_| RuntimeError::CommandHandlerFailed)?;
                        send_sequence = send_sequence
                            .checked_add(responses.len() as u64)
                            .ok_or(RuntimeError::Pbmux)?;
                        for response in responses {
                            send_frame(stream, &mut transport, &response)?;
                        }
                    }
                }
            } else if frame.header.channel == Channel::Compute {
                if initiator {
                    let response =
                        parse_compute_response_frame(&frame).map_err(|_| RuntimeError::Pbmux)?;
                    let expected = take_initiator_outstanding(
                        &mut application_outstanding,
                        frame.header.request_id,
                    )?;
                    if !expected.expected.accepts_compute(&response) {
                        return Err(RuntimeError::Pbmux);
                    }
                    expected.complete(InitiatorResponse::Compute(response));
                } else {
                    let request =
                        parse_compute_request_frame(&frame).map_err(|_| RuntimeError::Pbmux)?;
                    let response = handle_compute_with_transport_liveness(
                        stream,
                        command_handler,
                        verified_peer,
                        frame.header.request_id,
                        request,
                        last_authenticated_traffic,
                    )?;
                    let valid_response_kind = matches!(
                        (&request, &response),
                        (ComputeRequest::Submit(_), ComputeResponse::Status(_))
                            | (ComputeRequest::Submit(_), ComputeResponse::Result(_))
                            | (ComputeRequest::Status(_), ComputeResponse::Status(_))
                            | (ComputeRequest::Status(_), ComputeResponse::Result(_))
                            | (ComputeRequest::Cancel(_), ComputeResponse::Cancel(_))
                    );
                    if !valid_response_kind {
                        return Err(RuntimeError::CommandHandlerFailed);
                    }
                    let response = build_compute_response_frame(
                        &response,
                        frame.header.request_id,
                        send_sequence,
                    )
                    .map_err(|_| RuntimeError::CommandHandlerFailed)?;
                    send_sequence = send_sequence.checked_add(1).ok_or(RuntimeError::Pbmux)?;
                    send_frame(stream, &mut transport, &response)?;
                }
            } else if frame.header.channel == Channel::Metrics {
                if frame.header.message_type == 1 {
                    parse_heartbeat_frame(&frame).map_err(|_| RuntimeError::Pbmux)?;
                    runtime.heartbeat();
                    if let Some(driver) = initiator_driver.as_deref() {
                        driver.mark_liveness();
                    }
                } else {
                    return Err(RuntimeError::Pbmux);
                }
            } else {
                return Err(RuntimeError::Pbmux);
            }
        }
        std::thread::sleep(SESSION_POLL);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExpectedInitiatorResponse {
    Command(u64),
    Resource(ResourceResponseKind),
    RemoteBuffer(RemoteBufferResponseKind),
    ComputeSubmit,
    ComputeStatus([u8; 16]),
    ComputeCancel([u8; 16]),
}

impl ExpectedInitiatorResponse {
    fn accepts_compute(self, response: &ComputeResponse) -> bool {
        match (self, response) {
            (Self::ComputeSubmit, ComputeResponse::Status(_) | ComputeResponse::Result(_)) => true,
            (Self::ComputeStatus(job_id), ComputeResponse::Status(status)) => {
                status.job.as_ref().is_none_or(|job| job.job_id == job_id)
            }
            (Self::ComputeStatus(job_id), ComputeResponse::Result(result)) => {
                result.job.as_ref().is_none_or(|job| job.job_id == job_id)
            }
            (Self::ComputeCancel(job_id), ComputeResponse::Cancel(status)) => {
                status.job.as_ref().is_none_or(|job| job.job_id == job_id)
            }
            _ => false,
        }
    }
}

struct InitiatorOutstanding {
    request_id: u64,
    expected: ExpectedInitiatorResponse,
    response: Option<std::sync::mpsc::SyncSender<Result<InitiatorResponse, InitiatorClientError>>>,
}

impl InitiatorOutstanding {
    fn complete(mut self, response: InitiatorResponse) {
        if let Some(sender) = self.response.take() {
            let _ = sender.send(Ok(response));
        }
    }
}

impl Drop for InitiatorOutstanding {
    fn drop(&mut self) {
        if let Some(sender) = self.response.take() {
            let _ = sender.send(Err(InitiatorClientError::UnknownAfterDisconnect));
        }
    }
}

fn start_initiator_request(
    stream: &mut TcpStream,
    transport: &mut TransportState,
    send_sequence: &mut u64,
    pending: PendingInitiatorRequest,
) -> Result<Option<InitiatorOutstanding>, RuntimeError> {
    let PendingInitiatorRequest {
        request_id,
        request,
        response,
        ..
    } = pending;
    let request_id = match request_id {
        Some(0) => {
            let _ = response.send(Err(InitiatorClientError::InvalidRequest));
            return Ok(None);
        }
        Some(request_id) => request_id,
        None => random_nonzero_u64()?,
    };
    let built = match request {
        InitiatorRequest::Command(request) => {
            build_command_frame(&request, request_id, *send_sequence).map(|frame| {
                (
                    vec![frame],
                    ExpectedInitiatorResponse::Command(request.command_seq),
                )
            })
        }
        InitiatorRequest::Resource(request) => {
            let expected = match &request {
                ResourceRequest::Reserve { .. } => ResourceResponseKind::ReserveAck,
                ResourceRequest::Commit { .. } => ResourceResponseKind::Commit,
                ResourceRequest::Release { .. } => ResourceResponseKind::Release,
            };
            build_resource_request_frame(&request, request_id, *send_sequence)
                .map(|frame| (vec![frame], ExpectedInitiatorResponse::Resource(expected)))
        }
        InitiatorRequest::RemoteBuffer(request) => {
            let expected = match &request {
                RemoteBufferRequest::Alloc { .. } => RemoteBufferResponseKind::AllocAck,
                RemoteBufferRequest::Put { .. } => RemoteBufferResponseKind::Put,
                RemoteBufferRequest::Get { .. } => RemoteBufferResponseKind::Data,
                RemoteBufferRequest::Free { .. } => RemoteBufferResponseKind::Free,
                RemoteBufferRequest::Stat { .. } => RemoteBufferResponseKind::Stat,
                RemoteBufferRequest::Touch { .. } => RemoteBufferResponseKind::Touch,
            };
            build_remote_buffer_request_frames(&request, request_id, *send_sequence)
                .map(|frames| (frames, ExpectedInitiatorResponse::RemoteBuffer(expected)))
        }
        InitiatorRequest::Compute(request) => {
            let expected = match &request {
                ComputeRequest::Submit(_) => ExpectedInitiatorResponse::ComputeSubmit,
                ComputeRequest::Status(request) => {
                    ExpectedInitiatorResponse::ComputeStatus(request.job_id)
                }
                ComputeRequest::Cancel(request) => {
                    ExpectedInitiatorResponse::ComputeCancel(request.job_id)
                }
            };
            build_compute_request_frame(&request, request_id, *send_sequence)
                .map(|frame| (vec![frame], expected))
        }
    };
    let (frames, expected) = match built {
        Ok(built) => built,
        Err(_) => {
            let _ = response.send(Err(InitiatorClientError::InvalidRequest));
            return Ok(None);
        }
    };
    let frame_count = u64::try_from(frames.len()).map_err(|_| RuntimeError::Pbmux)?;
    for frame in frames {
        if let Err(error) = send_frame(stream, transport, &frame) {
            let _ = response.send(Err(InitiatorClientError::UnknownAfterDisconnect));
            return Err(error);
        }
    }
    *send_sequence = send_sequence
        .checked_add(frame_count)
        .ok_or(RuntimeError::Pbmux)?;
    Ok(Some(InitiatorOutstanding {
        request_id,
        expected,
        response: Some(response),
    }))
}

fn take_initiator_outstanding(
    outstanding: &mut Option<InitiatorOutstanding>,
    request_id: u64,
) -> Result<InitiatorOutstanding, RuntimeError> {
    if outstanding
        .as_ref()
        .is_none_or(|request| request.request_id != request_id)
    {
        return Err(RuntimeError::Pbmux);
    }
    outstanding.take().ok_or(RuntimeError::Pbmux)
}

fn remote_response_kind(message_type: u16) -> Result<RemoteBufferResponseKind, RuntimeError> {
    match message_type {
        2 => Ok(RemoteBufferResponseKind::AllocAck),
        3 => Ok(RemoteBufferResponseKind::Put),
        5 => Ok(RemoteBufferResponseKind::Data),
        6 => Ok(RemoteBufferResponseKind::Free),
        7 => Ok(RemoteBufferResponseKind::Stat),
        8 => Ok(RemoteBufferResponseKind::Touch),
        _ => Err(RuntimeError::Pbmux),
    }
}

fn handle_remote_buffer_with_transport_liveness(
    stream: &TcpStream,
    command_handler: &dyn AuthenticatedCommandHandler,
    verified_peer: &VerifiedPeerSession<'_>,
    request_id: u64,
    request: RemoteBufferRequest,
    last_authenticated_traffic: Instant,
) -> Result<(RemoteBufferResponseKind, BufferResult), RuntimeError> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let _ = sender.send(command_handler.handle_authenticated_remote_buffer(
                verified_peer,
                request_id,
                request,
            ));
        });
        loop {
            if transport_loss_observed(stream)
                || last_authenticated_traffic.elapsed() >= DEVICE_OFFLINE_TIMEOUT
            {
                verified_peer.revoke();
                let _ = stream.shutdown(Shutdown::Both);
                return Err(RuntimeError::SessionLost);
            }
            match receiver.try_recv() {
                Ok(result) => return result.map_err(handler_runtime_error),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    std::thread::sleep(SESSION_POLL);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(RuntimeError::CommandHandlerFailed);
                }
            }
        }
    })
}

fn handle_compute_with_transport_liveness(
    stream: &TcpStream,
    command_handler: &dyn AuthenticatedCommandHandler,
    verified_peer: &VerifiedPeerSession<'_>,
    request_id: u64,
    request: ComputeRequest,
    last_authenticated_traffic: Instant,
) -> Result<ComputeResponse, RuntimeError> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let _ = sender.send(command_handler.handle_authenticated_compute(
                verified_peer,
                request_id,
                request,
            ));
        });
        loop {
            if transport_loss_observed(stream)
                || last_authenticated_traffic.elapsed() >= DEVICE_OFFLINE_TIMEOUT
            {
                verified_peer.revoke();
                let _ = stream.shutdown(Shutdown::Both);
                return Err(RuntimeError::SessionLost);
            }
            match receiver.try_recv() {
                Ok(result) => return result.map_err(handler_runtime_error),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    std::thread::sleep(SESSION_POLL);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(RuntimeError::CommandHandlerFailed);
                }
            }
        }
    })
}

fn transport_loss_observed(stream: &TcpStream) -> bool {
    let mut byte = [0_u8; 1];
    match stream.peek(&mut byte) {
        Ok(0) => true,
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => false,
        Err(_) => true,
    }
}

fn handler_runtime_error(error: AuthenticatedCommandHandlerError) -> RuntimeError {
    match error {
        AuthenticatedCommandHandlerError::Unavailable => RuntimeError::CommandHandlerUnavailable,
        AuthenticatedCommandHandlerError::Failed => RuntimeError::CommandHandlerFailed,
    }
}

fn parse_authenticated_command(frame: &Frame) -> Result<CommandPayload, RuntimeError> {
    match parse_command_frame(frame) {
        Ok(command) => Ok(command),
        Err(error) if error.kind == PbmuxErrorKind::UnsupportedMessage => {
            let bytes = frame
                .payload
                .as_slice()
                .try_into()
                .map_err(|_| RuntimeError::Pbmux)?;
            Ok(decode_unsupported_command_envelope(bytes))
        }
        Err(_) => Err(RuntimeError::Pbmux),
    }
}

fn decode_unsupported_command_envelope(bytes: &[u8; 46]) -> CommandPayload {
    CommandPayload {
        command_type: bytes[0],
        lease_present: bytes[1],
        lease_id: bytes[2..18].try_into().expect("fixed command lease id"),
        command_seq: u64::from_be_bytes(bytes[18..26].try_into().expect("fixed command sequence")),
        trace_id: bytes[26..42].try_into().expect("fixed command trace id"),
        provider_present: bytes[42],
        provider_id: bytes[43],
        payload_len: u16::from_be_bytes(
            bytes[44..46]
                .try_into()
                .expect("fixed command payload length"),
        ),
    }
}

fn configure_session_stream(stream: &TcpStream) -> Result<(), RuntimeError> {
    stream
        .set_read_timeout(None)
        .and_then(|()| stream.set_nonblocking(true))
        .map_err(|_| RuntimeError::SessionLost)
}

fn receive_frame_if_available(
    stream: &mut TcpStream,
    transport: &mut TransportState,
) -> Result<Option<Frame>, RuntimeError> {
    let mut prefix = [0_u8; 2];
    let count = match stream.peek(&mut prefix) {
        Ok(0) => return Err(RuntimeError::SessionLost),
        Ok(count) => count,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
        Err(_) => return Err(RuntimeError::SessionLost),
    };
    if count < 2 {
        return Ok(None);
    }
    stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT)))
        .map_err(|_| RuntimeError::SessionLost)?;
    let plaintext = read_encrypted(stream, transport);
    stream
        .set_read_timeout(None)
        .and_then(|()| stream.set_nonblocking(true))
        .map_err(|_| RuntimeError::SessionLost)?;
    let plaintext = plaintext?;
    decode(&plaintext)
        .map(Some)
        .map_err(|_| RuntimeError::Pbmux)
}

fn send_frame(
    stream: &mut TcpStream,
    transport: &mut TransportState,
    frame: &Frame,
) -> Result<(), RuntimeError> {
    let plaintext = encode(frame).map_err(|_| RuntimeError::Pbmux)?;
    write_encrypted(stream, transport, &plaintext)?;
    Ok(())
}

fn control_frame(message_type: ControlType, request_id: u64, sequence: u64) -> Frame {
    Frame {
        header: Header {
            channel: Channel::Control,
            flags: FLAG_START | FLAG_END,
            message_type: message_type as u16,
            request_id,
            sequence,
            fragment_index: 0,
            payload_len: 0,
            logical_message_len: 0,
        },
        payload: Vec::new(),
    }
}

pub(crate) fn random_nonzero_u64() -> Result<u64, RuntimeError> {
    for _ in 0..4 {
        let mut bytes = [0_u8; 8];
        File::open("/dev/urandom")
            .and_then(|mut random| random.read_exact(&mut bytes))
            .map_err(|_| RuntimeError::PairPersistFailed)?;
        let value = u64::from_be_bytes(bytes);
        if value != 0 {
            return Ok(value);
        }
    }
    Err(RuntimeError::PairPersistFailed)
}

fn random_nonzero_128() -> Result<[u8; 16], RuntimeError> {
    for _ in 0..4 {
        let mut bytes = [0_u8; 16];
        File::open("/dev/urandom")
            .and_then(|mut random| random.read_exact(&mut bytes))
            .map_err(|_| RuntimeError::PairPersistFailed)?;
        if bytes != [0; 16] {
            return Ok(bytes);
        }
    }
    Err(RuntimeError::PairPersistFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::fs::{Mode, OFlags, open};
    use std::fs;
    use std::io::Write;
    use std::net::{Shutdown, TcpListener};
    use std::os::unix::fs::DirBuilderExt;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "phoneboost-c05-{label}-{}-{}",
                std::process::id(),
                TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700).create(&path).expect("test state root");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn open_store(path: &Path) -> StateStore {
        let fd = open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .expect("open test state");
        StateStore::from_directory_fd(fd).expect("test store")
    }

    fn wait_until(mut condition: impl FnMut() -> bool) {
        for _ in 0..250 {
            if condition() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("bounded secure runtime condition timed out");
    }

    fn verified_peer_id(runtime: &SecureRuntime) -> Option<PeerId> {
        runtime
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .verified_peer_id
    }

    fn verified_session_mints(runtime: &SecureRuntime) -> u64 {
        runtime
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .verified_session_mints
    }

    #[derive(Clone, Copy)]
    struct ObservedCommand {
        peer_id: PeerId,
        request_id: u64,
        command: CommandPayload,
    }

    struct RecordingCommandHandler {
        calls: AtomicUsize,
        observed: Mutex<Option<ObservedCommand>>,
        response: Result<AckPayload, AuthenticatedCommandHandlerError>,
    }

    impl RecordingCommandHandler {
        fn responding(response: AckPayload) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                observed: Mutex::new(None),
                response: Ok(response),
            }
        }

        fn failing(error: AuthenticatedCommandHandlerError) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                observed: Mutex::new(None),
                response: Err(error),
            }
        }
    }

    impl AuthenticatedCommandHandler for RecordingCommandHandler {
        fn handle_authenticated_command(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            command: CommandPayload,
        ) -> Result<AckPayload, AuthenticatedCommandHandlerError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            *self
                .observed
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ObservedCommand {
                peer_id: *verified_session.peer_id(),
                request_id,
                command,
            });
            self.response
        }
    }

    fn acquire_command() -> CommandPayload {
        CommandPayload {
            command_type: 1,
            lease_present: 0,
            lease_id: [0; 16],
            command_seq: 0,
            trace_id: [3; 16],
            provider_present: 0,
            provider_id: 0,
            payload_len: 0,
        }
    }

    fn receive_test_frame(
        stream: &mut TcpStream,
        transport: &mut TransportState,
    ) -> Result<Frame, RuntimeError> {
        for _ in 0..250 {
            if let Some(frame) = receive_frame_if_available(stream, transport)? {
                return Ok(frame);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err(RuntimeError::SessionLost)
    }

    fn start_manual_committed_ik(
        label: &str,
    ) -> (
        TcpStream,
        TransportState,
        Arc<SecureRuntime>,
        std::thread::JoinHandle<RuntimeError>,
    ) {
        let host_dir = TestDirectory::new(&format!("{label}-host"));
        let android_dir = TestDirectory::new(&format!("{label}-android"));
        let host_store = open_store(&host_dir.0);
        let android_store = open_store(&android_dir.0);
        let host_identity = host_store.load_or_create_identity().unwrap();
        let android_identity = android_store.load_or_create_identity().unwrap();
        let android_peer = PeerRecord::new(*android_identity.public(), "Android", 1);
        android_store
            .commit_peer(&PeerRecord::new(*host_identity.public(), "host", 1))
            .unwrap();
        let android = Arc::new(
            SecureRuntime::initialize(EndpointRole::AndroidResponder, android_store).unwrap(),
        );
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let responder_runtime = Arc::clone(&android);
        let responder = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            run_responder_session(&mut stream, &responder_runtime, &[]).unwrap_err()
        });
        let mut stream = TcpStream::connect(endpoint).unwrap();
        configure_handshake_stream(&stream).unwrap();
        let (mut transport, _) =
            initiator_ik(&mut stream, host_identity.private(), &android_peer).unwrap();
        configure_session_stream(&stream).unwrap();
        send_frame(
            &mut stream,
            &mut transport,
            &control_frame(ControlType::Ping, 9, 0),
        )
        .unwrap();
        let pong = receive_test_frame(&mut stream, &mut transport).unwrap();
        assert_eq!(pong.header.message_type, ControlType::Pong as u16);
        assert_eq!(pong.header.request_id, 9);
        assert!(android.snapshot().authenticated);
        assert!(android.snapshot().heartbeat_count > 0);
        (stream, transport, android, responder)
    }

    fn run_committed_ik_command(
        handler: Option<Arc<dyn AuthenticatedCommandHandler>>,
        command: CommandPayload,
    ) -> (Result<Frame, RuntimeError>, RuntimeError, PeerId) {
        let host_dir = TestDirectory::new("command-host");
        let android_dir = TestDirectory::new("command-android");
        let host_store = open_store(&host_dir.0);
        let android_store = open_store(&android_dir.0);
        let host_identity = host_store.load_or_create_identity().unwrap();
        let android_identity = android_store.load_or_create_identity().unwrap();
        let android_peer = PeerRecord::new(*android_identity.public(), "Android", 1);
        android_store
            .commit_peer(&PeerRecord::new(*host_identity.public(), "host", 1))
            .unwrap();
        let expected_peer_id = PeerId::from_static_public_key(host_identity.public());
        let android = Arc::new(
            SecureRuntime::initialize(EndpointRole::AndroidResponder, android_store).unwrap(),
        );

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let android_runtime = Arc::clone(&android);
        let responder = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            match handler {
                Some(handler) => run_responder_session_with_handler(
                    &mut stream,
                    &android_runtime,
                    &[],
                    handler.as_ref(),
                ),
                None => run_responder_session(&mut stream, &android_runtime, &[]),
            }
            .unwrap_err()
        });

        let mut stream = TcpStream::connect(endpoint).unwrap();
        configure_handshake_stream(&stream).unwrap();
        let (mut transport, _) =
            initiator_ik(&mut stream, host_identity.private(), &android_peer).unwrap();
        configure_session_stream(&stream).unwrap();
        send_frame(
            &mut stream,
            &mut transport,
            &control_frame(ControlType::Ping, 9, 0),
        )
        .unwrap();
        let pong = receive_test_frame(&mut stream, &mut transport).unwrap();
        assert_eq!(pong.header.message_type, ControlType::Pong as u16);
        assert_eq!(pong.header.request_id, 9);

        let command = pb_pbmux::build_command_frame(&command, 44, 1).unwrap();
        send_frame(&mut stream, &mut transport, &command).unwrap();
        let response = receive_test_frame(&mut stream, &mut transport);
        let _ = stream.shutdown(Shutdown::Both);
        let responder_error = responder.join().unwrap();
        (response, responder_error, expected_peer_id)
    }

    fn run_ik_failure(
        host: Arc<SecureRuntime>,
        android: Arc<SecureRuntime>,
        handler: Option<Arc<dyn AuthenticatedCommandHandler>>,
    ) -> (RuntimeError, RuntimeError) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let android_runtime = Arc::clone(&android);
        let responder = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            match handler {
                Some(handler) => run_responder_session_with_handler(
                    &mut stream,
                    &android_runtime,
                    &[],
                    handler.as_ref(),
                ),
                None => run_responder_session(&mut stream, &android_runtime, &[]),
            }
            .unwrap_err()
        });
        let host_runtime = Arc::clone(&host);
        let initiator = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(endpoint).unwrap();
            run_initiator_session(&mut stream, &host_runtime).unwrap_err()
        });
        (initiator.join().unwrap(), responder.join().unwrap())
    }

    #[test]
    fn pairing_timeout_is_exactly_120_seconds() {
        assert_eq!(PAIRING_TIMEOUT_MS, 120_000);
    }

    #[test]
    fn authenticated_liveness_profile_is_two_second_heartbeat_and_ten_second_offline() {
        assert_eq!(HEARTBEAT_INTERVAL, Duration::from_secs(2));
        assert_eq!(DEVICE_OFFLINE_TIMEOUT, Duration::from_secs(10));
    }

    #[test]
    fn reconnect_mints_a_fresh_opaque_session_id_for_the_same_peer() {
        let directory = TestDirectory::new("session-generation");
        let store = open_store(&directory.0);
        let runtime = SecureRuntime::initialize(EndpointRole::AndroidResponder, store).unwrap();
        let remote = [0x55; 32];
        let first_scope = SessionScope::new();
        let first = runtime.mark_authenticated(&remote, &first_scope).unwrap();
        assert!(first.is_live());
        let first_id = first.session_id();
        first.revoke();
        assert!(!first.is_live());
        let second_scope = SessionScope::new();
        let second = runtime
            .mark_authenticated(&remote, &second_scope)
            .unwrap()
            .session_id();
        assert!(first_id != second);
    }

    #[test]
    fn committed_transport_partial_records_crypto_and_pbmux_fail_closed() {
        let (mut prefix_stream, _prefix_transport, prefix_runtime, prefix_responder) =
            start_manual_committed_ik("partial-prefix");
        prefix_stream.set_nonblocking(false).unwrap();
        prefix_stream.write_all(&[0]).unwrap();
        assert_eq!(prefix_responder.join().unwrap(), RuntimeError::SessionLost);
        assert!(!prefix_runtime.snapshot().authenticated);
        assert_eq!(prefix_runtime.snapshot().state, RuntimeState::Paired);

        let (mut payload_stream, _payload_transport, payload_runtime, payload_responder) =
            start_manual_committed_ik("partial-payload");
        payload_stream.set_nonblocking(false).unwrap();
        payload_stream.write_all(&100_u16.to_be_bytes()).unwrap();
        payload_stream.write_all(&[0]).unwrap();
        let payload_error = payload_responder.join().unwrap();
        assert_eq!(payload_error, RuntimeError::Wire(SecureWireError::Io));
        assert_eq!(payload_error.reason_code(), "DEVICE_LOST");
        assert!(!payload_runtime.snapshot().authenticated);

        let (mut crypto_stream, _crypto_transport, crypto_runtime, crypto_responder) =
            start_manual_committed_ik("crypto-failure");
        crypto_stream.set_nonblocking(false).unwrap();
        write_record(&mut crypto_stream, &[0x5a; 32]).unwrap();
        assert_eq!(
            crypto_responder.join().unwrap(),
            RuntimeError::Wire(SecureWireError::Crypto)
        );
        assert!(!crypto_runtime.snapshot().authenticated);

        let (mut pbmux_stream, mut pbmux_transport, pbmux_runtime, pbmux_responder) =
            start_manual_committed_ik("pbmux-failure");
        pbmux_stream.set_nonblocking(false).unwrap();
        write_encrypted(&mut pbmux_stream, &mut pbmux_transport, &[0; 40]).unwrap();
        assert_eq!(pbmux_responder.join().unwrap(), RuntimeError::Pbmux);
        assert!(!pbmux_runtime.snapshot().authenticated);
    }

    #[test]
    fn control_pair_confirm_shape_is_canonical() {
        let frame = pair_confirm_frame(1, 0).unwrap();
        assert_eq!(frame.header.channel, Channel::Control);
        assert_eq!(frame.header.message_type, 8);
        assert_eq!(frame.header.flags, 0x0003);
        assert_eq!(frame.header.fragment_index, 0);
        assert_eq!(frame.header.payload_len, 0);
        assert_eq!(frame.header.logical_message_len, 0);
        assert!(frame.payload.is_empty());
    }

    #[test]
    fn precommit_gate_blocks_every_non_control_domain() {
        for channel in [
            Channel::Resource,
            Channel::RemoteBuffer,
            Channel::Compute,
            Channel::AiRpc,
            Channel::Metrics,
        ] {
            let frame = Frame {
                header: Header {
                    channel,
                    flags: FLAG_START | FLAG_END,
                    message_type: 1,
                    request_id: 1,
                    sequence: 0,
                    fragment_index: 0,
                    payload_len: 0,
                    logical_message_len: 0,
                },
                payload: Vec::new(),
            };
            assert!(authorize_dispatch(&frame, DispatchMode::PairingControlOnly).is_err());
        }
    }

    #[test]
    fn unsupported_command_envelope_remains_typed_for_failed_ack_dispatch() {
        let mut frame = pb_pbmux::build_command_frame(&acquire_command(), 1, 0).unwrap();
        frame.payload[0] = 99;
        let parsed = parse_authenticated_command(&frame).unwrap();
        assert_eq!(parsed.command_type, 99);
        assert_eq!(parsed.command_seq, 0);
        assert_eq!(parsed.trace_id, [3; 16]);
    }

    #[test]
    fn authenticated_command_handler_receives_verified_peer_and_emits_exact_ack() {
        let expected_ack = AckPayload {
            ack_state: 3,
            reason_code: 5,
            command_seq: 0,
            expected_present: 0,
            expected: 0,
            result_ref_present: 0,
            lease_id: [0; 16],
            worker_incarnation: [0; 16],
            ttl_remaining_ms: 0,
            next_command_seq: 0,
            digest_present: 0,
            digest: [0; 32],
        };
        let handler = Arc::new(RecordingCommandHandler::responding(expected_ack));
        let trait_handler: Arc<dyn AuthenticatedCommandHandler> = handler.clone();
        let (response, responder_error, expected_peer_id) =
            run_committed_ik_command(Some(trait_handler), acquire_command());
        let response = response.unwrap();
        assert_eq!(response.header.message_type, ControlType::CommandAck as u16);
        assert_eq!(response.header.request_id, 44);
        assert_eq!(response.payload.len(), 98);
        assert_eq!(parse_command_ack_frame(&response).unwrap(), expected_ack);
        assert_eq!(handler.calls.load(Ordering::Relaxed), 1);
        let observed = handler.observed.lock().unwrap().unwrap();
        assert_eq!(observed.peer_id, expected_peer_id);
        assert_eq!(observed.request_id, 44);
        assert_eq!(observed.command, acquire_command());
        assert_eq!(responder_error, RuntimeError::SessionLost);
    }

    #[test]
    fn missing_or_failed_authenticated_handler_never_emits_success() {
        let (missing_response, missing_error, _) =
            run_committed_ik_command(None, acquire_command());
        assert!(missing_response.is_err());
        assert_eq!(missing_error, RuntimeError::CommandHandlerUnavailable);

        let handler = Arc::new(RecordingCommandHandler::failing(
            AuthenticatedCommandHandlerError::Failed,
        ));
        let trait_handler: Arc<dyn AuthenticatedCommandHandler> = handler.clone();
        let (failed_response, failed_error, _) =
            run_committed_ik_command(Some(trait_handler), acquire_command());
        assert!(failed_response.is_err());
        assert_eq!(failed_error, RuntimeError::CommandHandlerFailed);
        assert_eq!(handler.calls.load(Ordering::Relaxed), 1);

        let accepted_handler = Arc::new(RecordingCommandHandler::responding(AckPayload {
            ack_state: 1,
            reason_code: 0,
            command_seq: 0,
            expected_present: 0,
            expected: 0,
            result_ref_present: 0,
            lease_id: [0; 16],
            worker_incarnation: [0; 16],
            ttl_remaining_ms: 0,
            next_command_seq: 0,
            digest_present: 0,
            digest: [0; 32],
        }));
        let trait_handler: Arc<dyn AuthenticatedCommandHandler> = accepted_handler;
        let (accepted_response, accepted_error, _) =
            run_committed_ik_command(Some(trait_handler), acquire_command());
        assert!(accepted_response.is_err());
        assert_eq!(accepted_error, RuntimeError::CommandHandlerFailed);
    }

    #[test]
    fn committed_xx_and_ik_mint_verified_peer_identity_only_after_liveness() {
        let host_dir = TestDirectory::new("host");
        let android_dir = TestDirectory::new("android");
        let host = Arc::new(
            SecureRuntime::initialize(EndpointRole::LinuxInitiator, open_store(&host_dir.0))
                .unwrap(),
        );
        let android = Arc::new(
            SecureRuntime::initialize(EndpointRole::AndroidResponder, open_store(&android_dir.0))
                .unwrap(),
        );
        let expected_host_peer = PeerId::from_static_public_key(android.identity.public());
        let expected_android_peer = PeerId::from_static_public_key(host.identity.public());

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let android_thread_runtime = Arc::clone(&android);
        let responder = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            run_responder_session(&mut stream, &android_thread_runtime, &[])
        });
        let mut host_stream = TcpStream::connect(endpoint).unwrap();
        let shutdown = host_stream.try_clone().unwrap();
        let host_thread_runtime = Arc::clone(&host);
        let initiator = std::thread::spawn(move || {
            run_initiator_session(&mut host_stream, &host_thread_runtime)
        });

        let host_begin = host.begin_pairing(Duration::from_secs(3)).unwrap();
        wait_until(|| android.snapshot().state == RuntimeState::SasPending);
        let host_sas = host_begin.sas.unwrap();
        let android_sas = android.snapshot().sas.unwrap();
        if host_sas != android_sas {
            panic!("SAS mismatch");
        }
        assert_eq!(verified_peer_id(&host), None);
        assert_eq!(verified_peer_id(&android), None);
        assert_eq!(verified_session_mints(&host), 0);
        assert_eq!(verified_session_mints(&android), 0);
        assert_eq!(host.local_confirm(), PairingActionResult::Accepted);
        assert_eq!(host.local_confirm(), PairingActionResult::Duplicate);
        assert_eq!(android.local_confirm(), PairingActionResult::Accepted);
        wait_until(|| host.snapshot().authenticated && android.snapshot().authenticated);
        assert_eq!(host.snapshot().committed_peer_count, 1);
        assert_eq!(android.snapshot().committed_peer_count, 1);
        assert!(host.snapshot().heartbeat_count > 0);
        assert_eq!(verified_peer_id(&host), Some(expected_host_peer));
        assert_eq!(verified_peer_id(&android), Some(expected_android_peer));
        assert_eq!(verified_session_mints(&host), 1);
        assert_eq!(verified_session_mints(&android), 1);
        shutdown.shutdown(Shutdown::Both).unwrap();
        assert!(matches!(
            initiator.join().unwrap(),
            Err(RuntimeError::SessionLost)
        ));
        assert!(matches!(
            responder.join().unwrap(),
            Err(RuntimeError::SessionLost)
        ));
        drop(host);
        drop(android);

        let host = Arc::new(
            SecureRuntime::initialize(EndpointRole::LinuxInitiator, open_store(&host_dir.0))
                .unwrap(),
        );
        let android = Arc::new(
            SecureRuntime::initialize(EndpointRole::AndroidResponder, open_store(&android_dir.0))
                .unwrap(),
        );
        assert_eq!(host.snapshot().state, RuntimeState::Paired);
        assert_eq!(android.snapshot().state, RuntimeState::Paired);
        assert_eq!(verified_peer_id(&host), None);
        assert_eq!(verified_peer_id(&android), None);

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let android_thread_runtime = Arc::clone(&android);
        let responder = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            run_responder_session(&mut stream, &android_thread_runtime, &[])
        });
        let mut host_stream = TcpStream::connect(endpoint).unwrap();
        let shutdown = host_stream.try_clone().unwrap();
        let host_thread_runtime = Arc::clone(&host);
        let initiator = std::thread::spawn(move || {
            run_initiator_session(&mut host_stream, &host_thread_runtime)
        });
        wait_until(|| host.snapshot().authenticated && android.snapshot().authenticated);
        assert!(host.snapshot().sas.is_none());
        assert!(android.snapshot().sas.is_none());
        assert_eq!(verified_peer_id(&host), Some(expected_host_peer));
        assert_eq!(verified_peer_id(&android), Some(expected_android_peer));
        assert_eq!(verified_session_mints(&host), 1);
        assert_eq!(verified_session_mints(&android), 1);
        shutdown.shutdown(Shutdown::Both).unwrap();
        assert!(matches!(
            initiator.join().unwrap(),
            Err(RuntimeError::SessionLost)
        ));
        assert!(matches!(
            responder.join().unwrap(),
            Err(RuntimeError::SessionLost)
        ));
    }

    #[test]
    fn cancel_preserves_guard_and_three_mismatches_persist_cooldown() {
        let directory = TestDirectory::new("guard-runtime");
        let runtime =
            SecureRuntime::initialize(EndpointRole::LinuxInitiator, open_store(&directory.0))
                .unwrap();
        runtime.set_sas("000000".to_owned());
        assert_eq!(runtime.cancel(), PairingActionResult::Accepted);
        let (_, cancelled, _) = runtime.take_actions();
        assert!(cancelled);
        assert_eq!(runtime.snapshot().mismatch_count, 0);
        for expected in 1..=3 {
            runtime.set_sas("000000".to_owned());
            assert_eq!(runtime.mismatch(), PairingActionResult::Accepted);
            let (_, _, mismatch) = runtime.take_actions();
            assert!(mismatch);
            runtime.record_mismatch().unwrap();
            assert_eq!(runtime.snapshot().mismatch_count, expected);
        }
        assert_eq!(runtime.snapshot().state, RuntimeState::Cooldown);
        drop(runtime);
        let reloaded =
            SecureRuntime::initialize(EndpointRole::LinuxInitiator, open_store(&directory.0))
                .unwrap();
        assert_eq!(reloaded.snapshot().state, RuntimeState::Cooldown);
    }

    #[test]
    fn wrong_pinned_responder_key_never_falls_back_to_xx() {
        let host_dir = TestDirectory::new("wrong-key-host");
        let android_dir = TestDirectory::new("wrong-key-android");
        let host_store = open_store(&host_dir.0);
        let android_store = open_store(&android_dir.0);
        let host_identity = host_store.load_or_create_identity().unwrap();
        android_store.load_or_create_identity().unwrap();
        host_store
            .commit_peer(&PeerRecord::new([9; 32], "wrong pin", 1))
            .unwrap();
        android_store
            .commit_peer(&PeerRecord::new(*host_identity.public(), "known host", 1))
            .unwrap();
        let host =
            Arc::new(SecureRuntime::initialize(EndpointRole::LinuxInitiator, host_store).unwrap());
        let android = Arc::new(
            SecureRuntime::initialize(EndpointRole::AndroidResponder, android_store).unwrap(),
        );
        let observed_host = Arc::clone(&host);
        let observed_android = Arc::clone(&android);
        let handler = Arc::new(RecordingCommandHandler::responding(AckPayload {
            ack_state: 3,
            reason_code: 5,
            command_seq: 0,
            expected_present: 0,
            expected: 0,
            result_ref_present: 0,
            lease_id: [0; 16],
            worker_incarnation: [0; 16],
            ttl_remaining_ms: 0,
            next_command_seq: 0,
            digest_present: 0,
            digest: [0; 32],
        }));
        let trait_handler: Arc<dyn AuthenticatedCommandHandler> = handler.clone();
        let (host_error, _) = run_ik_failure(host, android, Some(trait_handler));
        assert_eq!(host_error, RuntimeError::PeerKeyMismatch);
        assert_eq!(handler.calls.load(Ordering::Relaxed), 0);
        assert_eq!(verified_session_mints(&observed_host), 0);
        assert_eq!(verified_session_mints(&observed_android), 0);
        assert_eq!(verified_peer_id(&observed_host), None);
        assert_eq!(verified_peer_id(&observed_android), None);
    }

    #[test]
    fn unknown_ik_initiator_is_rejected_before_transport_mode() {
        let host_dir = TestDirectory::new("unknown-host");
        let android_dir = TestDirectory::new("unknown-android");
        let host_store = open_store(&host_dir.0);
        let android_store = open_store(&android_dir.0);
        host_store.load_or_create_identity().unwrap();
        let android_identity = android_store.load_or_create_identity().unwrap();
        host_store
            .commit_peer(&PeerRecord::new(*android_identity.public(), "Android", 1))
            .unwrap();
        let host =
            Arc::new(SecureRuntime::initialize(EndpointRole::LinuxInitiator, host_store).unwrap());
        let android = Arc::new(
            SecureRuntime::initialize(EndpointRole::AndroidResponder, android_store).unwrap(),
        );
        let observed_host = Arc::clone(&host);
        let observed_android = Arc::clone(&android);
        let handler = Arc::new(RecordingCommandHandler::responding(AckPayload {
            ack_state: 3,
            reason_code: 5,
            command_seq: 0,
            expected_present: 0,
            expected: 0,
            result_ref_present: 0,
            lease_id: [0; 16],
            worker_incarnation: [0; 16],
            ttl_remaining_ms: 0,
            next_command_seq: 0,
            digest_present: 0,
            digest: [0; 32],
        }));
        let trait_handler: Arc<dyn AuthenticatedCommandHandler> = handler.clone();
        let (_, android_error) = run_ik_failure(host, android, Some(trait_handler));
        assert_eq!(android_error, RuntimeError::UnknownInitiatorIkRejected);
        assert_eq!(handler.calls.load(Ordering::Relaxed), 0);
        assert_eq!(verified_session_mints(&observed_host), 0);
        assert_eq!(verified_session_mints(&observed_android), 0);
        assert_eq!(verified_peer_id(&observed_host), None);
        assert_eq!(verified_peer_id(&observed_android), None);
    }

    #[test]
    fn initiator_correlates_compute_status_result_and_cancel_to_exact_job_id() {
        let expected_job_id = [0x31; 16];
        let wrong_job_id = [0x32; 16];
        let status = |job_id| pb_pbmux::ComputeStatus {
            state: pb_pbmux::ComputeJobState::Running,
            reason: pb_pbmux::ComputeReason::None,
            lease_id: [0x41; 16],
            worker_incarnation_id: [0x42; 16],
            job: Some(pb_pbmux::ComputeJobRef {
                job_id,
                provider_id: pb_pbmux::BLAKE3_PROVIDER_ID,
                provider_version: pb_pbmux::BLAKE3_PROVIDER_VERSION,
            }),
        };
        let result = |job_id| pb_pbmux::ComputeResult {
            state: pb_pbmux::ComputeJobState::Completed,
            reason: pb_pbmux::ComputeReason::None,
            lease_id: [0x41; 16],
            worker_incarnation_id: [0x42; 16],
            job: Some(pb_pbmux::ComputeJobRef {
                job_id,
                provider_id: pb_pbmux::BLAKE3_PROVIDER_ID,
                provider_version: pb_pbmux::BLAKE3_PROVIDER_VERSION,
            }),
            digest: Some([0x51; 32]),
        };

        let expected = ExpectedInitiatorResponse::ComputeStatus(expected_job_id);
        assert!(expected.accepts_compute(&ComputeResponse::Status(status(expected_job_id))));
        assert!(expected.accepts_compute(&ComputeResponse::Result(result(expected_job_id))));
        assert!(!expected.accepts_compute(&ComputeResponse::Status(status(wrong_job_id))));
        assert!(!expected.accepts_compute(&ComputeResponse::Result(result(wrong_job_id))));

        let expected = ExpectedInitiatorResponse::ComputeCancel(expected_job_id);
        assert!(expected.accepts_compute(&ComputeResponse::Cancel(status(expected_job_id))));
        assert!(!expected.accepts_compute(&ComputeResponse::Cancel(status(wrong_job_id))));
    }
}
