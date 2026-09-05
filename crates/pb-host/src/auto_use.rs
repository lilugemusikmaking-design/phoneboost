use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use pb_pbmux::{
    AllocationFlags, BLAKE3_PROVIDER_ID, BLAKE3_PROVIDER_VERSION, BufferReason, BufferState,
    CommandPayload, ComputeJobRequest, ComputeJobState, ComputeReason, ComputeRequest,
    ComputeResponse, ComputeSubmit, MAX_COMPUTE_INPUT_BYTES, MAX_PUT_BODY,
    REMOTE_BUFFER_INPUT_KIND, RemoteBufferRequest, ResourceReason, ResourceRequest,
    ResourceResultState, WireReservationState, WireResourceClass,
};
use pb_runtime_secure::{
    InitiatorClientError, InitiatorSessionClient, SecureRuntime, initiator_session_channel,
    run_initiator_session_with_client,
};
use pb_types::PeerId;

use crate::{TransportCandidate, TransportManager, os_jitter_sample, retry_delay_ms};

const AUTHENTICATION_TIMEOUT: Duration = Duration::from_secs(6);
const DISCOVERY_POLL: Duration = Duration::from_millis(250);
const MANAGER_POLL: Duration = Duration::from_millis(25);
const READINESS_RETRY: Duration = Duration::from_secs(1);
const RENEWAL_INTERVAL: Duration = Duration::from_secs(20);
const FRESH_LIVENESS: Duration = Duration::from_secs(10);
const COMPUTE_STATUS_POLL: Duration = Duration::from_millis(25);
const SCRATCH_BYTES: u64 = 1_024;
/// P2 is passive: this only bounds how long an already-observed discovery hint
/// may be reported. It never schedules discovery.
const DISCOVERY_OBSERVATION_MAX_AGE: Duration = Duration::from_secs(30);
/// A readiness proof is request-specific, not durable ResourceGuard authority.
/// This bounds the display of an already-completed production proof only.
const ADMISSION_PROOF_MAX_AGE: Duration = Duration::from_secs(2);

pub trait DeviceDiscovery: Send + Sync + 'static {
    fn start(&self) -> Result<(), DiscoveryError> {
        Ok(())
    }

    /// Returns an untrusted transport hint. Authentication remains exclusively C05.
    fn discover(&self) -> Result<Option<TransportCandidate>, DiscoveryError>;

    fn stop(&self) {}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryError {
    BackendUnavailable,
}

#[derive(Clone, Copy, Debug)]
pub struct FixedDeviceDiscovery {
    candidate: TransportCandidate,
}

impl FixedDeviceDiscovery {
    pub const fn new(candidate: TransportCandidate) -> Self {
        Self { candidate }
    }
}

impl DeviceDiscovery for FixedDeviceDiscovery {
    fn discover(&self) -> Result<Option<TransportCandidate>, DiscoveryError> {
        Ok(Some(self.candidate))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutoUseState {
    Off,
    Discovering,
    Connecting,
    Authenticating,
    AcquiringAuthority,
    CheckingReadiness,
    Available,
    Degraded,
    Reconnecting,
    Unavailable,
}

impl AutoUseState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Discovering => "DISCOVERING",
            Self::Connecting => "CONNECTING",
            Self::Authenticating => "AUTHENTICATING",
            Self::AcquiringAuthority => "ACQUIRING_AUTHORITY",
            Self::CheckingReadiness => "CHECKING_READINESS",
            Self::Available => "AVAILABLE",
            Self::Degraded => "DEGRADED",
            Self::Reconnecting => "RECONNECTING",
            Self::Unavailable => "UNAVAILABLE",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutoUseReason {
    Off,
    NoDevice,
    NotPaired,
    AuthFailed,
    LeaseUnavailable,
    WorkerUnhealthy,
    ResourceRefused,
    TransportLost,
    Reconnecting,
    DiscoveryBackendUnavailable,
    Ready,
}

impl AutoUseReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::NoDevice => "NO_DEVICE",
            Self::NotPaired => "NOT_PAIRED",
            Self::AuthFailed => "AUTH_FAILED",
            Self::LeaseUnavailable => "LEASE_UNAVAILABLE",
            Self::WorkerUnhealthy => "WORKER_UNHEALTHY",
            Self::ResourceRefused => "RESOURCE_REFUSED",
            Self::TransportLost => "TRANSPORT_LOST",
            Self::Reconnecting => "RECONNECTING",
            Self::DiscoveryBackendUnavailable => "DISCOVERY_BACKEND_UNAVAILABLE",
            Self::Ready => "READY",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionSource {
    RemoteSuccess,
    LocalFallbackAfterRemoteUnavailable,
    LocalFallbackAfterAmbiguousRemote,
}

impl ExecutionSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RemoteSuccess => "REMOTE_SUCCESS",
            Self::LocalFallbackAfterRemoteUnavailable => "LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE",
            Self::LocalFallbackAfterAmbiguousRemote => "LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeStatus {
    state: AutoUseState,
    reason: AutoUseReason,
    peer_id: Option<PeerId>,
    worker_incarnation: Option<[u8; 16]>,
    remote_blake3_available: bool,
}

impl NodeStatus {
    pub const fn state(self) -> AutoUseState {
        self.state
    }

    pub const fn reason(self) -> AutoUseReason {
        self.reason
    }

    pub const fn peer_id(self) -> Option<PeerId> {
        self.peer_id
    }

    pub const fn worker_incarnation(self) -> Option<[u8; 16]> {
        self.worker_incarnation
    }

    pub const fn remote_blake3_available(self) -> bool {
        self.remote_blake3_available
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Blake3Execution {
    digest: [u8; 32],
    source: ExecutionSource,
    reason: AutoUseReason,
}

impl Blake3Execution {
    pub const fn digest(self) -> [u8; 32] {
        self.digest
    }

    pub const fn source(self) -> ExecutionSource {
        self.source
    }

    pub const fn reason(self) -> AutoUseReason {
        self.reason
    }
}

#[derive(Debug)]
pub enum AutoUseError {
    ThreadStart(io::Error),
}

impl std::fmt::Display for AutoUseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ThreadStart(error) => write!(formatter, "auto-use thread start failed: {error}"),
        }
    }
}

impl std::error::Error for AutoUseError {}

#[derive(Clone, Copy)]
struct Timing {
    authentication_timeout: Duration,
    discovery_poll: Duration,
    manager_poll: Duration,
    readiness_retry: Duration,
    renewal_interval: Duration,
    retry_override: Option<Duration>,
}

impl Timing {
    const PRODUCTION: Self = Self {
        authentication_timeout: AUTHENTICATION_TIMEOUT,
        discovery_poll: DISCOVERY_POLL,
        manager_poll: MANAGER_POLL,
        readiness_retry: READINESS_RETRY,
        renewal_interval: RENEWAL_INTERVAL,
        retry_override: None,
    };
}

#[derive(Clone, Copy)]
struct LeaseContext {
    lease_id: [u8; 16],
    incarnation: [u8; 16],
    next_command_seq: u64,
    /// Conservative local bound calculated before the C07 command is sent.
    not_after: Instant,
}

/// Sanitized P2 observation. These strings are deliberately the entire public
/// surface: no endpoint, peer identity, lease id, incarnation, TTL, or error
/// diagnostics leave the controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GateObservation {
    state: &'static str,
    reason: &'static str,
}

impl GateObservation {
    const fn new(state: &'static str, reason: &'static str) -> Self {
        Self { state, reason }
    }

    pub const fn state(self) -> &'static str {
        self.state
    }

    pub const fn reason(self) -> &'static str {
        self.reason
    }
}

/// Passive, fail-closed snapshot for C12 status consumers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GateObservations {
    discovery_observation: GateObservation,
    controller_lease: GateObservation,
    resource_guard_admission_proof: GateObservation,
}

/// Atomic local projection used by C12. Node status and P2 observations are
/// captured under the same controller-data lock and evaluated at one instant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControllerObservabilitySnapshot {
    node_status: NodeStatus,
    gate_observations: GateObservations,
}

impl ControllerObservabilitySnapshot {
    pub const fn node_status(self) -> NodeStatus {
        self.node_status
    }

    pub const fn gate_observations(self) -> GateObservations {
        self.gate_observations
    }
}

impl GateObservations {
    pub const fn not_observed() -> Self {
        Self {
            discovery_observation: GateObservation::new("UNKNOWN", "NOT_OBSERVED"),
            controller_lease: GateObservation::new("UNKNOWN", "NOT_OBSERVED"),
            resource_guard_admission_proof: GateObservation::new("UNKNOWN", "NOT_OBSERVED"),
        }
    }

    pub const fn discovery_observation(self) -> GateObservation {
        self.discovery_observation
    }

    pub const fn controller_lease(self) -> GateObservation {
        self.controller_lease
    }

    pub const fn resource_guard_admission_proof(self) -> GateObservation {
        self.resource_guard_admission_proof
    }
}

#[derive(Clone, Copy)]
struct TimedObservation {
    observation: GateObservation,
    observed_at: Instant,
}

#[derive(Clone, Copy)]
struct LeaseObservation {
    observation: GateObservation,
    identity: Option<SessionIdentity>,
    lease_id: Option<[u8; 16]>,
    incarnation: Option<[u8; 16]>,
    not_after: Option<Instant>,
}

impl LeaseObservation {
    const fn not_observed() -> Self {
        Self {
            observation: GateObservation::new("UNKNOWN", "NOT_OBSERVED"),
            identity: None,
            lease_id: None,
            incarnation: None,
            not_after: None,
        }
    }
}

#[derive(Clone, Copy)]
struct AdmissionProofObservation {
    observation: GateObservation,
    observed_at: Option<Instant>,
    identity: Option<SessionIdentity>,
    lease_id: Option<[u8; 16]>,
    incarnation: Option<[u8; 16]>,
}

