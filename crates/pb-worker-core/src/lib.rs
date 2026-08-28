#![forbid(unsafe_code)]

use std::fs::File;
use std::io::{self, Read};
use std::time::Instant;

use pb_pbmux::{
    BLAKE3_PROVIDER_ID, BLAKE3_PROVIDER_VERSION, BufferReason, BufferResult, ComputeReason,
    ComputeRequest, ComputeResponse, ComputeSubmit, MAX_COMPUTE_INPUT_BYTES,
    REMOTE_BUFFER_INPUT_KIND, RemoteBufferRequest, RemoteBufferResponseKind, ReservationResultRef,
    ResourceReason, ResourceRequest, ResourceResponseKind, ResourceResult, ResourceResultState,
    WireResourceClass,
};
use pb_runtime_secure::VerifiedPeerSession;

mod compute;
mod lease;
mod remote_buffer;
mod resource_guard;

pub use lease::{
    AuthenticatedSession, ControllerCommand, ControllerCommandError, ControllerCommandResult,
    ControllerFailureReason, ControllerLeaseManager, ControllerLeaseRef, LEASE_TTL_MS, LeaseId,
    LeaseState, RECOMMENDED_RENEWAL_MS,
};
pub use remote_buffer::{
    DEFAULT_BUFFER_TTL_MS, MAX_BUFFER_LIFETIME_MS, MAX_BUFFERS_PER_LEASE, MAX_REMOTE_BUFFER_BYTES,
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
    remote_buffers: remote_buffer::RemoteBufferStore,
    compute_jobs: compute::ComputeJobStore,
    resource_requests: std::collections::BTreeMap<(LeaseId, u64), ResourceRequestCacheEntry>,
    clock: Box<dyn WorkerMonotonicClock>,
}

#[derive(Clone)]
struct ResourceRequestCacheEntry {
    request: ResourceRequest,
    kind: ResourceResponseKind,
    result: ResourceResult,
    terminal_at_ms: u64,
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
            remote_buffers: remote_buffer::RemoteBufferStore::new(),
            compute_jobs: compute::ComputeJobStore::new(),
            resource_requests: std::collections::BTreeMap::new(),
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
    pub fn record_local_health(&mut self, mut sample: HealthSample) -> HealthStatus {
        // Resource authority and provider TTLs share one worker-owned monotonic
        // domain; JNI-supplied timestamps are observations, not clock authority.
        let now_ms = self.clock.now_ms();
        sample.monotonic_ms = now_ms;
        let status = self.resource_guard.record_health(sample);
        self.resource_guard.tick(now_ms);
        self.remote_buffers.tick(&mut self.resource_guard, now_ms);
        status
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
        let now_ms = self.clock.now_ms();
        let released_lease = match command {
            ControllerCommand::Release { lease_id, .. } => Some(lease_id),
            _ => None,
        };
        let result = self
            .lease_manager
            .apply_controller_command(&session, command, now_ms)?;
        if matches!(
            result,
            ControllerCommandResult::Completed { lease: None, .. }
        ) && let Some(lease_id) = released_lease
        {
            self.compute_jobs
                .lease_ended(&mut self.resource_guard, lease_id, now_ms);
        }
        Ok(result)
    }

