use std::fs::File;
use std::io::Read;
use std::net::TcpStream;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use pb_pbmux::{
    DispatchMode, Frame, Header, SequenceTracker, authorize_dispatch, decode, encode,
    pair_confirm_frame, parse_command_ack_frame, parse_command_frame, validate_pair_confirm,
};
use pb_secure::{
    NOISE_IK_NAME, PROLOGUE, PairingActor, PersistOutcome, derive_sas, production_xx_initiator,
    production_xx_responder,
};
use pb_types::{Channel, ControlType, FLAG_END, FLAG_START, PAIRING_TIMEOUT_MS, PairingState};
use snow::{Builder, HandshakeState, TransportState, params::NoiseParams};

use crate::storage::{Identity, PeerRecord, StateStore, StorageError, wall_clock_ms};
use crate::wire::{SecureWireError, read_encrypted, read_record, write_encrypted, write_record};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const SESSION_POLL: Duration = Duration::from_millis(20);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
const XX_FIRST_MESSAGE_BYTES: usize = 32;

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

    fn mark_authenticated(&self) -> Result<(), RuntimeError> {
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
        self.changed.notify_all();
        Ok(())
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
            run_committed_loop(stream, runtime, transport, remote, true)
        }
        None => {
            let (transport, remote) = initiator_xx(stream, runtime)?;
            run_pairing_loop(stream, runtime, transport, remote, true)
        }
    }
}

pub fn run_responder_session(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
    prefix: &[u8],
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
        run_pairing_loop(stream, runtime, transport, remote, false)
    } else {
        let (transport, remote) = responder_ik(stream, runtime, &first)?;
        run_committed_loop(stream, runtime, transport, remote, false)
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
    mut transport: TransportState,
    remote: [u8; 32],
    initiator: bool,
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
                    runtime.mark_authenticated()?;
                    runtime.heartbeat();
                    return run_authenticated_loop(
                        stream,
                        runtime,
                        transport,
                        send_sequence,
                        receive_sequence,
                        false,
                    );
                }
                Ok(ControlType::Pong) if committed && initiator => {
                    if Some(frame.header.request_id) != commit_ping_request {
                        return Err(RuntimeError::Pbmux);
                    }
                    runtime.mark_authenticated()?;
                    runtime.heartbeat();
                    return run_authenticated_loop(
                        stream,
                        runtime,
                        transport,
                        send_sequence,
                        receive_sequence,
                        true,
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
        }
        std::thread::sleep(SESSION_POLL);
    }
}

fn run_committed_loop(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
    mut transport: TransportState,
    _remote: [u8; 32],
    initiator: bool,
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
                    runtime.mark_authenticated()?;
                    runtime.heartbeat();
                    return run_authenticated_loop(
                        stream,
                        runtime,
                        transport,
                        send_sequence,
                        receive_sequence,
                        true,
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
            runtime.mark_authenticated()?;
            runtime.heartbeat();
            return run_authenticated_loop(
                stream,
                runtime,
                transport,
                send_sequence,
                receive_sequence,
                false,
            );
        }
        std::thread::sleep(SESSION_POLL);
    }
}

fn run_authenticated_loop(
    stream: &mut TcpStream,
    runtime: &SecureRuntime,
    mut transport: TransportState,
    mut send_sequence: u64,
    mut receive_sequence: SequenceTracker,
    initiator: bool,
) -> Result<SessionOutcome, RuntimeError> {
    let mut next_heartbeat = Instant::now() + HEARTBEAT_INTERVAL;
    let mut outstanding = None;
    loop {
        if initiator && Instant::now() >= next_heartbeat && outstanding.is_none() {
            let request = random_nonzero_u64()?;
            send_frame(
                stream,
                &mut transport,
                &control_frame(ControlType::Ping, request, send_sequence),
            )?;
            send_sequence = send_sequence.checked_add(1).ok_or(RuntimeError::Pbmux)?;
            outstanding = Some(request);
            next_heartbeat = Instant::now() + HEARTBEAT_INTERVAL;
        }
        if let Some(frame) = receive_frame_if_available(stream, &mut transport)? {
            receive_sequence
                .accept(frame.header.sequence)
                .map_err(|_| RuntimeError::Pbmux)?;
            authorize_dispatch(&frame, DispatchMode::Committed).map_err(|_| RuntimeError::Pbmux)?;
            match ControlType::try_from(frame.header.message_type) {
                Ok(ControlType::Ping) if !initiator => {
                    let pong =
                        control_frame(ControlType::Pong, frame.header.request_id, send_sequence);
                    send_sequence = send_sequence.checked_add(1).ok_or(RuntimeError::Pbmux)?;
                    send_frame(stream, &mut transport, &pong)?;
                    runtime.heartbeat();
                }
                Ok(ControlType::Pong) if initiator => {
                    if outstanding != Some(frame.header.request_id) {
                        return Err(RuntimeError::Pbmux);
                    }
                    outstanding = None;
                    runtime.heartbeat();
                }
                Ok(ControlType::Command) => {
                    parse_command_frame(&frame).map_err(|_| RuntimeError::Pbmux)?;
                }
                Ok(ControlType::CommandAck) => {
                    parse_command_ack_frame(&frame).map_err(|_| RuntimeError::Pbmux)?;
                }
                Ok(ControlType::SessionClose) => return Ok(SessionOutcome::Lost),
                _ => return Err(RuntimeError::Pbmux),
            }
        }
        std::thread::sleep(SESSION_POLL);
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
    let mut available = vec![0_u8; u16::MAX as usize + 2];
    let count = match stream.peek(&mut available) {
        Ok(0) => return Err(RuntimeError::SessionLost),
        Ok(count) => count,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
        Err(_) => return Err(RuntimeError::SessionLost),
    };
    if count < 2 {
        return Ok(None);
    }
    let length = usize::from(u16::from_be_bytes([available[0], available[1]]));
    if length == 0 || count < length + 2 {
        return Ok(None);
    }
    stream
        .set_nonblocking(false)
        .map_err(|_| RuntimeError::SessionLost)?;
    let plaintext = read_encrypted(stream, transport);
    stream
        .set_nonblocking(true)
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

fn random_nonzero_u64() -> Result<u64, RuntimeError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::fs::{Mode, OFlags, open};
    use std::fs;
    use std::net::{Shutdown, TcpListener};
    use std::os::unix::fs::DirBuilderExt;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

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

    fn run_ik_failure(
        host: Arc<SecureRuntime>,
        android: Arc<SecureRuntime>,
    ) -> (RuntimeError, RuntimeError) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let android_runtime = Arc::clone(&android);
        let responder = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            run_responder_session(&mut stream, &android_runtime, &[]).unwrap_err()
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
    fn real_xx_sas_mutual_commit_ping_pong_and_ik_restart() {
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
        assert_eq!(host.local_confirm(), PairingActionResult::Accepted);
        assert_eq!(host.local_confirm(), PairingActionResult::Duplicate);
        assert_eq!(android.local_confirm(), PairingActionResult::Accepted);
        wait_until(|| host.snapshot().authenticated && android.snapshot().authenticated);
        assert_eq!(host.snapshot().committed_peer_count, 1);
        assert_eq!(android.snapshot().committed_peer_count, 1);
        assert!(host.snapshot().heartbeat_count > 0);
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
        let (host_error, _) = run_ik_failure(host, android);
        assert_eq!(host_error, RuntimeError::PeerKeyMismatch);
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
        let (_, android_error) = run_ik_failure(host, android);
        assert_eq!(android_error, RuntimeError::UnknownInitiatorIkRejected);
    }
}