impl AdmissionProofObservation {
    const fn not_observed() -> Self {
        Self {
            observation: GateObservation::new("UNKNOWN", "NOT_OBSERVED"),
            observed_at: None,
            identity: None,
            lease_id: None,
            incarnation: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SessionIdentity {
    epoch: u64,
    serial: u64,
}

#[derive(Clone)]
struct ActiveContext {
    client: InitiatorSessionClient,
    identity: SessionIdentity,
    peer_id: PeerId,
    lease: LeaseContext,
}

const MAX_CLEANUP_OBLIGATIONS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CleanupTarget {
    Reservation([u8; 16]),
    Buffer([u8; 16]),
    UnresolvedReserve {
        request_id: u64,
        resource_class: WireResourceClass,
        requested_bytes: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CleanupKnowledge {
    Known,
    Ambiguous,
    AwaitingResourceGuardPurgeProof,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CleanupObligation {
    peer_id: PeerId,
    lease_id: [u8; 16],
    incarnation: [u8; 16],
    origin: SessionIdentity,
    target: CleanupTarget,
    knowledge: CleanupKnowledge,
}

#[derive(Default)]
struct CleanupLedger {
    entries: Vec<CleanupObligation>,
}

struct ControllerData {
    enabled: bool,
    epoch: u64,
    next_session_serial: u64,
    status: NodeStatus,
    session_client: Option<InitiatorSessionClient>,
    session_identity: Option<SessionIdentity>,
    active: Option<ActiveContext>,
    discovery_observation: Option<TimedObservation>,
    discovery_unknown: GateObservation,
    lease_observation: LeaseObservation,
    admission_proof_observation: AdmissionProofObservation,
}

struct Shared {
    runtime: Arc<SecureRuntime>,
    discovery: Arc<dyn DeviceDiscovery>,
    timing: Timing,
    data: Mutex<ControllerData>,
    changed: Condvar,
    operation: Mutex<()>,
    cleanup: Mutex<CleanupLedger>,
    shutdown: AtomicBool,
}

pub struct AutoUseController {
    shared: Arc<Shared>,
    manager: Option<JoinHandle<()>>,
}

impl AutoUseController {
    pub fn new(
        runtime: Arc<SecureRuntime>,
        discovery: Arc<dyn DeviceDiscovery>,
    ) -> Result<Self, AutoUseError> {
        Self::with_timing(runtime, discovery, Timing::PRODUCTION)
    }

    fn with_timing(
        runtime: Arc<SecureRuntime>,
        discovery: Arc<dyn DeviceDiscovery>,
        timing: Timing,
    ) -> Result<Self, AutoUseError> {
        let shared = Arc::new(Shared {
            runtime,
            discovery,
            timing,
            data: Mutex::new(ControllerData {
                enabled: false,
                epoch: 0,
                next_session_serial: 0,
                status: node_status(AutoUseState::Off, AutoUseReason::Off, None, None),
                session_client: None,
                session_identity: None,
                active: None,
                discovery_observation: None,
                discovery_unknown: GateObservation::new("UNKNOWN", "NOT_OBSERVED"),
                lease_observation: LeaseObservation::not_observed(),
                admission_proof_observation: AdmissionProofObservation::not_observed(),
            }),
            changed: Condvar::new(),
            operation: Mutex::new(()),
            cleanup: Mutex::new(CleanupLedger::default()),
            shutdown: AtomicBool::new(false),
        });
        let manager_shared = Arc::clone(&shared);
        let manager = thread::Builder::new()
            .name("phoneboost-auto-use".to_owned())
            .spawn(move || manager_loop(manager_shared))
            .map_err(AutoUseError::ThreadStart)?;
        Ok(Self {
            shared,
            manager: Some(manager),
        })
    }

    pub fn enable(&self) {
        let mut data = lock_data(&self.shared);
        if data.enabled {
            return;
        }
        data.enabled = true;
        data.epoch = data.epoch.saturating_add(1).max(1);
        data.status = node_status(
            AutoUseState::Discovering,
            AutoUseReason::NoDevice,
            None,
            None,
        );
        clear_observations_for_epoch(&mut data);
        self.shared.changed.notify_all();
    }

    pub fn disable(&self) {
        let mut data = lock_data(&self.shared);
        if !data.enabled && data.status.state == AutoUseState::Off {
            return;
        }
        if let Some(client) = data.session_client.as_ref() {
            client.cancel_session();
        }
        data.enabled = false;
        data.epoch = data.epoch.saturating_add(1).max(1);
        data.status = node_status(AutoUseState::Off, AutoUseReason::Off, None, None);
        data.active = None;
        data.session_identity = None;
        data.discovery_observation = None;
        data.discovery_unknown = GateObservation::new("UNKNOWN", "EPOCH_INVALIDATED");
        data.lease_observation = unavailable_lease("AUTO_USE_DISABLED");
        data.admission_proof_observation = unknown_admission_proof("AUTO_USE_DISABLED");
        self.shared.changed.notify_all();
    }

    pub fn current_state(&self) -> AutoUseState {
        lock_data(&self.shared).status.state
    }

    pub fn current_node_status(&self) -> NodeStatus {
        lock_data(&self.shared).status
    }

    /// Read-only P2 projection. It takes no network action and performs no
    /// discovery, lease operation, readiness proof, compute, or cleanup.
    pub fn current_gate_observations(&self) -> GateObservations {
        gate_observations(&lock_data(&self.shared), Instant::now())
    }

    /// Read one coherent, passive observability snapshot for C12 consumers.
    pub fn current_observability_snapshot(&self) -> ControllerObservabilitySnapshot {
        let data = lock_data(&self.shared);
        let now = Instant::now();
        ControllerObservabilitySnapshot {
            node_status: data.status,
            gate_observations: gate_observations(&data, now),
        }
    }

    pub fn execute_blake3(&self, input: &[u8]) -> Blake3Execution {
        let _operation = self
            .shared
            .operation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (enabled, status, active) = {
            let data = lock_data(&self.shared);
            (data.enabled, data.status, data.active.clone())
        };
        if !enabled || status.state != AutoUseState::Available {
            return local_execution(
                input,
                status.reason,
                ExecutionSource::LocalFallbackAfterRemoteUnavailable,
            );
        }
        if input.is_empty() || input.len() as u64 > MAX_COMPUTE_INPUT_BYTES {
            return local_execution(
                input,
                AutoUseReason::ResourceRefused,
                ExecutionSource::LocalFallbackAfterRemoteUnavailable,
            );
        }
        let Some(active) = active else {
            return local_execution(
                input,
                AutoUseReason::TransportLost,
                ExecutionSource::LocalFallbackAfterRemoteUnavailable,
            );
        };
        let access = SessionAccess {
            shared: &self.shared,
            identity: active.identity,
            client: &active.client,
        };
        match execute_remote_blake3(&access, active.peer_id, active.lease, input) {
            Ok(digest) => Blake3Execution {
                digest,
                source: ExecutionSource::RemoteSuccess,
                reason: AutoUseReason::Ready,
            },
            Err(failure) => {
                if failure.transport_lost {
                    active.client.cancel_session();
                    mark_connection_lost(&self.shared, &active);
                } else {
                    mark_degraded(&self.shared, &active, failure.reason);
                }
                local_execution(
                    input,
                    failure.reason,
                    if failure.ambiguous {
                        ExecutionSource::LocalFallbackAfterAmbiguousRemote
                    } else {
                        ExecutionSource::LocalFallbackAfterRemoteUnavailable
                    },
                )
            }
        }
    }
}

impl Drop for AutoUseController {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Release);
        let client = {
            let mut data = lock_data(&self.shared);
            data.enabled = false;
            data.active = None;
            data.session_identity = None;
            data.session_client.take()
        };
        if let Some(client) = client {
            client.cancel_session();
        }
        self.shared.changed.notify_all();
        if let Some(manager) = self.manager.take() {
            let _ = manager.join();
        }
    }
}

fn unavailable_lease(reason: &'static str) -> LeaseObservation {
    LeaseObservation {
        observation: GateObservation::new("UNAVAILABLE", reason),
        identity: None,
        lease_id: None,
        incarnation: None,
        not_after: None,
    }
}

fn unknown_admission_proof(reason: &'static str) -> AdmissionProofObservation {
    AdmissionProofObservation {
        observation: GateObservation::new("UNKNOWN", reason),
        observed_at: None,
        identity: None,
        lease_id: None,
        incarnation: None,
    }
}

fn clear_observations_for_epoch(data: &mut ControllerData) {
    data.discovery_observation = None;
    data.discovery_unknown = GateObservation::new("UNKNOWN", "EPOCH_INVALIDATED");
    data.lease_observation = LeaseObservation::not_observed();
    data.admission_proof_observation = AdmissionProofObservation::not_observed();
}

fn record_discovery_observation(
    shared: &Shared,
    expected_epoch: u64,
    observation: GateObservation,
) -> bool {
    let mut data = lock_data(shared);
    if data.enabled && data.epoch == expected_epoch {
        data.discovery_observation = Some(TimedObservation {
            observation,
            observed_at: Instant::now(),
        });
        data.discovery_unknown = GateObservation::new("UNKNOWN", "NOT_OBSERVED");
        shared.changed.notify_all();
        true
    } else {
        false
    }
}

fn record_lease_active(shared: &Shared, identity: SessionIdentity, lease: LeaseContext) {
    let mut data = lock_data(shared);
    if data.enabled && data.session_identity == Some(identity) {
        data.lease_observation = LeaseObservation {
            observation: GateObservation::new("ACTIVE", "C07_ACK_FRESH"),
            identity: Some(identity),
            lease_id: Some(lease.lease_id),
            incarnation: Some(lease.incarnation),
            not_after: Some(lease.not_after),
        };
        shared.changed.notify_all();
    }
}

fn record_lease_unavailable(shared: &Shared, identity: SessionIdentity, reason: &'static str) {
    let mut data = lock_data(shared);
    if data.enabled && data.session_identity == Some(identity) {
        data.lease_observation = unavailable_lease(reason);
        shared.changed.notify_all();
    }
}

fn record_admission_proof(
    shared: &Shared,
    identity: SessionIdentity,
    lease: LeaseContext,
    passed: bool,
) {
    let mut data = lock_data(shared);
    if data.enabled && data.session_identity == Some(identity) {
        data.admission_proof_observation = AdmissionProofObservation {
            observation: GateObservation::new(
                if passed { "FRESH_PASS" } else { "FAILED" },
                if passed {
                    "C08_C09_C10_PROBE_PASSED"
                } else {
                    "C08_C09_C10_PROBE_FAILED"
                },
            ),
            observed_at: Some(Instant::now()),
            identity: Some(identity),
            lease_id: Some(lease.lease_id),
            incarnation: Some(lease.incarnation),
        };
        shared.changed.notify_all();
    }
}

fn gate_observations(data: &ControllerData, now: Instant) -> GateObservations {
    if !data.enabled {
        return GateObservations {
            discovery_observation: GateObservation::new("UNKNOWN", "EPOCH_INVALIDATED"),
            controller_lease: GateObservation::new("UNAVAILABLE", "AUTO_USE_DISABLED"),
            resource_guard_admission_proof: GateObservation::new("UNKNOWN", "AUTO_USE_DISABLED"),
        };
    }

    let discovery_observation =
        data.discovery_observation
            .map_or(data.discovery_unknown, |entry| {
                if now.saturating_duration_since(entry.observed_at) <= DISCOVERY_OBSERVATION_MAX_AGE
                {
                    entry.observation
                } else {
                    GateObservation::new("STALE", "OBSERVATION_EXPIRED")
                }
            });

    let controller_lease = current_lease_observation(data, now);
    let resource_guard_admission_proof = current_admission_proof_observation(data, now);
    GateObservations {
        discovery_observation,
        controller_lease,
        resource_guard_admission_proof,
    }
}

fn session_is_current(data: &ControllerData, identity: SessionIdentity) -> bool {
    data.session_identity == Some(identity)
        && data
            .session_client
            .as_ref()
            .is_some_and(|client| client.snapshot().authenticated)
}

fn current_lease_observation(data: &ControllerData, now: Instant) -> GateObservation {
    let entry = data.lease_observation;
    if entry.observation.state != "ACTIVE" {
        return entry.observation;
    }
    let Some(identity) = entry.identity else {
        return GateObservation::new("UNAVAILABLE", "SESSION_INVALIDATED");
    };
    if !session_is_current(data, identity) {
        return GateObservation::new("UNAVAILABLE", "SESSION_INVALIDATED");
    }
    if entry.not_after.is_none_or(|not_after| now >= not_after) {
        return GateObservation::new("EXPIRED", "ACK_TTL_ELAPSED");
    }
    entry.observation
}

fn current_admission_proof_observation(data: &ControllerData, now: Instant) -> GateObservation {
    let entry = data.admission_proof_observation;
    let Some(observed_at) = entry.observed_at else {
        return entry.observation;
    };
    let Some(identity) = entry.identity else {
        return GateObservation::new("UNKNOWN", "SESSION_INVALIDATED");
    };
    if data.session_identity != Some(identity) {
        return GateObservation::new(
            "UNKNOWN",
            if data.session_identity.is_some() {
                "IDENTITY_OR_INCARNATION_CHANGED"
            } else {
                "SESSION_INVALIDATED"
            },
        );
    }
    if !session_is_current(data, identity) {
        return GateObservation::new("UNKNOWN", "SESSION_INVALIDATED");
    }
    let lease = data.lease_observation;
    if current_lease_observation(data, now).state != "ACTIVE"
        || lease.lease_id != entry.lease_id
        || lease.incarnation != entry.incarnation
    {
        return GateObservation::new("UNKNOWN", "LEASE_INVALIDATED");
    }
    if now.saturating_duration_since(observed_at) > ADMISSION_PROOF_MAX_AGE {
        return GateObservation::new("STALE", "PROOF_EXPIRED");
    }
    entry.observation
}

fn manager_loop(shared: Arc<Shared>) {
    loop {
        let Some(epoch) = wait_for_enable(&shared) else {
            return;
        };
        run_enabled(&shared, epoch);
        shared.discovery.stop();
    }
}

fn wait_for_enable(shared: &Shared) -> Option<u64> {
    let mut data = lock_data(shared);
    while !data.enabled && !shared.shutdown.load(Ordering::Acquire) {
        data = shared
            .changed
            .wait(data)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
    (!shared.shutdown.load(Ordering::Acquire)).then_some(data.epoch)
}

fn run_enabled(shared: &Shared, epoch: u64) {
    let mut retry_attempt = 0_usize;
    let mut authenticated_once = false;
    while is_current(shared, epoch) {
        if shared.discovery.start().is_err() {
            record_discovery_observation(
                shared,
                epoch,
                GateObservation::new("BACKEND_UNAVAILABLE", "DISCOVERY_BACKEND_UNAVAILABLE"),
            );
            set_phase(
                shared,
                epoch,
                AutoUseState::Unavailable,
                AutoUseReason::DiscoveryBackendUnavailable,
                None,
                None,
            );
            if !wait_current(shared, epoch, shared.timing.discovery_poll) {
                return;
            }
            continue;
        }
        if shared.runtime.snapshot().committed_peer_count == 0 {
            set_phase(
                shared,
                epoch,
                AutoUseState::Unavailable,
                AutoUseReason::NotPaired,
                None,
                None,
            );
            if !wait_current(shared, epoch, shared.timing.discovery_poll) {
                return;
            }
            continue;
        }
        set_phase(
            shared,
            epoch,
            if authenticated_once {
                AutoUseState::Reconnecting
            } else {
                AutoUseState::Discovering
            },
            if authenticated_once {
                AutoUseReason::Reconnecting
            } else {
                AutoUseReason::NoDevice
            },
            None,
            None,
        );
        let candidate = match shared.discovery.discover() {
            Ok(Some(candidate)) => {
                record_discovery_observation(
                    shared,
                    epoch,
                    GateObservation::new("FRESH_HINT", "C04_CANDIDATE_OBSERVED"),
                );
                candidate
            }
            Ok(None) => {
                record_discovery_observation(
                    shared,
                    epoch,
                    GateObservation::new("NO_HINT", "C04_NO_CANDIDATE"),
                );
                set_phase(
                    shared,
                    epoch,
                    AutoUseState::Unavailable,
                    AutoUseReason::NoDevice,
                    None,
                    None,
                );
                if !wait_current(shared, epoch, shared.timing.discovery_poll) {
                    return;
                }
                continue;
            }
            Err(DiscoveryError::BackendUnavailable) => {
                shared.discovery.stop();
                record_discovery_observation(
                    shared,
                    epoch,
                    GateObservation::new("BACKEND_UNAVAILABLE", "DISCOVERY_BACKEND_UNAVAILABLE"),
                );
                set_phase(
                    shared,
                    epoch,
                    AutoUseState::Unavailable,
                    AutoUseReason::DiscoveryBackendUnavailable,
                    None,
                    None,
                );
                if !wait_current(shared, epoch, shared.timing.discovery_poll) {
                    return;
                }
                continue;
            }
        };
        set_phase(
            shared,
            epoch,
            AutoUseState::Connecting,
            if authenticated_once {
                AutoUseReason::Reconnecting
            } else {
                AutoUseReason::NoDevice
            },
            None,
            None,
        );
        let failure_reason = match run_connection(shared, epoch, candidate) {
            ConnectionEnd::Disabled => return,
            ConnectionEnd::AuthenticatedLost => {
                authenticated_once = true;
                AutoUseReason::Reconnecting
            }
            ConnectionEnd::Failed(reason) => reason,
        };
        if !is_current(shared, epoch) {
            return;
        }
        set_phase(
            shared,
            epoch,
            if failure_reason == AutoUseReason::AuthFailed {
                AutoUseState::Unavailable
            } else if authenticated_once {
                AutoUseState::Reconnecting
            } else {
                AutoUseState::Unavailable
            },
            if failure_reason == AutoUseReason::AuthFailed {
                failure_reason
            } else if authenticated_once {
                AutoUseReason::Reconnecting
            } else {
                failure_reason
            },
            None,
            None,
        );
        let delay = shared.timing.retry_override.unwrap_or_else(|| {
            let sample = os_jitter_sample().unwrap_or(u16::MAX / 2);
            Duration::from_millis(retry_delay_ms(retry_attempt, sample))
        });
        retry_attempt = retry_attempt.saturating_add(1);
        if !wait_current(shared, epoch, delay) {
            return;
        }
    }
}

#[derive(Clone, Copy)]
enum ConnectionEnd {
    Disabled,
    AuthenticatedLost,
    Failed(AutoUseReason),
}

fn run_connection(shared: &Shared, epoch: u64, candidate: TransportCandidate) -> ConnectionEnd {
    let mut transport = TransportManager::new(candidate);
    if transport.connect().is_err() {
        return ConnectionEnd::Failed(AutoUseReason::NoDevice);
    }
    let Ok(mut stream) = transport.take_connected_stream() else {
        return ConnectionEnd::Failed(AutoUseReason::NoDevice);
    };
    let (client, driver) = initiator_session_channel();
    let identity = {
        let mut data = lock_data(shared);
        if !data.enabled || data.epoch != epoch {
            return ConnectionEnd::Disabled;
        }
        data.next_session_serial = data.next_session_serial.saturating_add(1).max(1);
        let identity = SessionIdentity {
            epoch,
            serial: data.next_session_serial,
        };
        data.session_client = Some(client.clone());
        data.session_identity = Some(identity);
        data.status = node_status(
            AutoUseState::Authenticating,
            AutoUseReason::AuthFailed,
            None,
            None,
        );
        shared.changed.notify_all();
        identity
    };
    let runtime = Arc::clone(&shared.runtime);
    let session = match thread::Builder::new()
        .name("phoneboost-secure-session".to_owned())
        .spawn(move || run_initiator_session_with_client(&mut stream, &runtime, driver))
    {
        Ok(session) => session,
        Err(_) => {
            clear_session(shared, identity);
            return ConnectionEnd::Failed(AutoUseReason::NoDevice);
        }
    };
    let authenticated = client.wait_authenticated(shared.timing.authentication_timeout);
    let snapshot = match authenticated {
        Ok(snapshot) if snapshot.authenticated => snapshot,
        _ => {
            client.cancel_session();
            let _ = session.join();
            clear_session(shared, identity);
            set_phase(
                shared,
                epoch,
                AutoUseState::Unavailable,
                AutoUseReason::AuthFailed,
                None,
                None,
            );
            return if is_current(shared, epoch) {
                ConnectionEnd::Failed(AutoUseReason::AuthFailed)
            } else {
                ConnectionEnd::Disabled
            };
        }
    };
    let Some(peer_id) = snapshot.peer_id else {
        client.cancel_session();
        let _ = session.join();
        clear_session(shared, identity);
        return ConnectionEnd::Failed(AutoUseReason::AuthFailed);
    };
    let access = SessionAccess {
        shared,
        identity,
        client: &client,
    };
    set_phase(
        shared,
        epoch,
        AutoUseState::AcquiringAuthority,
        AutoUseReason::LeaseUnavailable,
        Some(peer_id),
        None,
    );
    let lease = loop {
        if !access.is_current() || !client.snapshot().authenticated {
            client.cancel_session();
            let _ = session.join();
            clear_session(shared, identity);
            return if is_current(shared, epoch) {
                ConnectionEnd::AuthenticatedLost
            } else {
                ConnectionEnd::Disabled
            };
        }
        match acquire_lease(&access) {
            Ok(lease) => {
                record_lease_active(shared, identity, lease);
                break lease;
            }
            Err(failure) if failure.transport_lost => {
                client.cancel_session();
                let _ = session.join();
                clear_session(shared, identity);
                return ConnectionEnd::AuthenticatedLost;
            }
            Err(_) => {
                record_lease_unavailable(shared, identity, "C07_ACQUIRE_FAILED");
                set_phase(
                    shared,
                    epoch,
                    AutoUseState::Unavailable,
                    AutoUseReason::LeaseUnavailable,
                    Some(peer_id),
                    None,
                );
                if !wait_current(shared, epoch, shared.timing.readiness_retry) {
                    client.cancel_session();
                    let _ = session.join();
                    clear_session(shared, identity);
                    return ConnectionEnd::Disabled;
                }
            }
        }
    };
    set_phase(
        shared,
        epoch,
        AutoUseState::CheckingReadiness,
        AutoUseReason::WorkerUnhealthy,
        Some(peer_id),
        Some(lease.incarnation),
    );
    let mut lease = lease;
    if reconcile_cleanup(&access, peer_id, lease).is_err() {
        client.cancel_session();
        let _ = session.join();
        clear_session(shared, identity);
        return if is_current(shared, epoch) {
            ConnectionEnd::AuthenticatedLost
        } else {
            ConnectionEnd::Disabled
        };
    }
    loop {
        if !access.is_current() || !client.snapshot().authenticated {
            client.cancel_session();
            let _ = session.join();
            clear_session(shared, identity);
            return if is_current(shared, epoch) {
                ConnectionEnd::AuthenticatedLost
            } else {
                ConnectionEnd::Disabled
            };
        }
        match readiness_with_purge_proof(&access, peer_id, lease) {
            Ok(()) => break,
            Err(failure) if failure.transport_lost => {
                client.cancel_session();
                let _ = session.join();
                clear_session(shared, identity);
                return ConnectionEnd::AuthenticatedLost;
            }
            Err(failure) => {
                set_phase(
                    shared,
                    epoch,
                    AutoUseState::Degraded,
                    failure.reason,
                    Some(peer_id),
                    Some(lease.incarnation),
                );
                if !wait_current(shared, epoch, shared.timing.readiness_retry) {
                    client.cancel_session();
                    let _ = session.join();
                    clear_session(shared, identity);
                    return ConnectionEnd::Disabled;
                }
            }
        }
    }
    publish_available(shared, identity, client.clone(), peer_id, lease);
    let mut next_renewal = Instant::now() + shared.timing.renewal_interval;
    let mut next_readiness = Instant::now() + shared.timing.readiness_retry;
    loop {
        if !is_current(shared, epoch) {
            let _operation = shared
                .operation
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _ = release_lease(&access, lease);
            client.cancel_session();
            drop(_operation);
            let _ = session.join();
            clear_session(shared, identity);
            return ConnectionEnd::Disabled;
        }
        let live = client.snapshot();
        if !live.authenticated
            || live.liveness_age.is_none_or(|age| age > FRESH_LIVENESS)
            || session.is_finished()
        {
            mark_connection_lost_identity(shared, identity, peer_id);
            client.cancel_session();
            let _ = session.join();
            clear_session(shared, identity);
            return ConnectionEnd::AuthenticatedLost;
        }
        if Instant::now() >= next_renewal {
            match renew_lease(&access, lease) {
                Ok(renewed) => {
                    lease = renewed;
                    record_lease_active(shared, identity, lease);
                    if lock_data(shared).status.state == AutoUseState::Available {
                        publish_available(shared, identity, client.clone(), peer_id, lease);
                    }
                    next_renewal = Instant::now() + shared.timing.renewal_interval;
                }
                Err(_) => {
                    record_lease_unavailable(shared, identity, "C07_RENEW_FAILED");
                    client.cancel_session();
                    mark_connection_lost_identity(shared, identity, peer_id);
                    let _ = session.join();
                    clear_session(shared, identity);
                    return ConnectionEnd::AuthenticatedLost;
                }
            }
        }
        let degraded = lock_data(shared).status.state == AutoUseState::Degraded;
        if degraded && Instant::now() >= next_readiness {
            match readiness_with_purge_proof(&access, peer_id, lease) {
                Ok(()) => publish_available(shared, identity, client.clone(), peer_id, lease),
                Err(failure) if failure.transport_lost => {
                    client.cancel_session();
                    mark_connection_lost_identity(shared, identity, peer_id);
                    let _ = session.join();
                    clear_session(shared, identity);
                    return ConnectionEnd::AuthenticatedLost;
                }
                Err(failure) => {
                    mark_degraded_identity(shared, identity, peer_id, lease, failure.reason);
                }
            }
            next_readiness = Instant::now() + shared.timing.readiness_retry;
        }
        if !wait_current(shared, epoch, shared.timing.manager_poll) {
            continue;
        }
    }
}

struct SessionAccess<'a> {
    shared: &'a Shared,
    identity: SessionIdentity,
    client: &'a InitiatorSessionClient,
}

impl SessionAccess<'_> {
    fn is_current(&self) -> bool {
        let data = lock_data(self.shared);
        data.enabled
            && data.epoch == self.identity.epoch
            && data.session_identity == Some(self.identity)
            && !self.shared.shutdown.load(Ordering::Acquire)
    }

    fn require_current(&self) -> Result<(), RemoteFailure> {
        if self.is_current() {
            Ok(())
        } else {
            Err(RemoteFailure::session_stale())
        }
    }

    fn command(&self, request: CommandPayload) -> Result<pb_pbmux::AckPayload, RemoteFailure> {
        self.require_current()?;
        self.client
            .command(request)
            .map_err(RemoteFailure::from_client)
    }

    fn resource(
        &self,
        request: ResourceRequest,
    ) -> Result<pb_pbmux::ResourceResult, RemoteFailure> {
        self.require_current()?;
        self.client
            .resource(request)
            .map_err(RemoteFailure::from_client)
    }

    fn allocate_resource_request_id(&self) -> Result<u64, RemoteFailure> {
        self.require_current()?;
        self.client
            .allocate_resource_request_id()
            .map_err(RemoteFailure::from_client)
    }

    fn resource_with_request_id(
        &self,
        request_id: u64,
        request: ResourceRequest,
    ) -> Result<pb_pbmux::ResourceResult, RemoteFailure> {
        self.require_current()?;
        self.client
            .resource_with_request_id(request_id, request)
            .map_err(RemoteFailure::from_client)
    }

    fn remote_buffer(
        &self,
        request: RemoteBufferRequest,
    ) -> Result<pb_pbmux::BufferResult, RemoteFailure> {
        self.require_current()?;
        self.client
            .remote_buffer(request)
            .map_err(RemoteFailure::from_client)
    }

    fn compute(&self, request: ComputeRequest) -> Result<ComputeResponse, RemoteFailure> {
        self.require_current()?;
        self.client
            .compute(request)
            .map_err(RemoteFailure::from_client)
    }
}

fn acquire_lease(access: &SessionAccess<'_>) -> Result<LeaseContext, RemoteFailure> {
    let sent_at = Instant::now();
    let ack = access.command(controller_command(1, None, 0))?;
    lease_from_ack(ack, None, sent_at)
}

fn renew_lease(
    access: &SessionAccess<'_>,
    lease: LeaseContext,
) -> Result<LeaseContext, RemoteFailure> {
    let sent_at = Instant::now();
    let ack = access.command(controller_command(
        2,
        Some(lease.lease_id),
        lease.next_command_seq,
    ))?;
    lease_from_ack(ack, Some(lease), sent_at)
}

fn release_lease(access: &SessionAccess<'_>, lease: LeaseContext) -> Result<(), RemoteFailure> {
    let ack = access.command(controller_command(
        3,
        Some(lease.lease_id),
        lease.next_command_seq,
    ))?;
    if ack.ack_state == 2
        && ack.reason_code == 0
        && ack.command_seq == lease.next_command_seq
        && ack.result_ref_present == 0
    {
        Ok(())
    } else {
        Err(RemoteFailure::known(AutoUseReason::LeaseUnavailable))
    }
}

fn controller_command(
    command_type: u8,
    lease_id: Option<[u8; 16]>,
    command_seq: u64,
) -> CommandPayload {
    CommandPayload {
        command_type,
        lease_present: u8::from(lease_id.is_some()),
        lease_id: lease_id.unwrap_or([0; 16]),
        command_seq,
        trace_id: [0x41; 16],
        provider_present: 0,
        provider_id: 0,
        payload_len: 0,
    }
}

fn lease_from_ack(
    ack: pb_pbmux::AckPayload,
    previous: Option<LeaseContext>,
    sent_at: Instant,
) -> Result<LeaseContext, RemoteFailure> {
    let expected_command_seq = previous.map_or(0, |lease| lease.next_command_seq);
    if ack.ack_state != 2
        || ack.reason_code != 0
        || ack.command_seq != expected_command_seq
        || ack.result_ref_present != 1
        || ack.lease_id == [0; 16]
        || ack.worker_incarnation == [0; 16]
        || ack.ttl_remaining_ms == 0
        || previous.is_some_and(|old| {
            old.lease_id != ack.lease_id || old.incarnation != ack.worker_incarnation
        })
        || previous.is_some_and(|_| ack.next_command_seq != expected_command_seq.saturating_add(1))
    {
        return Err(RemoteFailure::known(AutoUseReason::LeaseUnavailable));
    }
    Ok(LeaseContext {
        lease_id: ack.lease_id,
        incarnation: ack.worker_incarnation,
        next_command_seq: ack.next_command_seq,
        not_after: sent_at + Duration::from_millis(u64::from(ack.ttl_remaining_ms)),
    })
}

fn readiness_probe(
    access: &SessionAccess<'_>,
    peer_id: PeerId,
    lease: LeaseContext,
) -> Result<(), RemoteFailure> {
    let reservation = reserve_with_cleanup(
        access,
        peer_id,
        lease,
        WireResourceClass::NativeOpScratchBytes,
        1,
    )?;
    let release = match access.resource(ResourceRequest::Release {
        lease_id: lease.lease_id,
        worker_incarnation_id: lease.incarnation,
        reservation_id: reservation,
    }) {
        Ok(release) => release,
        Err(failure) => {
            mark_cleanup_ambiguous(
                access.shared,
                CleanupTarget::Reservation(reservation),
                peer_id,
                lease,
            );
            return Err(failure);
        }
    };
    if release.state == ResourceResultState::Completed
        && release.reason == ResourceReason::None
        && release.lease_id == lease.lease_id
        && release.worker_incarnation_id == lease.incarnation
        && release.reservation.is_some_and(|entry| {
            entry.reservation_id == reservation
                && entry.resource_class == WireResourceClass::NativeOpScratchBytes
                && entry.granted_bytes == 1
                && entry.state == WireReservationState::Released
        })
    {
        remove_cleanup(
            access.shared,
            CleanupTarget::Reservation(reservation),
            peer_id,
            lease,
        );
        let probe_input = [0_u8; 1];
        let digest = execute_remote_blake3(access, peer_id, lease, &probe_input)?;
        if digest == *blake3::hash(&probe_input).as_bytes() {
            Ok(())
        } else {
            Err(RemoteFailure::known(AutoUseReason::WorkerUnhealthy))
        }
    } else {
        Err(resource_failure(release.reason))
    }
}

fn readiness_with_purge_proof(
    access: &SessionAccess<'_>,
    peer_id: PeerId,
    lease: LeaseContext,
) -> Result<(), RemoteFailure> {
    if cleanup_ledger_is_full_with_superseded_entries(access.shared) {
        prove_purge_with_known_superseded_reservation(access, peer_id, lease)?;
    }
    match readiness_probe(access, peer_id, lease) {
        Ok(()) => record_admission_proof(access.shared, access.identity, lease, true),
        Err(failure) => {
            record_admission_proof(access.shared, access.identity, lease, false);
            return Err(failure);
        }
    }
    resolve_superseded_cleanup_after_purge_proof(access.shared, peer_id, lease);
    Ok(())
}

fn cleanup_ledger_is_full_with_superseded_entries(shared: &Shared) -> bool {
    let ledger = shared
        .cleanup
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ledger.entries.len() >= MAX_CLEANUP_OBLIGATIONS
        && ledger
            .entries
            .iter()
            .any(|entry| entry.knowledge == CleanupKnowledge::AwaitingResourceGuardPurgeProof)
}

fn prove_purge_with_known_superseded_reservation(
    access: &SessionAccess<'_>,
    peer_id: PeerId,
    lease: LeaseContext,
) -> Result<(), RemoteFailure> {
    let probe_reservation = access
        .shared
        .cleanup
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entries
        .iter()
        .find_map(|entry| {
            (entry.knowledge == CleanupKnowledge::AwaitingResourceGuardPurgeProof)
                .then_some(entry.target)
                .and_then(|target| match target {
                    CleanupTarget::Reservation(reservation_id) => Some(reservation_id),
                    CleanupTarget::Buffer(_) | CleanupTarget::UnresolvedReserve { .. } => None,
                })
        })
        .ok_or_else(|| RemoteFailure::known(AutoUseReason::ResourceRefused))?;

    // The ID belongs to a superseded lease, so ResourceGuard removes it in
    // expire_and_purge before this replacement-lease RELEASE lookup. An exact
    // NOT_FOUND response therefore proves the lazy purge ran without creating
    // another reservation or needing an additional ledger slot.
    let result = access.resource(ResourceRequest::Release {
        lease_id: lease.lease_id,
        worker_incarnation_id: lease.incarnation,
        reservation_id: probe_reservation,
    })?;
    if result.state == ResourceResultState::Failed
        && result.reason == ResourceReason::ReservationNotFound
        && result.lease_id == lease.lease_id
        && result.worker_incarnation_id == lease.incarnation
        && result.reservation.is_none()
    {
        resolve_superseded_cleanup_after_purge_proof(access.shared, peer_id, lease);
        Ok(())
    } else {
        Err(RemoteFailure::ambiguous())
    }
}

fn mark_cleanup_ambiguous(
    shared: &Shared,
    target: CleanupTarget,
    peer_id: PeerId,
    lease: LeaseContext,
) {
    let mut ledger = shared
        .cleanup
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(entry) = ledger.entries.iter_mut().find(|entry| {
        entry.target == target
            && entry.peer_id == peer_id
            && entry.lease_id == lease.lease_id
            && entry.incarnation == lease.incarnation
    }) {
        entry.knowledge = CleanupKnowledge::Ambiguous;
    }
}

fn ensure_cleanup_capacity(shared: &Shared) -> Result<(), RemoteFailure> {
    let ledger = shared
        .cleanup
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if ledger.entries.len() < MAX_CLEANUP_OBLIGATIONS {
        Ok(())
    } else {
        Err(RemoteFailure::known(AutoUseReason::ResourceRefused))
    }
}

fn track_cleanup(shared: &Shared, obligation: CleanupObligation) -> Result<(), RemoteFailure> {
    let mut ledger = shared
        .cleanup
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if ledger.entries.contains(&obligation) {
        return Ok(());
    }
    if ledger.entries.len() >= MAX_CLEANUP_OBLIGATIONS {
        return Err(RemoteFailure::known(AutoUseReason::ResourceRefused));
    }
    ledger.entries.push(obligation);
    Ok(())
}

fn remove_cleanup(shared: &Shared, target: CleanupTarget, peer_id: PeerId, lease: LeaseContext) {
    shared
        .cleanup
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entries
        .retain(|entry| {
            !(entry.target == target
                && entry.peer_id == peer_id
                && entry.lease_id == lease.lease_id
                && entry.incarnation == lease.incarnation)
        });
}

fn replace_cleanup(
    shared: &Shared,
    from: CleanupTarget,
    to: CleanupTarget,
    peer_id: PeerId,
    lease: LeaseContext,
    origin: SessionIdentity,
) {
    let mut ledger = shared
        .cleanup
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(entry) = ledger.entries.iter_mut().find(|entry| {
        entry.target == from
            && entry.peer_id == peer_id
            && entry.lease_id == lease.lease_id
            && entry.incarnation == lease.incarnation
    }) {
        entry.target = to;
        entry.origin = origin;
        entry.knowledge = CleanupKnowledge::Known;
    }
}

fn reconcile_cleanup(
    access: &SessionAccess<'_>,
    peer_id: PeerId,
    lease: LeaseContext,
) -> Result<(), RemoteFailure> {
    let obligations: Vec<CleanupObligation> = {
        let mut ledger = access
            .shared
            .cleanup
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Buffers are session-bound and become LOST on authenticated session
        // end. They are never resurrected or addressed from a new session.
        // A WorkerCore incarnation change destroys the old ResourceGuard, but
        // a C07 lease change alone does not: superseded reservations are purged
        // lazily only after a valid C08 operation crosses ResourceGuard.
        ledger.entries.retain(|entry| {
            !matches!(entry.target, CleanupTarget::Buffer(_))
                && entry.incarnation == lease.incarnation
        });
        for entry in &mut ledger.entries {
            if entry.peer_id == peer_id && entry.lease_id == lease.lease_id {
                if entry.knowledge == CleanupKnowledge::AwaitingResourceGuardPurgeProof {
                    entry.knowledge = CleanupKnowledge::Ambiguous;
                }
            } else {
                entry.knowledge = CleanupKnowledge::AwaitingResourceGuardPurgeProof;
            }
        }
        ledger
            .entries
            .iter()
            .copied()
            .filter(|entry| {
                entry.peer_id == peer_id
                    && entry.lease_id == lease.lease_id
                    && entry.knowledge != CleanupKnowledge::AwaitingResourceGuardPurgeProof
            })
            .collect()
    };

    for obligation in obligations {
        match obligation.target {
            CleanupTarget::Reservation(reservation_id) => {
                reconcile_reservation_release(access, peer_id, lease, reservation_id)?;
            }
            CleanupTarget::UnresolvedReserve {
                request_id,
                resource_class,
                requested_bytes,
            } => {
                let replay = access.resource_with_request_id(
                    request_id,
                    ResourceRequest::Reserve {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.incarnation,
                        resource_class,
                        requested_bytes,
                    },
                )?;
                if let Ok(reservation_id) =
                    successful_reservation(&replay, lease, resource_class, requested_bytes)
                {
                    replace_cleanup(
                        access.shared,
                        obligation.target,
                        CleanupTarget::Reservation(reservation_id),
                        peer_id,
                        lease,
                        access.identity,
                    );
                    reconcile_reservation_release(access, peer_id, lease, reservation_id)?;
                } else if reserve_refusal_proves_absence(&replay, lease) {
                    remove_cleanup(access.shared, obligation.target, peer_id, lease);
                } else {
                    return Err(RemoteFailure::ambiguous());
                }
            }
            CleanupTarget::Buffer(_) => {}
        }
    }
    Ok(())
}

fn resolve_superseded_cleanup_after_purge_proof(
    shared: &Shared,
    peer_id: PeerId,
    lease: LeaseContext,
) {
    shared
        .cleanup
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entries
        .retain(|entry| {
            !(entry.incarnation == lease.incarnation
                && entry.knowledge == CleanupKnowledge::AwaitingResourceGuardPurgeProof
                && (entry.peer_id != peer_id || entry.lease_id != lease.lease_id))
        });
}

fn reconcile_reservation_release(
    access: &SessionAccess<'_>,
    peer_id: PeerId,
    lease: LeaseContext,
    reservation_id: [u8; 16],
) -> Result<(), RemoteFailure> {
    let target = CleanupTarget::Reservation(reservation_id);
    let result = match access.resource(ResourceRequest::Release {
        lease_id: lease.lease_id,
        worker_incarnation_id: lease.incarnation,
        reservation_id,
    }) {
        Ok(result) => result,
        Err(failure) => {
            mark_cleanup_ambiguous(access.shared, target, peer_id, lease);
            return Err(failure);
        }
    };
    if cleanup_release_terminal(&result, lease, reservation_id) {
        remove_cleanup(access.shared, target, peer_id, lease);
        Ok(())
    } else {
        Err(resource_failure(result.reason))
    }
}

fn cleanup_release_terminal(
    result: &pb_pbmux::ResourceResult,
    lease: LeaseContext,
    reservation_id: [u8; 16],
) -> bool {
    if result.lease_id != lease.lease_id || result.worker_incarnation_id != lease.incarnation {
        return false;
    }
    (result.state == ResourceResultState::Completed
        && result.reason == ResourceReason::None
        && result.reservation.is_some_and(|reservation| {
            reservation.reservation_id == reservation_id
                && reservation.state == WireReservationState::Released
        }))
        || (result.state == ResourceResultState::Failed
            && matches!(
                result.reason,
                ResourceReason::ReservationNotFound
                    | ResourceReason::ReservationNotCommitted
                    | ResourceReason::ReservationExpired
                    | ResourceReason::ReservationAlreadyConsumed
            ))
}

fn successful_reservation(
    result: &pb_pbmux::ResourceResult,
    lease: LeaseContext,
    resource_class: WireResourceClass,
    bytes: u64,
) -> Result<[u8; 16], RemoteFailure> {
    if result.state != ResourceResultState::Completed
        || result.reason != ResourceReason::None
        || result.lease_id != lease.lease_id
        || result.worker_incarnation_id != lease.incarnation
    {
        return Err(resource_failure(result.reason));
    }
    result
        .reservation
        .filter(|reservation| {
            reservation.state == WireReservationState::Reserved
                && reservation.resource_class == resource_class
                && reservation.granted_bytes == bytes
        })
        .map(|reservation| reservation.reservation_id)
        .ok_or_else(|| RemoteFailure::known(AutoUseReason::ResourceRefused))
}

fn resource_failure(reason: ResourceReason) -> RemoteFailure {
    RemoteFailure::known(if reason == ResourceReason::RefusedStaleState {
        AutoUseReason::WorkerUnhealthy
    } else if reason == ResourceReason::StaleControllerLease {
        AutoUseReason::LeaseUnavailable
    } else {
        AutoUseReason::ResourceRefused
    })
}

struct RemoteResources {
    class_one_reservation: Option<[u8; 16]>,
    buffer_id: Option<[u8; 16]>,
    scratch_reservation: Option<[u8; 16]>,
}

fn execute_remote_blake3(
    access: &SessionAccess<'_>,
    peer_id: PeerId,
    lease: LeaseContext,
    input: &[u8],
) -> Result<[u8; 32], RemoteFailure> {
    let mut resources = RemoteResources {
        class_one_reservation: None,
        buffer_id: None,
        scratch_reservation: None,
    };
    let result = (|| {
        let class_one = reserve_and_commit(
            access,
            peer_id,
            lease,
            WireResourceClass::RemoteBufferBytes,
            input.len() as u64,
        )?;
        resources.class_one_reservation = Some(class_one);
        let allocation = match access.remote_buffer(RemoteBufferRequest::Alloc {
            lease_id: lease.lease_id,
            worker_incarnation_id: lease.incarnation,
            reservation_id: class_one,
            size_bytes: input.len() as u64,
            allocation_flags: AllocationFlags::NONE,
        }) {
            Ok(allocation) => allocation,
            Err(failure) => {
                mark_cleanup_ambiguous(
                    access.shared,
                    CleanupTarget::Reservation(class_one),
                    peer_id,
                    lease,
                );
                return Err(failure);
            }
        };
        if !allocation.completed
            || allocation.lease_id != lease.lease_id
            || allocation.worker_incarnation_id != lease.incarnation
            || allocation.reservation_id != Some(class_one)
        {
            return Err(RemoteFailure::known(AutoUseReason::ResourceRefused));
        }
        let allocated = allocation
            .buffer
            .ok_or_else(|| RemoteFailure::known(AutoUseReason::ResourceRefused))?;
        if allocated.state != BufferState::Allocated || allocated.size_bytes != input.len() as u64 {
            return Err(RemoteFailure::known(AutoUseReason::ResourceRefused));
        }
        let buffer = allocated.buffer_id;
        resources.buffer_id = Some(buffer);
        resources.class_one_reservation = None;
        replace_cleanup(
            access.shared,
            CleanupTarget::Reservation(class_one),
            CleanupTarget::Buffer(buffer),
            peer_id,
            lease,
            access.identity,
        );
        for (index, chunk) in input.chunks(MAX_PUT_BODY).enumerate() {
            let offset = index
                .checked_mul(MAX_PUT_BODY)
                .and_then(|value| u64::try_from(value).ok())
                .ok_or_else(|| RemoteFailure::known(AutoUseReason::ResourceRefused))?;
            let put = access.remote_buffer(RemoteBufferRequest::Put {
                lease_id: lease.lease_id,
                worker_incarnation_id: lease.incarnation,
                buffer_id: buffer,
                offset,
                data: chunk.to_vec(),
            })?;
            if !put.completed
                || put.lease_id != lease.lease_id
                || put.worker_incarnation_id != lease.incarnation
                || put.buffer.is_none_or(|entry| entry.buffer_id != buffer)
                || put.offset != offset
                || put.data_len != chunk.len() as u32
            {
                return Err(RemoteFailure::known(AutoUseReason::ResourceRefused));
            }
        }
        let ready = access.remote_buffer(RemoteBufferRequest::Stat {
            lease_id: lease.lease_id,
            worker_incarnation_id: lease.incarnation,
            buffer_id: buffer,
        })?;
        if !ready.completed
            || ready.lease_id != lease.lease_id
            || ready.worker_incarnation_id != lease.incarnation
            || ready
                .buffer
                .is_none_or(|entry| entry.buffer_id != buffer || entry.state != BufferState::Ready)
        {
            return Err(RemoteFailure::known(AutoUseReason::ResourceRefused));
        }
        let scratch = reserve_and_commit(
            access,
            peer_id,
            lease,
            WireResourceClass::NativeOpScratchBytes,
            SCRATCH_BYTES,
        )?;
        resources.scratch_reservation = Some(scratch);
        let mut response = match access.compute(ComputeRequest::Submit(ComputeSubmit {
            lease_id: lease.lease_id,
            worker_incarnation_id: lease.incarnation,
            reservation_id: scratch,
            provider_id: BLAKE3_PROVIDER_ID,
            provider_version: BLAKE3_PROVIDER_VERSION,
            input_kind: REMOTE_BUFFER_INPUT_KIND,
            buffer_id: buffer,
            input_offset: 0,
            input_length: input.len() as u64,
        })) {
            Ok(response) => response,
            Err(failure) => {
                mark_cleanup_ambiguous(
                    access.shared,
                    CleanupTarget::Reservation(scratch),
                    peer_id,
                    lease,
                );
                return Err(failure);
            }
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut admitted_job_id = None;
        loop {
            match response {
                ComputeResponse::Result(result) => {
                    if result.lease_id != lease.lease_id
                        || result.worker_incarnation_id != lease.incarnation
                        || result.job.is_some_and(|job| {
                            job.provider_id != BLAKE3_PROVIDER_ID
                                || job.provider_version != BLAKE3_PROVIDER_VERSION
                                || admitted_job_id.is_some_and(|expected| expected != job.job_id)
                        })
                    {
                        return Err(RemoteFailure::known(AutoUseReason::AuthFailed));
                    }
                    if result.job.is_some() {
                        resources.scratch_reservation = None;
                        remove_cleanup(
                            access.shared,
                            CleanupTarget::Reservation(scratch),
                            peer_id,
                            lease,
                        );
                    }
                    if result.state == ComputeJobState::Completed
                        && result.reason == ComputeReason::None
                        && result.job.is_some()
                        && let Some(digest) = result.digest
                    {
                        return Ok(digest);
                    }
                    return Err(RemoteFailure::known(
                        if matches!(
                            result.reason,
                            ComputeReason::StaleControllerLease
                                | ComputeReason::WrongWorkerIncarnation
                        ) {
                            AutoUseReason::LeaseUnavailable
                        } else {
                            AutoUseReason::ResourceRefused
                        },
                    ));
                }
                ComputeResponse::Status(status) => {
                    if status.lease_id != lease.lease_id
                        || status.worker_incarnation_id != lease.incarnation
                        || status.job.is_some_and(|job| {
                            job.provider_id != BLAKE3_PROVIDER_ID
                                || job.provider_version != BLAKE3_PROVIDER_VERSION
                        })
                    {
                        return Err(RemoteFailure::known(AutoUseReason::AuthFailed));
                    }
                    let Some(job) = status.job else {
                        return Err(RemoteFailure::known(AutoUseReason::ResourceRefused));
                    };
                    if admitted_job_id.is_some_and(|expected| expected != job.job_id) {
                        return Err(RemoteFailure::known(AutoUseReason::AuthFailed));
                    }
                    admitted_job_id = Some(job.job_id);
                    resources.scratch_reservation = None;
                    remove_cleanup(
                        access.shared,
                        CleanupTarget::Reservation(scratch),
                        peer_id,
                        lease,
                    );
                    if Instant::now() >= deadline {
                        return Err(RemoteFailure::ambiguous());
                    }
                    thread::sleep(COMPUTE_STATUS_POLL);
                    response = access.compute(ComputeRequest::Status(ComputeJobRequest {
                        lease_id: lease.lease_id,
                        worker_incarnation_id: lease.incarnation,
                        job_id: job.job_id,
                    }))?;
                }
                ComputeResponse::Cancel(_) => {
                    return Err(RemoteFailure::known(AutoUseReason::ResourceRefused));
                }
            }
        }
    })();
    match cleanup_remote(access, peer_id, lease, &mut resources) {
        Ok(()) => result,
        Err(cleanup_failure) => Err(cleanup_failure),
    }
}

fn reserve_and_commit(
    access: &SessionAccess<'_>,
    peer_id: PeerId,
    lease: LeaseContext,
    resource_class: WireResourceClass,
    bytes: u64,
) -> Result<[u8; 16], RemoteFailure> {
    let reservation = reserve_with_cleanup(access, peer_id, lease, resource_class, bytes)?;
    let commit = match access.resource(ResourceRequest::Commit {
        lease_id: lease.lease_id,
        worker_incarnation_id: lease.incarnation,
        reservation_id: reservation,
    }) {
        Ok(commit) => commit,
        Err(failure) => {
            mark_cleanup_ambiguous(
                access.shared,
                CleanupTarget::Reservation(reservation),
                peer_id,
                lease,
            );
            return Err(failure);
        }
    };
    if commit.state == ResourceResultState::Completed
        && commit.reason == ResourceReason::None
        && commit.lease_id == lease.lease_id
        && commit.worker_incarnation_id == lease.incarnation
        && commit.reservation.is_some_and(|entry| {
            entry.reservation_id == reservation
                && entry.resource_class == resource_class
                && entry.granted_bytes == bytes
                && entry.state == WireReservationState::Committed
        })
    {
        Ok(reservation)
    } else {
        if let Ok(release) = access.resource(ResourceRequest::Release {
            lease_id: lease.lease_id,
            worker_incarnation_id: lease.incarnation,
            reservation_id: reservation,
        }) && cleanup_release_terminal(&release, lease, reservation)
        {
            remove_cleanup(
                access.shared,
                CleanupTarget::Reservation(reservation),
                peer_id,
                lease,
            );
        }
        Err(resource_failure(commit.reason))
    }
}

fn reserve_with_cleanup(
    access: &SessionAccess<'_>,
    peer_id: PeerId,
    lease: LeaseContext,
    resource_class: WireResourceClass,
    bytes: u64,
) -> Result<[u8; 16], RemoteFailure> {
    ensure_cleanup_capacity(access.shared)?;
    let request_id = access.allocate_resource_request_id()?;
    let unresolved = CleanupTarget::UnresolvedReserve {
        request_id,
        resource_class,
        requested_bytes: bytes,
    };
    track_cleanup(
        access.shared,
        CleanupObligation {
            peer_id,
            lease_id: lease.lease_id,
            incarnation: lease.incarnation,
            origin: access.identity,
            target: unresolved,
            knowledge: CleanupKnowledge::Ambiguous,
        },
    )?;
    let reserve_request = ResourceRequest::Reserve {
        lease_id: lease.lease_id,
        worker_incarnation_id: lease.incarnation,
        resource_class,
        requested_bytes: bytes,
    };
    let reserve = match access.resource_with_request_id(request_id, reserve_request) {
        Ok(reserve) => reserve,
        Err(failure) => {
            return Err(failure);
        }
    };
    if let Ok(reservation) = successful_reservation(&reserve, lease, resource_class, bytes) {
        replace_cleanup(
            access.shared,
            unresolved,
            CleanupTarget::Reservation(reservation),
            peer_id,
            lease,
            access.identity,
        );
        Ok(reservation)
    } else if reserve_refusal_proves_absence(&reserve, lease) {
        remove_cleanup(access.shared, unresolved, peer_id, lease);
        Err(resource_failure(reserve.reason))
    } else {
        Err(RemoteFailure::ambiguous())
    }
}

fn reserve_refusal_proves_absence(result: &pb_pbmux::ResourceResult, lease: LeaseContext) -> bool {
    result.state == ResourceResultState::Failed
        && result.lease_id == lease.lease_id
        && result.worker_incarnation_id == lease.incarnation
        && result.reservation.is_none()
        // These are the only typed RESERVE outcomes produced after live C07
        // validation and ResourceGuard's exact request evaluation without
        // creating a reservation. Stale authority, request-id conflict,
        // internal failure, and other responses do not prove absence.
        && matches!(
            result.reason,
            ResourceReason::RefusedStaleState | ResourceReason::ResourceExhausted
        )
}

fn cleanup_remote(
    access: &SessionAccess<'_>,
    peer_id: PeerId,
    lease: LeaseContext,
    resources: &mut RemoteResources,
) -> Result<(), RemoteFailure> {
    let mut unresolved = false;
    if let Some(buffer_id) = resources.buffer_id.take() {
        match access.remote_buffer(RemoteBufferRequest::Free {
            lease_id: lease.lease_id,
            worker_incarnation_id: lease.incarnation,
            buffer_id,
        }) {
            Ok(result) if buffer_cleanup_terminal(&result, lease, buffer_id) => remove_cleanup(
                access.shared,
                CleanupTarget::Buffer(buffer_id),
                peer_id,
                lease,
            ),
            _ => {
                mark_cleanup_ambiguous(
                    access.shared,
                    CleanupTarget::Buffer(buffer_id),
                    peer_id,
                    lease,
                );
                unresolved = true;
            }
        }
    }
    for reservation_id in [
        resources.scratch_reservation.take(),
        resources.class_one_reservation.take(),
    ]
    .into_iter()
    .flatten()
    {
        match access.resource(ResourceRequest::Release {
            lease_id: lease.lease_id,
            worker_incarnation_id: lease.incarnation,
            reservation_id,
        }) {
            Ok(result) if cleanup_release_terminal(&result, lease, reservation_id) => {
                remove_cleanup(
                    access.shared,
                    CleanupTarget::Reservation(reservation_id),
                    peer_id,
                    lease,
                );
            }
            _ => {
                mark_cleanup_ambiguous(
                    access.shared,
                    CleanupTarget::Reservation(reservation_id),
                    peer_id,
                    lease,
                );
                unresolved = true;
            }
        }
    }
    if unresolved {
        Err(RemoteFailure::ambiguous())
    } else {
        Ok(())
    }
}

fn buffer_cleanup_terminal(
    result: &pb_pbmux::BufferResult,
    lease: LeaseContext,
    buffer_id: [u8; 16],
) -> bool {
    if result.lease_id != lease.lease_id || result.worker_incarnation_id != lease.incarnation {
        return false;
    }
    (result.completed
        && result.reason == BufferReason::None
        && result.buffer.is_some_and(|buffer| {
            buffer.buffer_id == buffer_id && buffer.state == BufferState::Freed
        }))
        || (!result.completed
            && matches!(
                result.reason,
                BufferReason::BufferNotFound
                    | BufferReason::BufferLost
                    | BufferReason::BufferFreed
                    | BufferReason::BufferEvicted
            ))
}

#[derive(Clone, Copy, Debug)]
struct RemoteFailure {
    reason: AutoUseReason,
    ambiguous: bool,
    transport_lost: bool,
}

impl RemoteFailure {
    const fn known(reason: AutoUseReason) -> Self {
        Self {
            reason,
            ambiguous: false,
            transport_lost: false,
        }
    }

    const fn ambiguous() -> Self {
        Self {
            reason: AutoUseReason::TransportLost,
            ambiguous: true,
            transport_lost: true,
        }
    }

    const fn session_stale() -> Self {
        Self {
            reason: AutoUseReason::TransportLost,
            ambiguous: false,
            transport_lost: true,
        }
    }

    fn from_client(error: InitiatorClientError) -> Self {
        match error {
            InitiatorClientError::UnknownAfterDisconnect | InitiatorClientError::Timeout => {
                Self::ambiguous()
            }
            InitiatorClientError::NotAuthenticated | InitiatorClientError::SessionLost => Self {
                reason: AutoUseReason::TransportLost,
                ambiguous: false,
                transport_lost: true,
            },
            InitiatorClientError::Busy => Self::known(AutoUseReason::WorkerUnhealthy),
            InitiatorClientError::InvalidRequest | InitiatorClientError::ResponseMismatch => {
                Self::known(AutoUseReason::AuthFailed)
            }
        }
    }
}

fn local_execution(
    input: &[u8],
    reason: AutoUseReason,
    source: ExecutionSource,
) -> Blake3Execution {
    Blake3Execution {
        digest: *blake3::hash(input).as_bytes(),
        source,
        reason,
    }
}

fn publish_available(
    shared: &Shared,
    identity: SessionIdentity,
    client: InitiatorSessionClient,
    peer_id: PeerId,
    lease: LeaseContext,
) {
    let mut data = lock_data(shared);
    if data.enabled
        && data.epoch == identity.epoch
        && data.session_identity == Some(identity)
        && client.snapshot().authenticated
    {
        data.active = Some(ActiveContext {
            client,
            identity,
            peer_id,
            lease,
        });
        data.status = node_status(
            AutoUseState::Available,
            AutoUseReason::Ready,
            Some(peer_id),
            Some(lease.incarnation),
        );
        shared.changed.notify_all();
    }
}

fn mark_connection_lost(shared: &Shared, active: &ActiveContext) {
    mark_connection_lost_identity(shared, active.identity, active.peer_id);
}

fn mark_connection_lost_identity(shared: &Shared, identity: SessionIdentity, peer_id: PeerId) {
    let mut data = lock_data(shared);
    if data.enabled && data.session_identity == Some(identity) {
        data.active = None;
        data.status = node_status(
            AutoUseState::Reconnecting,
            AutoUseReason::Reconnecting,
            Some(peer_id),
            None,
        );
        shared.changed.notify_all();
    }
}

fn mark_degraded(shared: &Shared, active: &ActiveContext, reason: AutoUseReason) {
    mark_degraded_identity(
        shared,
        active.identity,
        active.peer_id,
        active.lease,
        reason,
    );
}

fn mark_degraded_identity(
    shared: &Shared,
    identity: SessionIdentity,
    peer_id: PeerId,
    lease: LeaseContext,
    reason: AutoUseReason,
) {
    let mut data = lock_data(shared);
    if data.enabled && data.session_identity == Some(identity) {
        data.active = None;
        data.status = node_status(
            AutoUseState::Degraded,
            reason,
            Some(peer_id),
            Some(lease.incarnation),
        );
        shared.changed.notify_all();
    }
}

fn set_phase(
    shared: &Shared,
    epoch: u64,
    state: AutoUseState,
    reason: AutoUseReason,
    peer_id: Option<PeerId>,
    incarnation: Option<[u8; 16]>,
) {
    let mut data = lock_data(shared);
    if data.enabled && data.epoch == epoch {
        data.active = None;
        data.status = node_status(state, reason, peer_id, incarnation);
        shared.changed.notify_all();
    }
}

fn clear_session(shared: &Shared, identity: SessionIdentity) {
    let mut data = lock_data(shared);
    if data.session_identity == Some(identity) {
        data.session_client = None;
        data.session_identity = None;
        data.active = None;
        data.discovery_observation = None;
        data.discovery_unknown = GateObservation::new("UNKNOWN", "EPOCH_INVALIDATED");
        data.lease_observation = unavailable_lease("SESSION_INVALIDATED");
        data.admission_proof_observation = unknown_admission_proof("SESSION_INVALIDATED");
        shared.changed.notify_all();
    }
}

fn node_status(
    state: AutoUseState,
    reason: AutoUseReason,
    peer_id: Option<PeerId>,
    worker_incarnation: Option<[u8; 16]>,
) -> NodeStatus {
    NodeStatus {
        state,
        reason,
        peer_id,
        worker_incarnation,
        remote_blake3_available: state == AutoUseState::Available && reason == AutoUseReason::Ready,
    }
}

fn is_current(shared: &Shared, epoch: u64) -> bool {
    let data = lock_data(shared);
    data.enabled && data.epoch == epoch && !shared.shutdown.load(Ordering::Acquire)
}

fn wait_current(shared: &Shared, epoch: u64, duration: Duration) -> bool {
    let data = lock_data(shared);
    if !data.enabled || data.epoch != epoch || shared.shutdown.load(Ordering::Acquire) {
        return false;
    }
    let waited = shared
        .changed
        .wait_timeout(data, duration)
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    waited.0.enabled && waited.0.epoch == epoch && !shared.shutdown.load(Ordering::Acquire)
}

fn lock_data(shared: &Shared) -> std::sync::MutexGuard<'_, ControllerData> {
    shared
        .data
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs::{self, File};
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::os::fd::OwnedFd;
    use std::os::unix::fs::DirBuilderExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize};

    use pb_runtime_secure::{
        AckPayload, AuthenticatedCommandHandler, AuthenticatedCommandHandlerError, BufferResult,
        CommandPayload, ComputeRequest, ComputeResponse, EndpointRole, PeerRecord,
        RemoteBufferResponseKind, ResourceResponseKind, ResourceResult, StateStore,
        VerifiedPeerSession, run_responder_session_with_handler,
    };
    use pb_worker_core::{
        ControllerCommand, ControllerCommandResult, ControllerFailureReason, ControllerLeaseRef,
        HealthSample, LeaseId, POC_CAP_BYTES, ThermalBand, WorkerCore,
    };

    use super::*;

    static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn public_auto_use_string_mappings_are_exhaustive_and_exact() {
        assert_eq!(
            [
                AutoUseState::Off,
                AutoUseState::Discovering,
                AutoUseState::Connecting,
                AutoUseState::Authenticating,
                AutoUseState::AcquiringAuthority,
                AutoUseState::CheckingReadiness,
                AutoUseState::Available,
                AutoUseState::Degraded,
                AutoUseState::Reconnecting,
                AutoUseState::Unavailable,
            ]
            .map(AutoUseState::as_str),
            [
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
            ]
        );
        assert_eq!(
            [
                AutoUseReason::Off,
                AutoUseReason::NoDevice,
                AutoUseReason::NotPaired,
                AutoUseReason::AuthFailed,
                AutoUseReason::LeaseUnavailable,
                AutoUseReason::WorkerUnhealthy,
                AutoUseReason::ResourceRefused,
                AutoUseReason::TransportLost,
                AutoUseReason::Reconnecting,
                AutoUseReason::DiscoveryBackendUnavailable,
                AutoUseReason::Ready,
            ]
            .map(AutoUseReason::as_str),
            [
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
            ]
        );
        assert_eq!(
            [
                ExecutionSource::RemoteSuccess,
                ExecutionSource::LocalFallbackAfterRemoteUnavailable,
                ExecutionSource::LocalFallbackAfterAmbiguousRemote,
            ]
            .map(ExecutionSource::as_str),
            [
                "REMOTE_SUCCESS",
                "LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE",
                "LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE",
            ]
        );
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "phoneboost-auto-use-{}-{}",
                std::process::id(),
                TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&path)
                .expect("private test directory");
            Self(path)
        }

        fn store(&self, name: &str) -> StateStore {
            let path = self.0.join(name);
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&path)
                .expect("private authority directory");
            let fd: OwnedFd = File::open(path).expect("authority directory").into();
            StateStore::from_directory_fd(fd).expect("test state store")
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn paired_runtimes() -> (TestDirectory, Arc<SecureRuntime>, Arc<SecureRuntime>) {
        let directory = TestDirectory::new();
        let host_store = directory.store("host");
        let android_store = directory.store("android");
        let host_identity = host_store.load_or_create_identity().expect("host identity");
        let android_identity = android_store
            .load_or_create_identity()
            .expect("android identity");
        host_store
            .commit_peer(&PeerRecord::new(
                *android_identity.public(),
                "Android worker",
                1,
            ))
            .expect("host pins Android");
        android_store
            .commit_peer(&PeerRecord::new(*host_identity.public(), "Linux host", 1))
            .expect("Android pins host");
        let host = Arc::new(
            SecureRuntime::initialize(EndpointRole::LinuxInitiator, host_store)
                .expect("host runtime"),
        );
        let android = Arc::new(
            SecureRuntime::initialize(EndpointRole::AndroidResponder, android_store)
                .expect("Android runtime"),
        );
        (directory, host, android)
    }

    fn host_with_dummy_pin() -> (TestDirectory, Arc<SecureRuntime>) {
        let directory = TestDirectory::new();
        let store = directory.store("host");
        store.load_or_create_identity().expect("host identity");
        store
            .commit_peer(&PeerRecord::new([0x72; 32], "absent Android", 1))
            .expect("dummy pin");
        let runtime = Arc::new(
            SecureRuntime::initialize(EndpointRole::LinuxInitiator, store).expect("host runtime"),
        );
        (directory, runtime)
    }

    fn wrong_pinned_runtimes() -> (TestDirectory, Arc<SecureRuntime>, Arc<SecureRuntime>) {
        let directory = TestDirectory::new();
        let host_store = directory.store("host");
        let expected_android_store = directory.store("expected-android");
        let actual_android_store = directory.store("actual-android");
        let host_identity = host_store.load_or_create_identity().expect("host identity");
        let expected_android = expected_android_store
            .load_or_create_identity()
            .expect("expected Android identity");
        actual_android_store
            .load_or_create_identity()
            .expect("actual Android identity");
        host_store
            .commit_peer(&PeerRecord::new(
                *expected_android.public(),
                "expected Android",
                1,
            ))
            .expect("host pin");
        actual_android_store
            .commit_peer(&PeerRecord::new(*host_identity.public(), "Linux host", 1))
            .expect("actual Android host pin");
        let host = Arc::new(
            SecureRuntime::initialize(EndpointRole::LinuxInitiator, host_store)
                .expect("host runtime"),
        );
        let actual_android = Arc::new(
            SecureRuntime::initialize(EndpointRole::AndroidResponder, actual_android_store)
                .expect("actual Android runtime"),
        );
        (directory, host, actual_android)
    }

    struct UnavailableHandler;

    impl AuthenticatedCommandHandler for UnavailableHandler {
        fn handle_authenticated_command(
            &self,
            _verified_session: &VerifiedPeerSession<'_>,
            _request_id: u64,
            _command: CommandPayload,
        ) -> Result<AckPayload, AuthenticatedCommandHandlerError> {
            Err(AuthenticatedCommandHandlerError::Unavailable)
        }
    }

    struct TestWorker {
        core: Mutex<WorkerCore>,
        force_busy: AtomicBool,
        force_resource_refusal: AtomicBool,
        command_types: Mutex<Vec<u8>>,
        acquire_leases: Mutex<Vec<[u8; 16]>>,
        block_compute: AtomicBool,
        compute_entered: AtomicBool,
        release_compute: AtomicBool,
        block_stage: AtomicU8,
        stage_entered: AtomicBool,
        release_stage: AtomicBool,
        remote_request_count: AtomicUsize,
        reservation_classes: Mutex<HashMap<[u8; 16], WireResourceClass>>,
        fail_commit_class: Mutex<Option<WireResourceClass>>,
        reserve_failure: Mutex<Option<ReserveFailure>>,
        reserve_attempts: Mutex<Vec<(u64, WireResourceClass)>>,
        reserve_results: Mutex<HashMap<u64, [u8; 16]>>,
        released_reservations: Mutex<Vec<[u8; 16]>>,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    #[repr(u8)]
    enum TestRequestStage {
        ResourceReserve = 1,
        RemoteBufferPut = 2,
        ComputeSubmit = 3,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ReserveFailureTiming {
        BeforeExecution,
        AfterExecution,
    }

    #[derive(Clone, Copy, Debug)]
    struct ReserveFailure {
        resource_class: WireResourceClass,
        timing: ReserveFailureTiming,
        remaining: usize,
    }

    impl TestWorker {
        fn healthy() -> Arc<Self> {
            let mut core = WorkerCore::cold_start().expect("worker");
            refresh_health(&mut core);
            thread::sleep(Duration::from_millis(10_050));
            refresh_health(&mut core);
            Arc::new(Self {
                core: Mutex::new(core),
                force_busy: AtomicBool::new(false),
                force_resource_refusal: AtomicBool::new(false),
                command_types: Mutex::new(Vec::new()),
                acquire_leases: Mutex::new(Vec::new()),
                block_compute: AtomicBool::new(false),
                compute_entered: AtomicBool::new(false),
                release_compute: AtomicBool::new(false),
                block_stage: AtomicU8::new(0),
                stage_entered: AtomicBool::new(false),
                release_stage: AtomicBool::new(false),
                remote_request_count: AtomicUsize::new(0),
                reservation_classes: Mutex::new(HashMap::new()),
                fail_commit_class: Mutex::new(None),
                reserve_failure: Mutex::new(None),
                reserve_attempts: Mutex::new(Vec::new()),
                reserve_results: Mutex::new(HashMap::new()),
                released_reservations: Mutex::new(Vec::new()),
            })
        }

        fn block(&self, stage: TestRequestStage) {
            self.stage_entered.store(false, Ordering::Release);
            self.release_stage.store(false, Ordering::Release);
            self.block_stage.store(stage as u8, Ordering::Release);
        }

        fn before_request(&self, stage: Option<TestRequestStage>) {
            self.remote_request_count.fetch_add(1, Ordering::AcqRel);
            if let Some(stage) = stage
                && self.block_stage.load(Ordering::Acquire) == stage as u8
            {
                self.stage_entered.store(true, Ordering::Release);
                while !self.release_stage.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(2));
                }
                self.block_stage.store(0, Ordering::Release);
            }
        }

        fn unblock(&self) {
            self.release_stage.store(true, Ordering::Release);
        }

        fn fail_next_commit(&self, resource_class: WireResourceClass) {
            *self
                .fail_commit_class
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(resource_class);
        }

        fn fail_reserve_responses(
            &self,
            resource_class: WireResourceClass,
            timing: ReserveFailureTiming,
            count: usize,
        ) {
            assert!(count > 0);
            *self
                .reserve_failure
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ReserveFailure {
                resource_class,
                timing,
                remaining: count,
            });
        }

        fn consume_reserve_failure(
            &self,
            resource_class: WireResourceClass,
            timing: ReserveFailureTiming,
        ) -> bool {
            let mut failure = self
                .reserve_failure
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(configured) = failure.as_mut() else {
                return false;
            };
            if configured.resource_class != resource_class || configured.timing != timing {
                return false;
            }
            configured.remaining -= 1;
            if configured.remaining == 0 {
                *failure = None;
            }
            true
        }

        fn reserve_request_ids(&self, resource_class: WireResourceClass) -> Vec<u64> {
            self.reserve_attempts
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .filter_map(|(request_id, seen_class)| {
                    (*seen_class == resource_class).then_some(*request_id)
                })
                .collect()
        }

        fn reservation_for_request(&self, request_id: u64) -> Option<[u8; 16]> {
            self.reserve_results
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&request_id)
                .copied()
        }