    /// Production C08 authority entry point. Authentication proof, active C07
    /// lease, and the request's current incarnation are all required.
    pub fn apply_resource_request(
        &mut self,
        verified_session: &VerifiedPeerSession<'_>,
        request_id: u64,
        request: ResourceRequest,
    ) -> (ResourceResponseKind, ResourceResult) {
        let now_ms = self.clock.now_ms();
        let kind = resource_response_kind(&request);
        let lease_id = LeaseId::from_bytes(*request.lease_id());
        let wire_lease_id = *request.lease_id();
        let session = AuthenticatedSession::from_verified(verified_session);
        let proof = if request_id == 0
            || request.worker_incarnation_id() != &self.incarnation.into_bytes()
        {
            None
        } else {
            self.lease_manager.validate(&session, lease_id, now_ms).ok()
        };
        let Some(proof) = proof else {
            return (
                kind,
                resource_failure(
                    wire_lease_id,
                    self.incarnation,
                    ResourceReason::StaleControllerLease,
                ),
            );
        };
        self.resource_requests.retain(|(cached_lease, _), entry| {
            *cached_lease == lease_id
                && now_ms.saturating_sub(entry.terminal_at_ms) <= TERMINAL_RETENTION_MS
        });
        let key = (lease_id, request_id);
        if let Some(entry) = self.resource_requests.get(&key) {
            return if entry.request == request {
                (entry.kind, entry.result)
            } else {
                (
                    kind,
                    resource_failure(
                        wire_lease_id,
                        self.incarnation,
                        ResourceReason::RequestIdConflict,
                    ),
                )
            };
        }
        if self
            .resource_requests
            .keys()
            .filter(|(cached_lease, _)| *cached_lease == lease_id)
            .count()
            >= 1_024
        {
            return (
                kind,
                resource_failure(
                    wire_lease_id,
                    self.incarnation,
                    ResourceReason::IdempotenceTableFull,
                ),
            );
        }
        let result = match request.clone() {
            ResourceRequest::Reserve {
                resource_class,
                requested_bytes,
                ..
            } => {
                let decision = self.resource_guard.reserve(
                    &mut self.lease_manager,
                    &session,
                    lease_id,
                    ReserveRequest {
                        request_id: RequestId::from_u64(request_id),
                        class: match resource_class {
                            WireResourceClass::RemoteBufferBytes => {
                                ResourceClass::RemoteBufferBytes
                            }
                            WireResourceClass::NativeOpScratchBytes => {
                                ResourceClass::NativeOpScratchBytes
                            }
                        },
                        bytes: requested_bytes,
                    },
                    now_ms,
                );
                match decision {
                    ReservationDecision::Reserved { reservation_id } => resource_success(
                        wire_lease_id,
                        self.incarnation,
                        match self.resource_guard.reservation_snapshot(
                            proof,
                            reservation_id,
                            now_ms,
                        ) {
                            Some(snapshot) => snapshot,
                            None => {
                                return (
                                    kind,
                                    resource_failure(
                                        wire_lease_id,
                                        self.incarnation,
                                        ResourceReason::InternalError,
                                    ),
                                );
                            }
                        },
                    ),
                    ReservationDecision::RefusedSafety => resource_failure(
                        wire_lease_id,
                        self.incarnation,
                        if self.resource_guard.health_status(now_ms).safety
                            == SafetyBand::RefusedStaleState
                        {
                            ResourceReason::RefusedStaleState
                        } else {
                            ResourceReason::ResourceExhausted
                        },
                    ),
                    ReservationDecision::RefusedBudget => resource_failure(
                        wire_lease_id,
                        self.incarnation,
                        ResourceReason::ResourceExhausted,
                    ),
                    ReservationDecision::UnknownResourceClass => resource_failure(
                        wire_lease_id,
                        self.incarnation,
                        ResourceReason::UnsupportedMessage,
                    ),
                    ReservationDecision::StaleLease => resource_failure(
                        wire_lease_id,
                        self.incarnation,
                        ResourceReason::StaleControllerLease,
                    ),
                    ReservationDecision::IdempotenceConflict => resource_failure(
                        wire_lease_id,
                        self.incarnation,
                        ResourceReason::RequestIdConflict,
                    ),
                    ReservationDecision::IdempotenceTableFull => resource_failure(
                        wire_lease_id,
                        self.incarnation,
                        ResourceReason::IdempotenceTableFull,
                    ),
                    ReservationDecision::EntropyUnavailable => resource_failure(
                        wire_lease_id,
                        self.incarnation,
                        ResourceReason::InternalError,
                    ),
                }
            }
            ResourceRequest::Commit { reservation_id, .. } => {
                let reservation_id = ReservationId::from_bytes(reservation_id);
                let decision = self.resource_guard.commit(
                    &mut self.lease_manager,
                    &session,
                    lease_id,
                    reservation_id,
                    now_ms,
                );
                let snapshot =
                    self.resource_guard
                        .reservation_snapshot(proof, reservation_id, now_ms);
                match decision {
                    CommitDecision::Committed => match snapshot {
                        Some(snapshot) => {
                            resource_success(wire_lease_id, self.incarnation, snapshot)
                        }
                        None => resource_failure(
                            wire_lease_id,
                            self.incarnation,
                            ResourceReason::InternalError,
                        ),
                    },
                    CommitDecision::RefusedSafety => resource_failure_with_snapshot(
                        wire_lease_id,
                        self.incarnation,
                        ResourceReason::CommitRefusedSafety,
                        snapshot,
                    ),
                    CommitDecision::StaleLease => resource_failure(
                        wire_lease_id,
                        self.incarnation,
                        ResourceReason::StaleControllerLease,
                    ),
                    CommitDecision::UnknownOrExpiredReservation => {
                        let reason = snapshot.map_or(
                            ResourceReason::ReservationNotFound,
                            |value| match value.wire_state() {
                                pb_pbmux::WireReservationState::Expired => {
                                    ResourceReason::ReservationExpired
                                }
                                pb_pbmux::WireReservationState::Consumed
                                | pb_pbmux::WireReservationState::ConsumedReleased => {
                                    ResourceReason::ReservationAlreadyConsumed
                                }
                                _ => ResourceReason::ReservationNotCommitted,
                            },
                        );
                        resource_failure_with_snapshot(
                            wire_lease_id,
                            self.incarnation,
                            reason,
                            snapshot,
                        )
                    }
                }
            }
            ResourceRequest::Release { reservation_id, .. } => {
                let reservation_id = ReservationId::from_bytes(reservation_id);
                let decision = self.resource_guard.release(
                    &mut self.lease_manager,
                    &session,
                    lease_id,
                    reservation_id,
                    now_ms,
                );
                let snapshot =
                    self.resource_guard
                        .reservation_snapshot(proof, reservation_id, now_ms);
                match decision {
                    ReleaseDecision::Released => match snapshot {
                        Some(snapshot) => {
                            resource_success(wire_lease_id, self.incarnation, snapshot)
                        }
                        None => resource_failure(
                            wire_lease_id,
                            self.incarnation,
                            ResourceReason::InternalError,
                        ),
                    },
                    ReleaseDecision::AlreadyTerminal => {
                        let Some(snapshot) = snapshot else {
                            return (
                                kind,
                                resource_failure(
                                    wire_lease_id,
                                    self.incarnation,
                                    ResourceReason::ReservationNotFound,
                                ),
                            );
                        };
                        match snapshot.wire_state() {
                            pb_pbmux::WireReservationState::Released => {
                                resource_success(wire_lease_id, self.incarnation, snapshot)
                            }
                            pb_pbmux::WireReservationState::Consumed
                            | pb_pbmux::WireReservationState::ConsumedReleased => {
                                resource_failure_with_snapshot(
                                    wire_lease_id,
                                    self.incarnation,
                                    ResourceReason::ReservationAlreadyConsumed,
                                    Some(snapshot),
                                )
                            }
                            pb_pbmux::WireReservationState::Expired => {
                                resource_failure_with_snapshot(
                                    wire_lease_id,
                                    self.incarnation,
                                    ResourceReason::ReservationExpired,
                                    Some(snapshot),
                                )
                            }
                            _ => resource_failure_with_snapshot(
                                wire_lease_id,
                                self.incarnation,
                                ResourceReason::ReservationNotCommitted,
                                Some(snapshot),
                            ),
                        }
                    }
                    ReleaseDecision::StaleLease => resource_failure(
                        wire_lease_id,
                        self.incarnation,
                        ResourceReason::StaleControllerLease,
                    ),
                    ReleaseDecision::UnknownReservation => resource_failure(
                        wire_lease_id,
                        self.incarnation,
                        ResourceReason::ReservationNotFound,
                    ),
                }
            }
        };
        self.resource_requests.insert(
            key,
            ResourceRequestCacheEntry {
                request,
                kind,
                result,
                terminal_at_ms: now_ms,
            },
        );
        (kind, result)
    }

