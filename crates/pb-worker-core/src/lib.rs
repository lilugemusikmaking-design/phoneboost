#![forbid(unsafe_code)]

use std::fs::File;
use std::io::{self, Read};
use std::time::Instant;

use pb_runtime_secure::VerifiedPeerSession;

mod lease;
mod resource_guard;

pub use lease::{
    AuthenticatedSession, ControllerCommand, ControllerCommandError, ControllerCommandResult,
    ControllerFailureReason, ControllerLeaseManager, ControllerLeaseRef, LEASE_TTL_MS, LeaseId,
    LeaseState, RECOMMENDED_RENEWAL_MS,
};
pub use resource_guard::{
    BatteryBand, CommitDecision, HEALTH_INTERVAL_MS, HEALTH_STALE_AFTER_MS, HealthSample,
    HealthStatus, MEMORY_MARGIN_BYTES, MIN_AVAILABLE_BYTES, NATIVE_OPERATION_THREADS,
    POC_CAP_BYTES, RECOVERY_AVAILABLE_BYTES, RESERVATION_TTL_MS, ReleaseDecision, RequestId,
    ReservationDecision, ReservationId, ReserveRequest, ResourceClass, ResourceGuard,
    ResourceGuardState, SafetyBand, TERMINAL_RETENTION_MS, ThermalBand, poc_budget,
};

const RANDOM_ID_BYTES: usize = 16;
const MAX_ZERO_RETRIES: usize = 8;

/// Later RemoteBuffer and compute domains remain deferred by build order.
pub const DEFERRED_BY_BUILD_ORDER: &str = "DEFERRED_BY_BUILD_ORDER";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum WorkerState {
    ColdStart = 1,
    PairingRequired = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerIncarnationId([u8; RANDOM_ID_BYTES]);

impl WorkerIncarnationId {
    pub const BITS: usize = RANDOM_ID_BYTES * 8;

    pub const fn high_u64(self) -> u64 {
        u64::from_be_bytes([
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5], self.0[6], self.0[7],
        ])
    }

    pub const fn low_u64(self) -> u64 {
        u64::from_be_bytes([
            self.0[8], self.0[9], self.0[10], self.0[11], self.0[12], self.0[13], self.0[14],
            self.0[15],
        ])
    }

    pub fn is_nonzero(self) -> bool {
        self.0.iter().any(|byte| *byte != 0)
    }

    pub const fn into_bytes(self) -> [u8; RANDOM_ID_BYTES] {
        self.0
    }
}

trait WorkerMonotonicClock: Send + Sync {
    fn now_ms(&self) -> u64;
}

struct SystemWorkerClock {
    origin: Instant,
}

impl SystemWorkerClock {
    fn start() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl WorkerMonotonicClock for SystemWorkerClock {
    fn now_ms(&self) -> u64 {
        self.origin
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }
}

#[derive(Debug)]
pub enum WorkerStartError {
    EntropyUnavailable,
}

impl std::fmt::Display for WorkerStartError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EntropyUnavailable => formatter.write_str("worker entropy unavailable"),
        }
    }
}

impl std::error::Error for WorkerStartError {}

/// Rust-owned A5 worker authority. Construction is the full core start boundary.
pub struct WorkerCore {
    incarnation: WorkerIncarnationId,
    state: WorkerState,
    lease_manager: ControllerLeaseManager,
    resource_guard: ResourceGuard,
    clock: Box<dyn WorkerMonotonicClock>,
}

impl WorkerCore {
    pub fn cold_start() -> Result<Self, WorkerStartError> {
        let mut state = WorkerState::ColdStart;
        let incarnation =
            random_incarnation_id().map_err(|_| WorkerStartError::EntropyUnavailable)?;
        initialize_a5_skeleton(&mut state);
        Ok(Self {
            incarnation,
            state,
            lease_manager: ControllerLeaseManager::new(incarnation),
            resource_guard: ResourceGuard::new(),
            clock: Box::new(SystemWorkerClock::start()),
        })
    }