        fn release_count(&self, reservation_id: [u8; 16]) -> usize {
            self.released_reservations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .filter(|released| **released == reservation_id)
                .count()
        }

        fn incarnation(&self) -> [u8; 16] {
            self.core
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .incarnation()
                .into_bytes()
        }

        fn restart(&self) -> [u8; 16] {
            let mut core = self
                .core
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *core = WorkerCore::cold_start().expect("restarted worker");
            refresh_health(&mut core);
            thread::sleep(Duration::from_millis(10_050));
            refresh_health(&mut core);
            core.incarnation().into_bytes()
        }

        fn acquire_leases(&self) -> Vec<[u8; 16]> {
            self.acquire_leases
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }

        fn command_count(&self, command_type: u8) -> usize {
            self.command_types
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .filter(|seen| **seen == command_type)
                .count()
        }
    }

    impl AuthenticatedCommandHandler for TestWorker {
        fn handle_authenticated_command(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            _request_id: u64,
            command: CommandPayload,
        ) -> Result<AckPayload, AuthenticatedCommandHandlerError> {
            self.before_request(None);
            self.command_types
                .lock()
                .map_err(|_| AuthenticatedCommandHandlerError::Failed)?
                .push(command.command_type);
            if command.command_type == 1 && self.force_busy.load(Ordering::Acquire) {
                return Ok(failed_ack(0, ControllerFailureReason::ControllerBusy, None));
            }
            let command_type = command.command_type;
            let command = match command.command_type {
                1 => ControllerCommand::Acquire,
                2 => ControllerCommand::Renew {
                    lease_id: LeaseId::from_bytes(command.lease_id),
                    command_seq: command.command_seq,
                },
                3 => ControllerCommand::Release {
                    lease_id: LeaseId::from_bytes(command.lease_id),
                    command_seq: command.command_seq,
                },
                _ => ControllerCommand::Unsupported {
                    command_seq: command.command_seq,
                },
            };
            let result = self
                .core
                .lock()
                .map_err(|_| AuthenticatedCommandHandlerError::Failed)?
                .apply_controller_command(verified_session, command)
                .map_err(|_| AuthenticatedCommandHandlerError::Failed)?;
            let ack = controller_ack(result);
            if command_type == 1 && ack.result_ref_present == 1 {
                self.acquire_leases
                    .lock()
                    .map_err(|_| AuthenticatedCommandHandlerError::Failed)?
                    .push(ack.lease_id);
            }
            Ok(ack)
        }

        fn handle_authenticated_resource(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            request: ResourceRequest,
        ) -> Result<(ResourceResponseKind, ResourceResult), AuthenticatedCommandHandlerError>
        {
            let stage = matches!(&request, ResourceRequest::Reserve { .. })
                .then_some(TestRequestStage::ResourceReserve);
            self.before_request(stage);
            let mut core = self
                .core
                .lock()
                .map_err(|_| AuthenticatedCommandHandlerError::Failed)?;
            refresh_health(&mut core);
            if self.force_resource_refusal.load(Ordering::Acquire) {
                let kind = match &request {
                    ResourceRequest::Reserve { .. } => ResourceResponseKind::ReserveAck,
                    ResourceRequest::Commit { .. } => ResourceResponseKind::Commit,
                    ResourceRequest::Release { .. } => ResourceResponseKind::Release,
                };
                return Ok((
                    kind,
                    ResourceResult {
                        state: ResourceResultState::Failed,
                        reason: ResourceReason::ResourceExhausted,
                        lease_id: *request.lease_id(),
                        worker_incarnation_id: core.incarnation().into_bytes(),
                        reservation: None,
                    },
                ));
            }
            let request_copy = request.clone();
            if let ResourceRequest::Reserve { resource_class, .. } = &request_copy {
                self.reserve_attempts
                    .lock()
                    .map_err(|_| AuthenticatedCommandHandlerError::Failed)?
                    .push((request_id, *resource_class));
                if self
                    .consume_reserve_failure(*resource_class, ReserveFailureTiming::BeforeExecution)
                {
                    return Err(AuthenticatedCommandHandlerError::Failed);
                }
            }
            if let ResourceRequest::Release { reservation_id, .. } = &request_copy {
                self.released_reservations
                    .lock()
                    .map_err(|_| AuthenticatedCommandHandlerError::Failed)?
                    .push(*reservation_id);
            }
            let response = core.apply_resource_request(verified_session, request_id, request);
            if let ResourceRequest::Reserve { resource_class, .. } = request_copy
                && let Some(reservation) = response.1.reservation
            {
                self.reservation_classes
                    .lock()
                    .map_err(|_| AuthenticatedCommandHandlerError::Failed)?
                    .insert(reservation.reservation_id, resource_class);
                self.reserve_results
                    .lock()
                    .map_err(|_| AuthenticatedCommandHandlerError::Failed)?
                    .insert(request_id, reservation.reservation_id);
                if self
                    .consume_reserve_failure(resource_class, ReserveFailureTiming::AfterExecution)
                {
                    return Err(AuthenticatedCommandHandlerError::Failed);
                }
            }
            if let ResourceRequest::Commit { reservation_id, .. } = request_copy {
                let resource_class = self
                    .reservation_classes
                    .lock()
                    .map_err(|_| AuthenticatedCommandHandlerError::Failed)?
                    .get(&reservation_id)
                    .copied();
                let mut fail = self
                    .fail_commit_class
                    .lock()
                    .map_err(|_| AuthenticatedCommandHandlerError::Failed)?;
                if resource_class.is_some() && *fail == resource_class {
                    *fail = None;
                    return Err(AuthenticatedCommandHandlerError::Failed);
                }
            }
            Ok(response)
        }

        fn handle_authenticated_remote_buffer(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            _request_id: u64,
            request: RemoteBufferRequest,
        ) -> Result<(RemoteBufferResponseKind, BufferResult), AuthenticatedCommandHandlerError>
        {
            self.before_request(
                matches!(&request, RemoteBufferRequest::Put { .. })
                    .then_some(TestRequestStage::RemoteBufferPut),
            );
            Ok(self
                .core
                .lock()
                .map_err(|_| AuthenticatedCommandHandlerError::Failed)?
                .apply_remote_buffer_request(verified_session, request))
        }

        fn handle_authenticated_compute(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
            request_id: u64,
            request: ComputeRequest,
        ) -> Result<ComputeResponse, AuthenticatedCommandHandlerError> {
            self.before_request(
                matches!(&request, ComputeRequest::Submit(_))
                    .then_some(TestRequestStage::ComputeSubmit),
            );
            if self.block_compute.load(Ordering::Acquire) {
                self.compute_entered.store(true, Ordering::Release);
                while !self.release_compute.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(5));
                }
            }
            Ok(self
                .core
                .lock()
                .map_err(|_| AuthenticatedCommandHandlerError::Failed)?
                .apply_compute_request(verified_session, request_id, request))
        }

        fn authenticated_session_ended(
            &self,
            verified_session: &VerifiedPeerSession<'_>,
        ) -> Result<(), AuthenticatedCommandHandlerError> {
            self.core
                .lock()
                .map_err(|_| AuthenticatedCommandHandlerError::Failed)?
                .authenticated_session_ended(verified_session);
            Ok(())
        }
    }

    fn refresh_health(core: &mut WorkerCore) {
        core.record_local_health(HealthSample {
            available_memory_bytes: 2_147_483_648,
            low_memory: false,
            thermal: ThermalBand::None,
            battery_percent: 80,
            charging: false,
            power_save: false,
            monotonic_ms: 0,
        });
    }

    fn controller_ack(result: ControllerCommandResult) -> AckPayload {
        match result {
            ControllerCommandResult::Completed { command_seq, lease } => {
                completed_ack(command_seq, lease)
            }
            ControllerCommandResult::Failed {
                command_seq,
                reason,
                expected_next_seq,
            } => failed_ack(command_seq, reason, expected_next_seq),
        }
    }

    fn completed_ack(command_seq: u64, lease: Option<ControllerLeaseRef>) -> AckPayload {
        let (present, lease_id, incarnation, ttl, next) =
            lease.map_or((0, [0; 16], [0; 16], 0, 0), |lease| {
                (
                    1,
                    lease.lease_id.into_bytes(),
                    lease.worker_incarnation_id.into_bytes(),
                    lease.ttl_remaining_ms,
                    lease.next_command_seq,
                )
            });
        AckPayload {
            ack_state: 2,
            reason_code: 0,
            command_seq,
            expected_present: 0,
            expected: 0,
            result_ref_present: present,
            lease_id,
            worker_incarnation: incarnation,
            ttl_remaining_ms: ttl,
            next_command_seq: next,
            digest_present: 0,
            digest: [0; 32],
        }
    }

    fn failed_ack(
        command_seq: u64,
        reason: ControllerFailureReason,
        expected: Option<u64>,
    ) -> AckPayload {
        AckPayload {
            ack_state: 3,
            reason_code: match reason {
                ControllerFailureReason::ControllerBusy => 1,
                ControllerFailureReason::StaleControllerLease => 2,
                ControllerFailureReason::OutOfOrder => 3,
                ControllerFailureReason::DuplicateResultEvicted => 4,
                ControllerFailureReason::UnsupportedMessage => 5,
            },
            command_seq,
            expected_present: u8::from(expected.is_some()),
            expected: expected.unwrap_or(0),
            result_ref_present: 0,
            lease_id: [0; 16],
            worker_incarnation: [0; 16],
            ttl_remaining_ms: 0,
            next_command_seq: 0,
            digest_present: 0,
            digest: [0; 32],
        }
    }

    struct TestDiscovery {
        candidate: TransportCandidate,
        present: Arc<AtomicBool>,
        calls: AtomicUsize,
    }

    struct UnavailableDiscovery {
        starts: AtomicUsize,
        stops: AtomicUsize,
    }

    impl DeviceDiscovery for UnavailableDiscovery {
        fn start(&self) -> Result<(), DiscoveryError> {
            self.starts.fetch_add(1, Ordering::Relaxed);
            Err(DiscoveryError::BackendUnavailable)
        }

        fn discover(&self) -> Result<Option<TransportCandidate>, DiscoveryError> {
            panic!("unavailable backend must not produce candidates")
        }

        fn stop(&self) {
            self.stops.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl DeviceDiscovery for TestDiscovery {
        fn discover(&self) -> Result<Option<TransportCandidate>, DiscoveryError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(self
                .present
                .load(Ordering::Acquire)
                .then_some(self.candidate))
        }
    }

    struct TestDevice {
        endpoint: std::net::SocketAddr,
        present: Arc<AtomicBool>,
        connections: Arc<Mutex<Vec<TcpStream>>>,
        stop: Arc<AtomicBool>,
        allow_auth: Arc<AtomicBool>,
        auth_entered: Arc<AtomicBool>,
        server: Option<JoinHandle<()>>,
    }

    impl TestDevice {
        fn start(
            runtime: Arc<SecureRuntime>,
            handler: Arc<dyn AuthenticatedCommandHandler>,
        ) -> Self {
            Self::start_with_auth(runtime, handler, true)
        }

        fn start_auth_blocked(
            runtime: Arc<SecureRuntime>,
            handler: Arc<dyn AuthenticatedCommandHandler>,
        ) -> Self {
            Self::start_with_auth(runtime, handler, false)
        }

        fn start_with_auth(
            runtime: Arc<SecureRuntime>,
            handler: Arc<dyn AuthenticatedCommandHandler>,
            allow_auth_initially: bool,
        ) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("loopback listener");
            listener
                .set_nonblocking(true)
                .expect("nonblocking listener");
            let endpoint = listener.local_addr().expect("loopback endpoint");
            let present = Arc::new(AtomicBool::new(true));
            let connections = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let allow_auth = Arc::new(AtomicBool::new(allow_auth_initially));
            let auth_entered = Arc::new(AtomicBool::new(false));
            let server_connections = Arc::clone(&connections);
            let server_stop = Arc::clone(&stop);
            let server_allow_auth = Arc::clone(&allow_auth);
            let server_auth_entered = Arc::clone(&auth_entered);
            let server = thread::spawn(move || {
                while !server_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            server_auth_entered.store(true, Ordering::Release);
                            while !server_allow_auth.load(Ordering::Acquire)
                                && !server_stop.load(Ordering::Acquire)
                            {
                                thread::sleep(Duration::from_millis(2));
                            }
                            if let Ok(clone) = stream.try_clone() {
                                server_connections
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                                    .push(clone);
                            }
                            let _ = run_responder_session_with_handler(
                                &mut stream,
                                &runtime,
                                &[],
                                handler.as_ref(),
                            );
                            server_connections
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .clear();
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => return,
                    }
                }
            });
            Self {
                endpoint,
                present,
                connections,
                stop,
                allow_auth,
                auth_entered,
                server: Some(server),
            }
        }

        fn release_auth(&self) {
            self.allow_auth.store(true, Ordering::Release);
        }

        fn discovery(&self) -> Arc<TestDiscovery> {
            Arc::new(TestDiscovery {
                candidate: TransportCandidate::manual(self.endpoint),
                present: Arc::clone(&self.present),
                calls: AtomicUsize::new(0),
            })
        }

        fn disconnect(&self) {
            self.present.store(false, Ordering::Release);
            for stream in self
                .connections
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
            {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }

        fn reconnect(&self) {
            self.present.store(true, Ordering::Release);
        }
    }

    impl Drop for TestDevice {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            self.disconnect();
            let _ = TcpStream::connect(self.endpoint);
            if let Some(server) = self.server.take() {
                let _ = server.join();
            }
        }
    }

    fn test_timing() -> Timing {
        Timing {
            authentication_timeout: Duration::from_secs(2),
            discovery_poll: Duration::from_millis(10),
            manager_poll: Duration::from_millis(5),
            readiness_retry: Duration::from_millis(20),
            renewal_interval: Duration::from_millis(100),
            retry_override: Some(Duration::from_millis(20)),
        }
    }

    fn controller(
        runtime: Arc<SecureRuntime>,
        discovery: Arc<dyn DeviceDiscovery>,
    ) -> AutoUseController {
        AutoUseController::with_timing(runtime, discovery, test_timing()).expect("controller")
    }

    fn wait_for(
        controller: &AutoUseController,
        predicate: impl Fn(NodeStatus) -> bool,
    ) -> NodeStatus {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let status = controller.current_node_status();
            if predicate(status) {
                return status;
            }
            assert!(Instant::now() < deadline, "last status: {status:?}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_flag(flag: &AtomicBool, label: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !flag.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "{label}");
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn assert_full_resource_capacity(
        controller: &AutoUseController,
        resource_class: WireResourceClass,
    ) {
        let active = lock_data(&controller.shared)
            .active
            .clone()
            .expect("available active session");
        let access = SessionAccess {
            shared: &controller.shared,
            identity: active.identity,
            client: &active.client,
        };
        let request_bytes = resource_class.max_bytes();
        let request_count = POC_CAP_BYTES.div_ceil(request_bytes);
        let mut reservations = Vec::new();
        for _ in 0..request_count {
            let result = access
                .resource(ResourceRequest::Reserve {
                    lease_id: active.lease.lease_id,
                    worker_incarnation_id: active.lease.incarnation,
                    resource_class,
                    requested_bytes: request_bytes,
                })
                .expect("full-capacity reserve response");
            reservations.push(
                successful_reservation(&result, active.lease, resource_class, request_bytes)
                    .expect("no leaked held bytes before capacity probe"),
            );
        }
        for reservation_id in reservations {
            let release = access
                .resource(ResourceRequest::Release {
                    lease_id: active.lease.lease_id,
                    worker_incarnation_id: active.lease.incarnation,
                    reservation_id,
                })
                .expect("capacity probe release");
            assert!(cleanup_release_terminal(
                &release,
                active.lease,
                reservation_id
            ));
        }
    }

    #[test]
    fn off_never_discovers_and_enable_without_device_falls_back_locally() {
        let (_directory, runtime) = host_with_dummy_pin();
        let present = Arc::new(AtomicBool::new(false));
        let discovery = Arc::new(TestDiscovery {
            candidate: TransportCandidate::manual("127.0.0.1:9".parse().unwrap()),
            present,
            calls: AtomicUsize::new(0),
        });
        let controller = controller(runtime, discovery.clone());
        thread::sleep(Duration::from_millis(50));
        assert_eq!(discovery.calls.load(Ordering::Relaxed), 0);
        assert_eq!(controller.current_state(), AutoUseState::Off);
        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Unavailable
                && status.reason() == AutoUseReason::NoDevice
        });
        assert!(discovery.calls.load(Ordering::Relaxed) > 0);
        let input = b"local fallback";
        let result = controller.execute_blake3(input);
        assert_eq!(result.digest(), *blake3::hash(input).as_bytes());
        assert_eq!(
            result.source(),
            ExecutionSource::LocalFallbackAfterRemoteUnavailable
        );
        controller.disable();
        assert_eq!(controller.current_state(), AutoUseState::Off);
    }

    #[test]
    fn p2_gate_status_reads_are_passive_and_cannot_create_remote_work() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = TestDevice::start(android, worker.clone());
        let discovery = device.discovery();
        let controller = controller(host, discovery.clone());
        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });

        let discovery_before = discovery.calls.load(Ordering::Acquire);
        let remote_before = worker.remote_request_count.load(Ordering::Acquire);
        for _ in 0..8 {
            let gates = controller.current_gate_observations();
            assert_eq!(
                gates.discovery_observation(),
                GateObservation::new("FRESH_HINT", "C04_CANDIDATE_OBSERVED")
            );
            assert_eq!(
                gates.controller_lease(),
                GateObservation::new("ACTIVE", "C07_ACK_FRESH")
            );
            assert_eq!(
                gates.resource_guard_admission_proof(),
                GateObservation::new("FRESH_PASS", "C08_C09_C10_PROBE_PASSED")
            );
        }
        assert_eq!(discovery.calls.load(Ordering::Acquire), discovery_before);
        assert_eq!(
            worker.remote_request_count.load(Ordering::Acquire),
            remote_before
        );
    }

    #[test]
    fn p2_observations_expire_fail_closed_without_a_refresh_operation() {
        let (_directory, runtime) = host_with_dummy_pin();
        let discovery = Arc::new(TestDiscovery {
            candidate: TransportCandidate::manual("127.0.0.1:9".parse().unwrap()),
            present: Arc::new(AtomicBool::new(false)),
            calls: AtomicUsize::new(0),
        });
        let controller = controller(runtime, discovery);
        {
            let mut data = lock_data(&controller.shared);
            // Do not notify the sleeping manager: this is a pure projection test.
            data.enabled = true;
            data.discovery_observation = Some(TimedObservation {
                observation: GateObservation::new("FRESH_HINT", "C04_CANDIDATE_OBSERVED"),
                observed_at: Instant::now()
                    - DISCOVERY_OBSERVATION_MAX_AGE
                    - Duration::from_millis(1),
            });
        }
        let gates = controller.current_gate_observations();
        assert_eq!(
            gates.discovery_observation(),
            GateObservation::new("STALE", "OBSERVATION_EXPIRED")
        );
        assert_eq!(
            gates.resource_guard_admission_proof(),
            GateObservation::new("UNKNOWN", "NOT_OBSERVED")
        );
    }

    #[test]
    fn p2_discovery_observations_require_the_initiating_epoch() {
        let (_directory, runtime) = host_with_dummy_pin();
        let discovery = Arc::new(TestDiscovery {
            candidate: TransportCandidate::manual("127.0.0.1:9".parse().unwrap()),
            present: Arc::new(AtomicBool::new(false)),
            calls: AtomicUsize::new(0),
        });
        let controller = controller(runtime, discovery.clone());
        controller.shared.shutdown.store(true, Ordering::Release);
        {
            let mut data = lock_data(&controller.shared);
            data.enabled = true;
            data.epoch = 41;
            clear_observations_for_epoch(&mut data);
        }

        assert!(record_discovery_observation(
            &controller.shared,
            41,
            GateObservation::new("FRESH_HINT", "C04_CANDIDATE_OBSERVED")
        ));
        assert_eq!(
            controller
                .current_gate_observations()
                .discovery_observation(),
            GateObservation::new("FRESH_HINT", "C04_CANDIDATE_OBSERVED")
        );

        {
            let mut data = lock_data(&controller.shared);
            data.epoch = 42;
            clear_observations_for_epoch(&mut data);
        }
        assert!(!record_discovery_observation(
            &controller.shared,
            41,
            GateObservation::new("NO_HINT", "C04_NO_CANDIDATE")
        ));
        assert_eq!(
            controller
                .current_gate_observations()
                .discovery_observation(),
            GateObservation::new("UNKNOWN", "EPOCH_INVALIDATED")
        );

        controller.disable();
        assert!(!record_discovery_observation(
            &controller.shared,
            42,
            GateObservation::new("FRESH_HINT", "C04_CANDIDATE_OBSERVED")
        ));
        assert_eq!(
            controller
                .current_gate_observations()
                .discovery_observation(),
            GateObservation::new("UNKNOWN", "EPOCH_INVALIDATED")
        );
        assert_eq!(discovery.calls.load(Ordering::Acquire), 0);
    }

    #[test]
    fn p2_observability_snapshot_is_coherent_and_passive() {
        let (_directory, runtime) = host_with_dummy_pin();
        let discovery = Arc::new(TestDiscovery {
            candidate: TransportCandidate::manual("127.0.0.1:9".parse().unwrap()),
            present: Arc::new(AtomicBool::new(false)),
            calls: AtomicUsize::new(0),
        });
        let controller = controller(runtime, discovery.clone());
        let snapshot = controller.current_observability_snapshot();
        assert_eq!(snapshot.node_status().state(), AutoUseState::Off);
        assert_eq!(snapshot.node_status().reason(), AutoUseReason::Off);
        assert_eq!(
            snapshot.gate_observations().discovery_observation(),
            GateObservation::new("UNKNOWN", "EPOCH_INVALIDATED")
        );
        assert_eq!(
            snapshot.gate_observations().controller_lease(),
            GateObservation::new("UNAVAILABLE", "AUTO_USE_DISABLED")
        );
        assert_eq!(discovery.calls.load(Ordering::Acquire), 0);
    }

    #[test]
    fn p2_snapshot_evaluates_admission_proof_after_controller_data_lock() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = TestDevice::start(android, worker.clone());
        let discovery = device.discovery();
        let mut timing = test_timing();
        timing.renewal_interval = Duration::from_secs(30);
        let controller = Arc::new(
            AutoUseController::with_timing(host, discovery.clone(), timing).expect("controller"),
        );
        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });

        let discovery_before = discovery.calls.load(Ordering::Acquire);
        let remote_before = worker.remote_request_count.load(Ordering::Acquire);
        let mut data = lock_data(&controller.shared);
        data.admission_proof_observation.observed_at = Some(Instant::now());

        let reader = Arc::clone(&controller);
        let reader = thread::spawn(move || reader.current_observability_snapshot());
        thread::sleep(ADMISSION_PROOF_MAX_AGE + Duration::from_millis(50));
        drop(data);

        let snapshot = reader.join().expect("snapshot reader");
        assert_eq!(
            snapshot
                .gate_observations()
                .resource_guard_admission_proof(),
            GateObservation::new("STALE", "PROOF_EXPIRED")
        );
        assert_eq!(discovery.calls.load(Ordering::Acquire), discovery_before);
        assert_eq!(
            worker.remote_request_count.load(Ordering::Acquire),
            remote_before
        );
    }

    #[test]
    fn discovery_backend_failure_is_explicit_and_disable_stops_browsing() {
        let (_directory, runtime) = host_with_dummy_pin();
        let discovery = Arc::new(UnavailableDiscovery {
            starts: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
        });
        let controller = controller(runtime, discovery.clone());
        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Unavailable
                && status.reason() == AutoUseReason::DiscoveryBackendUnavailable
        });
        assert!(discovery.starts.load(Ordering::Relaxed) > 0);

        controller.disable();
        let deadline = Instant::now() + Duration::from_secs(2);
        while discovery.stops.load(Ordering::Relaxed) == 0 {
            assert!(Instant::now() < deadline, "discovery stop was not observed");
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(controller.current_state(), AutoUseState::Off);
    }

    #[test]
    fn wrong_pinned_key_never_reaches_available_or_mints_host_session_use() {
        let (_directory, host, actual_android) = wrong_pinned_runtimes();
        let device = TestDevice::start(actual_android, Arc::new(UnavailableHandler));
        let controller = controller(host, device.discovery());
        controller.enable();
        let failed = wait_for(&controller, |status| {
            status.state() == AutoUseState::Unavailable
                && status.reason() == AutoUseReason::AuthFailed
        });
        assert!(!failed.remote_blake3_available());
        let result = controller.execute_blake3(b"wrong pin");
        assert_eq!(
            result.source(),
            ExecutionSource::LocalFallbackAfterRemoteUnavailable
        );
    }

    #[test]
    fn automatic_ik_authority_readiness_renewal_and_remote_blake3_are_end_to_end() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = TestDevice::start(android, worker.clone());
        let controller = controller(Arc::clone(&host), device.discovery());
        controller.enable();
        let ready = wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });
        assert_eq!(ready.reason(), AutoUseReason::Ready);
        assert!(ready.peer_id().is_some());
        assert_eq!(ready.worker_incarnation(), Some(worker.incarnation()));
        assert!(ready.remote_blake3_available());
        for input in [b"x".as_slice(), b"small production payload".as_slice()] {
            let result = controller.execute_blake3(input);
            assert_eq!(result.digest(), *blake3::hash(input).as_bytes());
            assert_eq!(result.source(), ExecutionSource::RemoteSuccess);
            assert_eq!(result.reason(), AutoUseReason::Ready);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while worker.command_count(2) == 0 {
            assert!(Instant::now() < deadline, "automatic renewal not observed");
            thread::sleep(Duration::from_millis(10));
        }
        controller.disable();
        assert_eq!(controller.current_state(), AutoUseState::Off);
        let renewals_after_disable = worker.command_count(2);
        let deadline = Instant::now() + Duration::from_secs(2);
        while host.snapshot().authenticated {
            assert!(
                Instant::now() < deadline,
                "disable did not tear down session"
            );
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(Duration::from_millis(150));
        assert_eq!(worker.command_count(2), renewals_after_disable);
    }

    #[test]
    fn busy_and_resource_refusal_are_explicit_and_recover_without_false_available() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        worker.force_busy.store(true, Ordering::Release);
        let device = TestDevice::start(android, worker.clone());
        let controller = controller(host, device.discovery());
        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Unavailable
                && status.reason() == AutoUseReason::LeaseUnavailable
        });
        worker.force_busy.store(false, Ordering::Release);
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });
        worker.force_resource_refusal.store(true, Ordering::Release);
        let result = controller.execute_blake3(b"refused");
        assert_eq!(
            result.source(),
            ExecutionSource::LocalFallbackAfterRemoteUnavailable
        );
        assert_eq!(result.reason(), AutoUseReason::ResourceRefused);
        let degraded = controller.current_node_status();
        assert_eq!(degraded.state(), AutoUseState::Degraded);
        assert!(!degraded.remote_blake3_available());
        worker
            .force_resource_refusal
            .store(false, Ordering::Release);
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });
    }

    #[test]
    fn session_loss_removes_available_and_same_peer_reconnect_reuses_lease() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = TestDevice::start(android, worker.clone());
        let controller = controller(host, device.discovery());
        controller.enable();
        let first = wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });
        device.disconnect();
        wait_for(&controller, |status| {
            status.state() != AutoUseState::Available
        });
        assert!(!controller.current_node_status().remote_blake3_available());
        device.reconnect();
        let second = wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });
        assert_eq!(first.peer_id(), second.peer_id());
        assert_eq!(first.worker_incarnation(), second.worker_incarnation());
        let acquired = worker.acquire_leases();
        assert!(acquired.len() >= 2);
        assert_eq!(acquired[0], acquired[1]);
    }

    #[test]
    fn restart_uses_new_incarnation_and_never_publishes_stale_available() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = TestDevice::start(android, worker.clone());
        let controller = controller(host, device.discovery());
        controller.enable();
        let first = wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });
        device.disconnect();
        wait_for(&controller, |status| {
            status.state() != AutoUseState::Available
        });
        let restarted = worker.restart();
        assert_ne!(first.worker_incarnation(), Some(restarted));
        device.reconnect();
        let second = wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });
        assert_eq!(second.worker_incarnation(), Some(restarted));
    }

    #[test]
    fn ambiguous_pure_remote_compute_recomputes_locally_and_recovers_cleanly() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = Arc::new(TestDevice::start(android, worker.clone()));
        let controller = Arc::new(controller(host, device.discovery()));
        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });
        worker.block_compute.store(true, Ordering::Release);
        let operation_controller = Arc::clone(&controller);
        let operation = thread::spawn(move || operation_controller.execute_blake3(b"ambiguous"));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !worker.compute_entered.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "compute did not reach worker");
            thread::sleep(Duration::from_millis(5));
        }
        device.disconnect();
        worker.release_compute.store(true, Ordering::Release);
        let result = operation.join().expect("pure fallback result");
        assert_eq!(result.digest(), *blake3::hash(b"ambiguous").as_bytes());
        assert_eq!(
            result.source(),
            ExecutionSource::LocalFallbackAfterAmbiguousRemote
        );
        assert_ne!(controller.current_state(), AutoUseState::Available);
        worker.block_compute.store(false, Ordering::Release);
        device.reconnect();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });
        let recovered = controller.execute_blake3(b"after cleanup");
        assert_eq!(recovered.source(), ExecutionSource::RemoteSuccess);
        assert_eq!(
            recovered.digest(),
            *blake3::hash(b"after cleanup").as_bytes()
        );
    }

    #[test]
    fn remote_blake3_64_mib_is_exact_when_practical() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = TestDevice::start(android, worker);
        let controller = controller(host, device.discovery());
        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });
        let input = vec![0x5a; 64 * 1024 * 1024];
        let result = controller.execute_blake3(&input);
        assert_eq!(
            result.source(),
            ExecutionSource::RemoteSuccess,
            "fallback reason: {:?}",
            result.reason()
        );
        assert_eq!(result.digest(), *blake3::hash(&input).as_bytes());
    }

    #[test]
    fn disable_during_authentication_never_reaches_authoritative_remote_work() {
        let (_directory, host, android) = paired_runtimes();
        let device = TestDevice::start_auth_blocked(android, Arc::new(UnavailableHandler));
        let controller = controller(Arc::clone(&host), device.discovery());
        controller.enable();
        wait_for_flag(
            &device.auth_entered,
            "transport did not reach authentication gate",
        );
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Authenticating
        });

        controller.disable();
        device.release_auth();
        thread::sleep(Duration::from_millis(100));

        assert_eq!(controller.current_state(), AutoUseState::Off);
        assert!(!host.snapshot().authenticated);
    }

    #[test]
    fn disable_wins_during_readiness_upload_compute_and_reconnect() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = Arc::new(TestDevice::start(android, worker.clone()));
        let controller = Arc::new(controller(host, device.discovery()));

        worker.block(TestRequestStage::ResourceReserve);
        controller.enable();
        wait_for_flag(&worker.stage_entered, "readiness reserve did not enter");
        controller.disable();
        let request_count = worker.remote_request_count.load(Ordering::Acquire);
        worker.unblock();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(controller.current_state(), AutoUseState::Off);
        assert_eq!(
            worker.remote_request_count.load(Ordering::Acquire),
            request_count,
            "C08/C09/C10 request started after readiness disable won"
        );

        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });
        worker.block(TestRequestStage::RemoteBufferPut);
        let operation_controller = Arc::clone(&controller);
        let upload = thread::spawn(move || operation_controller.execute_blake3(b"disable upload"));
        wait_for_flag(&worker.stage_entered, "C09 PUT did not enter");
        controller.disable();
        let request_count = worker.remote_request_count.load(Ordering::Acquire);
        worker.unblock();
        let _ = upload.join().expect("upload fallback");
        thread::sleep(Duration::from_millis(100));
        assert_eq!(controller.current_state(), AutoUseState::Off);
        assert_eq!(
            worker.remote_request_count.load(Ordering::Acquire),
            request_count
        );

        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });
        worker.block(TestRequestStage::ComputeSubmit);
        let operation_controller = Arc::clone(&controller);
        let compute =
            thread::spawn(move || operation_controller.execute_blake3(b"disable compute"));
        wait_for_flag(&worker.stage_entered, "C10 SUBMIT did not enter");
        controller.disable();
        let request_count = worker.remote_request_count.load(Ordering::Acquire);
        worker.unblock();
        let _ = compute.join().expect("compute fallback");
        thread::sleep(Duration::from_millis(100));
        assert_eq!(controller.current_state(), AutoUseState::Off);
        assert_eq!(
            worker.remote_request_count.load(Ordering::Acquire),
            request_count
        );

        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });
        device.disconnect();
        wait_for(&controller, |status| {
            matches!(
                status.state(),
                AutoUseState::Reconnecting | AutoUseState::Unavailable
            )
        });
        controller.disable();
        let request_count = worker.remote_request_count.load(Ordering::Acquire);
        device.reconnect();
        thread::sleep(Duration::from_millis(150));
        assert_eq!(controller.current_state(), AutoUseState::Off);
        assert_eq!(
            worker.remote_request_count.load(Ordering::Acquire),
            request_count
        );
    }

    #[test]
    fn stale_session_completion_cannot_clobber_replacement_available_state() {
        let (_directory, runtime) = host_with_dummy_pin();
        let present = Arc::new(AtomicBool::new(false));
        let controller = controller(
            runtime,
            Arc::new(TestDiscovery {
                candidate: TransportCandidate::manual("127.0.0.1:9".parse().unwrap()),
                present,
                calls: AtomicUsize::new(0),
            }),
        );
        let (client, _driver) = initiator_session_channel();
        let stale = SessionIdentity {
            epoch: 1,
            serial: 1,
        };
        let replacement = SessionIdentity {
            epoch: 1,
            serial: 2,
        };
        let lease = LeaseContext {
            lease_id: [0x61; 16],
            incarnation: [0x62; 16],
            next_command_seq: 1,
            not_after: Instant::now() + Duration::from_secs(1),
        };
        let peer_id = PeerId::from_sha256_digest([0x63; 32]);
        {
            let mut data = lock_data(&controller.shared);
            data.enabled = true;
            data.epoch = 1;
            data.session_identity = Some(replacement);
            data.session_client = Some(client.clone());
            data.active = Some(ActiveContext {
                client,
                identity: replacement,
                peer_id,
                lease,
            });
            data.status = node_status(
                AutoUseState::Available,
                AutoUseReason::Ready,
                Some(peer_id),
                Some(lease.incarnation),
            );
        }

        mark_degraded_identity(
            &controller.shared,
            stale,
            peer_id,
            lease,
            AutoUseReason::ResourceRefused,
        );
        mark_connection_lost_identity(&controller.shared, stale, peer_id);

        let data = lock_data(&controller.shared);
        assert_eq!(data.status.state, AutoUseState::Available);
        assert_eq!(data.session_identity, Some(replacement));
        assert_eq!(
            data.active.as_ref().map(|active| active.identity),
            Some(replacement)
        );
    }

    #[test]
    fn ambiguous_commits_reconcile_class_one_and_class_two_accounting() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = TestDevice::start(android, worker.clone());
        let controller = controller(host, device.discovery());
        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });

        for resource_class in [
            WireResourceClass::RemoteBufferBytes,
            WireResourceClass::NativeOpScratchBytes,
        ] {
            worker.fail_next_commit(resource_class);
            let result = controller.execute_blake3(b"ambiguous commit cleanup");
            assert_eq!(
                result.source(),
                ExecutionSource::LocalFallbackAfterAmbiguousRemote
            );
            wait_for(&controller, |status| {
                status.state() == AutoUseState::Available
            });
            assert!(
                controller
                    .shared
                    .cleanup
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .entries
                    .is_empty(),
                "reconnected session retained cleanup obligation"
            );
            assert_full_resource_capacity(&controller, resource_class);
        }
    }

    #[test]
    fn ambiguous_reserve_replay_recovers_exact_id_and_restores_both_class_budgets() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = TestDevice::start(android, worker.clone());
        let controller = controller(host, device.discovery());
        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });

        for resource_class in [
            WireResourceClass::RemoteBufferBytes,
            WireResourceClass::NativeOpScratchBytes,
        ] {
            let before = worker.reserve_request_ids(resource_class).len();
            worker.fail_reserve_responses(resource_class, ReserveFailureTiming::AfterExecution, 1);
            let result = controller.execute_blake3(b"lost reserve response");
            assert_eq!(
                result.source(),
                ExecutionSource::LocalFallbackAfterAmbiguousRemote
            );
            wait_for(&controller, |status| {
                status.state() == AutoUseState::Available
            });

            let attempts = worker.reserve_request_ids(resource_class);
            assert!(attempts.len() >= before + 2);
            let original_request_id = attempts[before];
            assert_eq!(attempts[before + 1], original_request_id);
            let recovered = worker
                .reservation_for_request(original_request_id)
                .expect("worker executed original RESERVE");
            assert_eq!(worker.release_count(recovered), 1);
            assert!(
                controller
                    .shared
                    .cleanup
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .entries
                    .is_empty()
            );
            assert_full_resource_capacity(&controller, resource_class);
        }
    }

    #[test]
    fn lease_rotation_retains_cleanup_until_valid_c08_purge_proof_for_both_classes() {
        for resource_class in [
            WireResourceClass::RemoteBufferBytes,
            WireResourceClass::NativeOpScratchBytes,
        ] {
            let (_directory, host, android) = paired_runtimes();
            let worker = TestWorker::healthy();
            let device = TestDevice::start(android, worker.clone());
            let mut timing = test_timing();
            timing.renewal_interval = Duration::from_secs(30);
            let controller = AutoUseController::with_timing(host, device.discovery(), timing)
                .expect("controller");
            controller.enable();
            wait_for(&controller, |status| {
                status.state() == AutoUseState::Available
            });

            let active = lock_data(&controller.shared)
                .active
                .clone()
                .expect("initial active authority");
            let access = SessionAccess {
                shared: &controller.shared,
                identity: active.identity,
                client: &active.client,
            };
            let bytes = resource_class.max_bytes();
            let reservation =
                reserve_with_cleanup(&access, active.peer_id, active.lease, resource_class, bytes)
                    .expect("old-lease reservation");
            let commit = access
                .resource(ResourceRequest::Commit {
                    lease_id: active.lease.lease_id,
                    worker_incarnation_id: active.lease.incarnation,
                    reservation_id: reservation,
                })
                .expect("old-lease commit response");
            assert_eq!(commit.state, ResourceResultState::Completed);
            assert_eq!(commit.reason, ResourceReason::None);
            assert_eq!(
                commit.reservation.map(|entry| entry.state),
                Some(WireReservationState::Committed)
            );
            mark_cleanup_ambiguous(
                &controller.shared,
                CleanupTarget::Reservation(reservation),
                active.peer_id,
                active.lease,
            );

            worker.block(TestRequestStage::ResourceReserve);
            release_lease(&access, active.lease).expect("replace old C07 lease");
            active.client.cancel_session();
            wait_for_flag(
                &worker.stage_entered,
                "replacement-lease C08 purge proof did not enter",
            );

            let replacement_lease = *worker
                .acquire_leases()
                .last()
                .expect("replacement lease acquired");
            assert_ne!(replacement_lease, active.lease.lease_id);
            {
                let ledger = controller
                    .shared
                    .cleanup
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let retained = ledger
                    .entries
                    .iter()
                    .find(|entry| entry.target == CleanupTarget::Reservation(reservation))
                    .copied()
                    .expect("old-lease obligation remains alongside the in-flight probe");
                assert_eq!(retained.lease_id, active.lease.lease_id);
                assert_eq!(retained.incarnation, active.lease.incarnation);
                assert_eq!(
                    retained.knowledge,
                    CleanupKnowledge::AwaitingResourceGuardPurgeProof
                );
            }

            // The replacement lease exists, but the C08 call is still blocked
            // before WorkerCore. Loss here must not erase the old obligation.
            device.disconnect();
            worker.unblock();
            wait_for(&controller, |status| {
                matches!(
                    status.state(),
                    AutoUseState::Reconnecting | AutoUseState::Unavailable
                )
            });
            {
                let ledger = controller
                    .shared
                    .cleanup
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let retained = ledger
                    .entries
                    .iter()
                    .find(|entry| entry.target == CleanupTarget::Reservation(reservation))
                    .expect("disconnect cannot erase the old-lease obligation");
                assert_eq!(
                    retained.knowledge,
                    CleanupKnowledge::AwaitingResourceGuardPurgeProof
                );
            }

            device.reconnect();
            wait_for(&controller, |status| {
                status.state() == AutoUseState::Available
                    && status.worker_incarnation() == Some(active.lease.incarnation)
            });
            assert!(
                controller
                    .shared
                    .cleanup
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .entries
                    .is_empty(),
                "old lease obligation survived a completed C08 purge proof"
            );
            assert_full_resource_capacity(&controller, resource_class);
        }
    }

    #[test]
    fn reserve_absence_proof_allowlist_rejects_stale_conflict_and_internal_failures() {
        let lease = LeaseContext {
            lease_id: [0x31; 16],
            incarnation: [0x42; 16],
            next_command_seq: 0,
            not_after: Instant::now() + Duration::from_secs(1),
        };
        let failed = |reason| pb_pbmux::ResourceResult {
            state: ResourceResultState::Failed,
            reason,
            lease_id: lease.lease_id,
            worker_incarnation_id: lease.incarnation,
            reservation: None,
        };

        for reason in [
            ResourceReason::StaleControllerLease,
            ResourceReason::RequestIdConflict,
            ResourceReason::InternalError,
            ResourceReason::IdempotenceTableFull,
            ResourceReason::UnsupportedMessage,
        ] {
            assert!(
                !reserve_refusal_proves_absence(&failed(reason), lease),
                "{reason:?} must retain the unresolved obligation"
            );
        }
        for reason in [
            ResourceReason::RefusedStaleState,
            ResourceReason::ResourceExhausted,
        ] {
            assert!(
                reserve_refusal_proves_absence(&failed(reason), lease),
                "{reason:?} is a typed Reserve refusal that creates no reservation"
            );
        }
    }

    #[test]
    fn ambiguous_unexecuted_reserve_replay_creates_then_releases() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = TestDevice::start(android, worker.clone());
        let controller = controller(host, device.discovery());
        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });

        let resource_class = WireResourceClass::RemoteBufferBytes;
        let before = worker.reserve_request_ids(resource_class).len();
        worker.fail_reserve_responses(resource_class, ReserveFailureTiming::BeforeExecution, 1);
        let result = controller.execute_blake3(b"reserve not dispatched");
        assert_eq!(
            result.source(),
            ExecutionSource::LocalFallbackAfterAmbiguousRemote
        );
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });

        let attempts = worker.reserve_request_ids(resource_class);
        assert!(attempts.len() >= before + 2);
        let original_request_id = attempts[before];
        assert_eq!(attempts[before + 1], original_request_id);
        let created_on_replay = worker
            .reservation_for_request(original_request_id)
            .expect("replay created the original idempotent reservation");
        assert_eq!(worker.release_count(created_on_replay), 1);
        assert_full_resource_capacity(&controller, resource_class);
    }

    #[test]
    fn second_reserve_replay_loss_keeps_obligation_until_next_reconnect() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = TestDevice::start(android, worker.clone());
        let controller = controller(host, device.discovery());
        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });

        let resource_class = WireResourceClass::NativeOpScratchBytes;
        let before = worker.reserve_request_ids(resource_class).len();
        worker.fail_reserve_responses(resource_class, ReserveFailureTiming::AfterExecution, 2);
        let result = controller.execute_blake3(b"two lost reserve responses");
        assert_eq!(
            result.source(),
            ExecutionSource::LocalFallbackAfterAmbiguousRemote
        );
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });

        let attempts = worker.reserve_request_ids(resource_class);
        assert!(attempts.len() >= before + 3);
        assert_eq!(attempts[before], attempts[before + 1]);
        assert_eq!(attempts[before], attempts[before + 2]);
        let reservation = worker
            .reservation_for_request(attempts[before])
            .expect("first execution returned a reservation internally");
        assert_eq!(worker.release_count(reservation), 1);
        assert!(
            controller
                .shared
                .cleanup
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .entries
                .is_empty()
        );
        assert_full_resource_capacity(&controller, resource_class);
    }

    #[test]
    fn reserve_obligation_drops_only_after_worker_incarnation_rotation() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = TestDevice::start(android, worker.clone());
        let controller = controller(host, device.discovery());
        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });

        worker.fail_reserve_responses(
            WireResourceClass::RemoteBufferBytes,
            ReserveFailureTiming::AfterExecution,
            1,
        );
        device.present.store(false, Ordering::Release);
        let result = controller.execute_blake3(b"restart unresolved reserve");
        assert_eq!(
            result.source(),
            ExecutionSource::LocalFallbackAfterAmbiguousRemote
        );
        assert_eq!(
            controller
                .shared
                .cleanup
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .entries
                .len(),
            1
        );
        let old_incarnation = worker.incarnation();
        let new_incarnation = worker.restart();
        assert_ne!(old_incarnation, new_incarnation);
        device.reconnect();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
                && status.worker_incarnation() == Some(new_incarnation)
        });
        assert!(
            controller
                .shared
                .cleanup
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .entries
                .is_empty()
        );
        assert_full_resource_capacity(&controller, WireResourceClass::RemoteBufferBytes);
    }

    #[test]
    fn full_cleanup_ledger_refuses_new_remote_admission_without_eviction() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = TestDevice::start(android, worker.clone());
        let controller = controller(host, device.discovery());
        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });
        let active = lock_data(&controller.shared)
            .active
            .clone()
            .expect("active session");
        {
            let mut ledger = controller
                .shared
                .cleanup
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for request_id in 1..=MAX_CLEANUP_OBLIGATIONS as u64 {
                ledger.entries.push(CleanupObligation {
                    peer_id: active.peer_id,
                    lease_id: active.lease.lease_id,
                    incarnation: active.lease.incarnation,
                    origin: active.identity,
                    target: CleanupTarget::UnresolvedReserve {
                        request_id,
                        resource_class: WireResourceClass::RemoteBufferBytes,
                        requested_bytes: 1,
                    },
                    knowledge: CleanupKnowledge::Ambiguous,
                });
            }
        }
        let before = worker.remote_request_count.load(Ordering::Acquire);
        let result = controller.execute_blake3(b"ledger full");
        assert_eq!(
            result.source(),
            ExecutionSource::LocalFallbackAfterRemoteUnavailable
        );
        assert_eq!(result.reason(), AutoUseReason::ResourceRefused);
        thread::sleep(Duration::from_millis(50));
        assert_eq!(worker.remote_request_count.load(Ordering::Acquire), before);
        assert_eq!(
            controller
                .shared
                .cleanup
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .entries
                .len(),
            MAX_CLEANUP_OBLIGATIONS
        );
    }

    #[test]
    fn incarnation_restart_invalidates_old_cleanup_without_leak() {
        let (_directory, host, android) = paired_runtimes();
        let worker = TestWorker::healthy();
        let device = TestDevice::start(android, worker.clone());
        let controller = controller(host, device.discovery());
        controller.enable();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
        });

        worker.fail_next_commit(WireResourceClass::RemoteBufferBytes);
        device.present.store(false, Ordering::Release);
        let result = controller.execute_blake3(b"restart cleanup");
        assert_eq!(
            result.source(),
            ExecutionSource::LocalFallbackAfterAmbiguousRemote
        );
        wait_for(&controller, |status| {
            status.state() != AutoUseState::Available
        });
        let old_incarnation = worker.incarnation();
        let new_incarnation = worker.restart();
        assert_ne!(old_incarnation, new_incarnation);
        device.reconnect();
        wait_for(&controller, |status| {
            status.state() == AutoUseState::Available
                && status.worker_incarnation() == Some(new_incarnation)
        });
        assert!(
            controller
                .shared
                .cleanup
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .entries
                .is_empty()
        );
        assert_full_resource_capacity(&controller, WireResourceClass::RemoteBufferBytes);
    }
}