    /// Production C09 authority entry point. Bare peer IDs, handles, and
    /// authentication booleans cannot call into the store.
    pub fn apply_remote_buffer_request(
        &mut self,
        verified_session: &VerifiedPeerSession<'_>,
        request: RemoteBufferRequest,
    ) -> (RemoteBufferResponseKind, BufferResult) {
        let now_ms = self.clock.now_ms();
        let kind = remote_buffer_response_kind(&request);
        let wire_lease_id = *request.lease_id();
        if request.worker_incarnation_id() != &self.incarnation.into_bytes() {
            return (
                kind,
                remote_buffer_failure(
                    wire_lease_id,
                    self.incarnation,
                    BufferReason::BufferWrongIncarnation,
                ),
            );
        }
        let lease_id = LeaseId::from_bytes(wire_lease_id);
        let session = AuthenticatedSession::from_verified(verified_session);
        let proof = match self.lease_manager.validate(&session, lease_id, now_ms) {
            Ok(proof) => proof,
            Err(_) => {
                return (
                    kind,
                    remote_buffer_failure(
                        wire_lease_id,
                        self.incarnation,
                        BufferReason::StaleControllerLease,
                    ),
                );
            }
        };
        self.remote_buffers.apply(
            &mut self.resource_guard,
            proof,
            remote_buffer::SessionBinding::from_verified(verified_session.session_id()),
            request,
            now_ms,
        )
    }