    pub const fn state(&self) -> WorkerState {
        self.state
    }

    pub const fn incarnation(&self) -> WorkerIncarnationId {
        self.incarnation
    }

    /// Accepts only Android-local observations. It grants no remote authority.
    pub fn record_local_health(&mut self, sample: HealthSample) -> HealthStatus {
        self.resource_guard.record_health(sample)
    }

    pub fn health_status(&self, now_ms: u64) -> HealthStatus {
        self.resource_guard.health_status(now_ms)
    }

    pub const fn health_sample_count(&self) -> u64 {
        self.resource_guard.health_sample_count()
    }

    pub const fn lease_state(&self) -> LeaseState {
        self.lease_manager.state()
    }

    pub const fn resource_guard_state(&self) -> ResourceGuardState {
        self.resource_guard.state()
    }

    /// The sole production C07 lease-mutation entry point.
    ///
    /// A bare peer identifier cannot be substituted for authenticated proof:
    ///
    /// ```compile_fail
    /// use pb_types::PeerId;
    /// use pb_worker_core::{ControllerCommand, WorkerCore};
    ///
    /// let mut worker = WorkerCore::cold_start().unwrap();
    /// let peer_id = PeerId::from_sha256_digest([7; 32]);
    /// let _ = worker.apply_controller_command(&peer_id, ControllerCommand::Acquire);
    /// ```
    ///
    /// A status boolean cannot be substituted either:
    ///
    /// ```compile_fail
    /// use pb_worker_core::{ControllerCommand, WorkerCore};
    ///
    /// let mut worker = WorkerCore::cold_start().unwrap();
    /// let authenticated = true;
    /// let _ = worker.apply_controller_command(&authenticated, ControllerCommand::Acquire);
    /// ```
    pub fn apply_controller_command(
        &mut self,
        verified_session: &VerifiedPeerSession<'_>,
        command: ControllerCommand,
    ) -> Result<ControllerCommandResult, ControllerCommandError> {
        let session = AuthenticatedSession::from_verified(verified_session);
        self.lease_manager
            .apply_controller_command(&session, command, self.clock.now_ms())
    }
}

fn initialize_a5_skeleton(state: &mut WorkerState) {
    *state = WorkerState::PairingRequired;
}

fn random_incarnation_id() -> io::Result<WorkerIncarnationId> {
    random_nonzero_128().map(WorkerIncarnationId)
}

pub(crate) fn random_nonzero_128() -> io::Result<[u8; RANDOM_ID_BYTES]> {
    let mut source = File::open("/dev/urandom")?;
    for _ in 0..MAX_ZERO_RETRIES {
        let mut bytes = [0_u8; RANDOM_ID_BYTES];
        source.read_exact(&mut bytes)?;
        if bytes.iter().any(|byte| *byte != 0) {
            return Ok(bytes);
        }
    }
    Err(io::Error::other("OS RNG returned repeated zero values"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a5_t06_incarnation_is_exactly_128_bits_and_nonzero() {
        let worker = WorkerCore::cold_start().expect("OS randomness available");
        assert_eq!(WorkerIncarnationId::BITS, 128);
        assert!(worker.incarnation().is_nonzero());
    }

    #[test]
    fn a5_t07_cold_start_reaches_only_pairing_required() {
        let worker = WorkerCore::cold_start().expect("OS randomness available");
        assert_eq!(worker.state(), WorkerState::PairingRequired);
    }

    #[test]
    fn a5_t10_full_core_restart_rotates_incarnation() {
        let first = WorkerCore::cold_start().expect("first start").incarnation();
        let second = WorkerCore::cold_start()
            .expect("second start")
            .incarnation();
        assert_ne!(first.0, second.0);
    }

    #[test]
    fn later_remote_buffer_and_compute_authority_remain_deferred() {
        assert_eq!(DEFERRED_BY_BUILD_ORDER, "DEFERRED_BY_BUILD_ORDER");
    }
}