    /// Production C10 authority entry point. The verified session, active C07
    /// lease, current incarnation, class-2 reservation, and same-session READY
    /// RemoteBuffer are validated before a job becomes visible.
    pub fn apply_compute_request(
        &mut self,
        verified_session: &VerifiedPeerSession<'_>,
        request_id: u64,
        request: ComputeRequest,
    ) -> ComputeResponse {
        let now_ms = self.clock.now_ms();
        if request.worker_incarnation_id() != &self.incarnation.into_bytes() {
            return compute_request_failure(
                request,
                self.incarnation,
                ComputeReason::WrongWorkerIncarnation,
            );
        }
        let lease_id = LeaseId::from_bytes(*request.lease_id());
        let session = AuthenticatedSession::from_verified(verified_session);
        let proof = match self.lease_manager.validate(&session, lease_id, now_ms) {
            Ok(proof) if request_id != 0 => proof,
            _ => {
                return compute_request_failure(
                    request,
                    self.incarnation,
                    ComputeReason::StaleControllerLease,
                );
            }
        };
        let session_id =
            remote_buffer::SessionBinding::from_verified(verified_session.session_id());
        match request {
            ComputeRequest::Submit(submit) => {
                self.apply_compute_submit(proof, session_id, request_id, submit, now_ms)
            }
            ComputeRequest::Status(status) => {
                self.compute_jobs.status(proof, session_id, status, now_ms)
            }
            ComputeRequest::Cancel(cancel) => self.compute_jobs.cancel(
                &mut self.resource_guard,
                proof,
                session_id,
                request_id,
                cancel,
                now_ms,
            ),
        }
    }

    fn apply_compute_submit(
        &mut self,
        proof: lease::LeaseProof,
        session_id: remote_buffer::SessionBinding,
        request_id: u64,
        submit: ComputeSubmit,
        now_ms: u64,
    ) -> ComputeResponse {
        match self
            .compute_jobs
            .check_submit(proof.lease_id, request_id, session_id, submit, now_ms)
        {
            compute::SubmitLookup::Replay(response) => return response,
            compute::SubmitLookup::Conflict => {
                return compute::absent_result(
                    submit,
                    self.incarnation,
                    ComputeReason::RequestIdConflict,
                );
            }
            compute::SubmitLookup::Full => {
                return compute::absent_result(
                    submit,
                    self.incarnation,
                    ComputeReason::IdempotenceTableFull,
                );
            }
            compute::SubmitLookup::Vacant => {}
        }

        let refusal = if (submit.provider_id, submit.provider_version)
            != (BLAKE3_PROVIDER_ID, BLAKE3_PROVIDER_VERSION)
        {
            Some(ComputeReason::UnsupportedProvider)
        } else if submit.input_kind != REMOTE_BUFFER_INPUT_KIND {
            Some(ComputeReason::InvalidInput)
        } else if submit.input_length > MAX_COMPUTE_INPUT_BYTES
            || submit
                .input_offset
                .checked_add(submit.input_length)
                .is_none()
        {
            Some(ComputeReason::InputTooLarge)
        } else {
            self.remote_buffers
                .validate_compute_input(
                    &mut self.resource_guard,
                    proof,
                    session_id,
                    submit.buffer_id,
                    submit.input_offset,
                    submit.input_length,
                    now_ms,
                )
                .err()
        };
        if let Some(reason) = refusal {
            return self.cache_compute_refusal(
                proof.lease_id,
                session_id,
                request_id,
                submit,
                reason,
                now_ms,
            );
        }

        let reservation_id = ReservationId::from_bytes(submit.reservation_id);
        let scratch_bytes =
            match self
                .resource_guard
                .reservation_snapshot(proof, reservation_id, now_ms)
            {
                Some(snapshot)
                    if snapshot.class == ResourceClass::NativeOpScratchBytes
                        && snapshot.bytes > 0
                        && snapshot.bytes <= pb_pbmux::NATIVE_OP_SCRATCH_MAX_BYTES
                        && snapshot.wire_state() == pb_pbmux::WireReservationState::Committed =>
                {
                    snapshot.bytes
                }
                _ => {
                    return self.cache_compute_refusal(
                        proof.lease_id,
                        session_id,
                        request_id,
                        submit,
                        ComputeReason::ReservationInvalid,
                        now_ms,
                    );
                }
            };
        let staged = match self.compute_jobs.stage(
            proof,
            session_id,
            request_id,
            submit,
            scratch_bytes,
            now_ms,
        ) {
            Ok(staged) => staged,
            Err(()) => {
                return self.cache_compute_refusal(
                    proof.lease_id,
                    session_id,
                    request_id,
                    submit,
                    ComputeReason::InternalError,
                    now_ms,
                );
            }
        };
        if self
            .resource_guard
            .consume_for_compute(proof, reservation_id, now_ms)
            .is_err()
        {
            return self.cache_compute_refusal(
                proof.lease_id,
                session_id,
                request_id,
                submit,
                ComputeReason::ReservationInvalid,
                now_ms,
            );
        }
        let job_id = self.compute_jobs.publish_running(staged, submit, now_ms);
        let (buffer_id, input_offset, input_length) = self
            .compute_jobs
            .job_input(job_id)
            .expect("published job retains input reference");
        let clock = &self.clock;
        let digest = self.remote_buffers.with_compute_input(
            proof,
            session_id,
            buffer_id,
            input_offset,
            input_length,
            |input| compute::blake3_digest_bounded(input, now_ms, || clock.now_ms()),
        );
        let finished_ms = self.clock.now_ms();
        self.compute_jobs.terminalize(
            &mut self.resource_guard,
            job_id,
            session_id,
            digest.unwrap_or(Err(ComputeReason::InternalError)),
            finished_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn cache_compute_refusal(
        &mut self,
        lease_id: LeaseId,
        session_id: remote_buffer::SessionBinding,
        request_id: u64,
        submit: ComputeSubmit,
        reason: ComputeReason,
        now_ms: u64,
    ) -> ComputeResponse {
        let response = compute::absent_result(submit, self.incarnation, reason);
        self.compute_jobs
            .cache_refusal(lease_id, request_id, session_id, submit, response, now_ms);
        response
    }

    /// Terminalizes only buffers created by this exact authenticated session.
    /// The C07 controller lease is intentionally left untouched.
    pub fn authenticated_session_ended(&mut self, verified_session: &VerifiedPeerSession<'_>) {
        let now_ms = self.clock.now_ms();
        let session_id =
            remote_buffer::SessionBinding::from_verified(verified_session.session_id());
        self.compute_jobs
            .session_lost(&mut self.resource_guard, session_id, now_ms);
        self.remote_buffers
            .session_lost(&mut self.resource_guard, session_id, now_ms);
    }
}

fn compute_request_failure(
    request: ComputeRequest,
    incarnation: WorkerIncarnationId,
    reason: ComputeReason,
) -> ComputeResponse {
    match request {
        ComputeRequest::Submit(submit) => compute::absent_result(submit, incarnation, reason),
        ComputeRequest::Status(status) => compute::absent_status(status, incarnation, reason),
        ComputeRequest::Cancel(cancel) => compute::absent_cancel(cancel, incarnation, reason),
    }
}

fn resource_response_kind(request: &ResourceRequest) -> ResourceResponseKind {
    match request {
        ResourceRequest::Reserve { .. } => ResourceResponseKind::ReserveAck,
        ResourceRequest::Commit { .. } => ResourceResponseKind::Commit,
        ResourceRequest::Release { .. } => ResourceResponseKind::Release,
    }
}

fn resource_success(
    lease_id: [u8; 16],
    incarnation: WorkerIncarnationId,
    snapshot: resource_guard::ReservationSnapshot,
) -> ResourceResult {
    ResourceResult {
        state: ResourceResultState::Completed,
        reason: ResourceReason::None,
        lease_id,
        worker_incarnation_id: incarnation.into_bytes(),
        reservation: Some(ReservationResultRef {
            reservation_id: snapshot.reservation_id.into_bytes(),
            state: snapshot.wire_state(),
            resource_class: match snapshot.class {
                ResourceClass::RemoteBufferBytes => WireResourceClass::RemoteBufferBytes,
                ResourceClass::NativeOpScratchBytes => WireResourceClass::NativeOpScratchBytes,
                ResourceClass::Unknown => unreachable!("unknown class has no reservation"),
            },
            granted_bytes: snapshot.bytes,
            ttl_remaining_ms: snapshot.ttl_remaining_ms,
        }),
    }
}

fn resource_failure(
    lease_id: [u8; 16],
    incarnation: WorkerIncarnationId,
    reason: ResourceReason,
) -> ResourceResult {
    resource_failure_with_snapshot(lease_id, incarnation, reason, None)
}

fn resource_failure_with_snapshot(
    lease_id: [u8; 16],
    incarnation: WorkerIncarnationId,
    reason: ResourceReason,
    snapshot: Option<resource_guard::ReservationSnapshot>,
) -> ResourceResult {
    ResourceResult {
        state: ResourceResultState::Failed,
        reason,
        lease_id,
        worker_incarnation_id: incarnation.into_bytes(),
        reservation: snapshot.map(|snapshot| ReservationResultRef {
            reservation_id: snapshot.reservation_id.into_bytes(),
            state: snapshot.wire_state(),
            resource_class: match snapshot.class {
                ResourceClass::RemoteBufferBytes => WireResourceClass::RemoteBufferBytes,
                ResourceClass::NativeOpScratchBytes => WireResourceClass::NativeOpScratchBytes,
                ResourceClass::Unknown => unreachable!("unknown class has no reservation"),
            },
            granted_bytes: snapshot.bytes,
            ttl_remaining_ms: snapshot.ttl_remaining_ms,
        }),
    }
}

fn remote_buffer_response_kind(request: &RemoteBufferRequest) -> RemoteBufferResponseKind {
    match request {
        RemoteBufferRequest::Alloc { .. } => RemoteBufferResponseKind::AllocAck,
        RemoteBufferRequest::Put { .. } => RemoteBufferResponseKind::Put,
        RemoteBufferRequest::Get { .. } => RemoteBufferResponseKind::Data,
        RemoteBufferRequest::Free { .. } => RemoteBufferResponseKind::Free,
        RemoteBufferRequest::Stat { .. } => RemoteBufferResponseKind::Stat,
        RemoteBufferRequest::Touch { .. } => RemoteBufferResponseKind::Touch,
    }
}

fn remote_buffer_failure(
    lease_id: [u8; 16],
    incarnation: WorkerIncarnationId,
    reason: BufferReason,
) -> BufferResult {
    BufferResult {
        completed: false,
        reason,
        lease_id,
        worker_incarnation_id: incarnation.into_bytes(),
        buffer: None,
        reservation_id: None,
        offset: 0,
        data_len: 0,
        data: Vec::new(),
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
}
