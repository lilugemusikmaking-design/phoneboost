use std::collections::BTreeMap;

use pb_pbmux::{
    ComputeJobRef, ComputeJobRequest, ComputeJobState, ComputeReason, ComputeResponse,
    ComputeResult, ComputeStatus, ComputeSubmit,
};
use pb_types::PeerId;

use crate::lease::{LeaseId, LeaseProof};
use crate::remote_buffer::SessionBinding;
use crate::resource_guard::{ReservationId, ResourceGuard};
use crate::{WorkerIncarnationId, random_nonzero_128};

pub(crate) const COMPUTE_IDEMPOTENCE_CAPACITY_PER_LEASE: usize = 256;
pub(crate) const COMPUTE_TERMINAL_RETENTION_MS: u64 = 5 * 60_000;
pub(crate) const PROVIDER_TIMEOUT_MS: u64 = 30_000;
const HASH_CHUNK_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InputReference {
    buffer_id: [u8; 16],
    offset: u64,
    length: u64,
}

struct JobRecord {
    job_id: [u8; 16],
    owner_peer_id: PeerId,
    controller_lease_id: LeaseId,
    worker_incarnation_id: WorkerIncarnationId,
    session_id: SessionBinding,
    provider_id: u8,
    provider_version: u8,
    input: InputReference,
    scratch_reservation_id: ReservationId,
    scratch_bytes: u64,
    state: ComputeJobState,
    reason: ComputeReason,
    created_mono_ms: u64,
    started_mono_ms: Option<u64>,
    terminal_mono_ms: Option<u64>,
    digest: Option<[u8; 32]>,
    submit_request_id: u64,
    scratch_released: bool,
}

impl JobRecord {
    fn job_ref(&self) -> ComputeJobRef {
        ComputeJobRef {
            job_id: self.job_id,
            provider_id: self.provider_id,
            provider_version: self.provider_version,
        }
    }

    fn result(&self) -> ComputeResponse {
        ComputeResponse::Result(ComputeResult {
            state: self.state,
            reason: self.reason,
            lease_id: self.controller_lease_id.into_bytes(),
            worker_incarnation_id: self.worker_incarnation_id.into_bytes(),
            job: Some(self.job_ref()),
            digest: self.digest,
        })
    }
}

#[derive(Clone, Copy)]
struct SubmitCacheEntry {
    session_id: SessionBinding,
    request: ComputeSubmit,
    response: ComputeResponse,
    terminal_mono_ms: Option<u64>,
}

#[derive(Clone, Copy)]
struct CancelCacheEntry {
    session_id: SessionBinding,
    request: ComputeJobRequest,
    response: ComputeResponse,
    terminal_mono_ms: u64,
}

pub(crate) enum SubmitLookup {
    Vacant,
    Replay(ComputeResponse),
    Conflict,
    Full,
}

pub(crate) struct StagedJob {
    record: JobRecord,
}

/// Private C10 single-writer authority. No raw store mutation crosses WorkerCore.
pub(crate) struct ComputeJobStore {
    jobs: BTreeMap<[u8; 16], JobRecord>,
    submits: BTreeMap<(LeaseId, u64), SubmitCacheEntry>,
    cancels: BTreeMap<(LeaseId, u64), CancelCacheEntry>,
}

impl ComputeJobStore {
    pub(crate) const fn new() -> Self {
        Self {
            jobs: BTreeMap::new(),
            submits: BTreeMap::new(),
            cancels: BTreeMap::new(),
        }
    }

    pub(crate) fn check_submit(
        &mut self,
        lease_id: LeaseId,
        request_id: u64,
        session_id: SessionBinding,
        request: ComputeSubmit,
        now_ms: u64,
    ) -> SubmitLookup {
        self.purge(now_ms);
        let key = (lease_id, request_id);
        if self
            .submits
            .get(&key)
            .is_some_and(|entry| entry.session_id != session_id)
        {
            self.submits.remove(&key);
        }
        if let Some(entry) = self.submits.get(&key) {
            return if entry.request == request {
                SubmitLookup::Replay(entry.response)
            } else {
                SubmitLookup::Conflict
            };
        }
        if self
            .submits
            .keys()
            .filter(|(cached_lease, _)| *cached_lease == lease_id)
            .count()
            >= COMPUTE_IDEMPOTENCE_CAPACITY_PER_LEASE
        {
            SubmitLookup::Full
        } else {
            SubmitLookup::Vacant
        }
    }

    pub(crate) fn cache_refusal(
        &mut self,
        lease_id: LeaseId,
        request_id: u64,
        session_id: SessionBinding,
        request: ComputeSubmit,
        response: ComputeResponse,
        now_ms: u64,
    ) {
        self.submits.insert(
            (lease_id, request_id),
            SubmitCacheEntry {
                session_id,
                request,
                response,
                terminal_mono_ms: Some(now_ms),
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn stage(
        &self,
        proof: LeaseProof,
        session_id: SessionBinding,
        request_id: u64,
        request: ComputeSubmit,
        scratch_bytes: u64,
        now_ms: u64,
    ) -> Result<StagedJob, ()> {
        for _ in 0..8 {
            let job_id = random_nonzero_128().map_err(|_| ())?;
            if !self.jobs.contains_key(&job_id) {
                return Ok(StagedJob {
                    record: JobRecord {
                        job_id,
                        owner_peer_id: proof.peer_id,
                        controller_lease_id: proof.lease_id,
                        worker_incarnation_id: proof.incarnation,
                        session_id,
                        provider_id: request.provider_id,
                        provider_version: request.provider_version,
                        input: InputReference {
                            buffer_id: request.buffer_id,
                            offset: request.input_offset,
                            length: request.input_length,
                        },
                        scratch_reservation_id: ReservationId::from_bytes(request.reservation_id),
                        scratch_bytes,
                        state: ComputeJobState::Accepted,
                        reason: ComputeReason::None,
                        created_mono_ms: now_ms,
                        started_mono_ms: None,
                        terminal_mono_ms: None,
                        digest: None,
                        submit_request_id: request_id,
                        scratch_released: false,
                    },
                });
            }
        }
        Err(())
    }

    pub(crate) fn publish_running(
        &mut self,
        staged: StagedJob,
        request: ComputeSubmit,
        now_ms: u64,
    ) -> [u8; 16] {
        let mut record = staged.record;
        record.state = ComputeJobState::Running;
        record.started_mono_ms = Some(now_ms);
        let job_id = record.job_id;
        let response = ComputeResponse::Status(ComputeStatus {
            state: ComputeJobState::Running,
            reason: ComputeReason::None,
            lease_id: record.controller_lease_id.into_bytes(),
            worker_incarnation_id: record.worker_incarnation_id.into_bytes(),
            job: Some(record.job_ref()),
        });
        self.submits.insert(
            (record.controller_lease_id, record.submit_request_id),
            SubmitCacheEntry {
                session_id: record.session_id,
                request,
                response,
                terminal_mono_ms: None,
            },
        );
        self.jobs.insert(job_id, record);
        job_id
    }

    pub(crate) fn terminalize(
        &mut self,
        guard: &mut ResourceGuard,
        job_id: [u8; 16],
        session_id: SessionBinding,
        outcome: Result<[u8; 32], ComputeReason>,
        now_ms: u64,
    ) -> ComputeResponse {
        let record = self.jobs.get_mut(&job_id).expect("published job exists");
        if record.state.is_terminal() {
            return record.result();
        }
        if record.session_id != session_id {
            Self::finish_record(
                record,
                guard,
                ComputeJobState::Failed,
                ComputeReason::SessionLost,
                None,
                now_ms,
            );
        } else if now_ms.saturating_sub(record.started_mono_ms.unwrap_or(record.created_mono_ms))
            > PROVIDER_TIMEOUT_MS
        {
            Self::finish_record(
                record,
                guard,
                ComputeJobState::Failed,
                ComputeReason::ProviderTimeout,
                None,
                now_ms,
            );
        } else {
            match outcome {
                Ok(digest) => Self::finish_record(
                    record,
                    guard,
                    ComputeJobState::Completed,
                    ComputeReason::None,
                    Some(digest),
                    now_ms,
                ),
                Err(reason) => Self::finish_record(
                    record,
                    guard,
                    ComputeJobState::Failed,
                    reason,
                    None,
                    now_ms,
                ),
            }
        }
        let response = record.result();
        if let Some(entry) = self
            .submits
            .get_mut(&(record.controller_lease_id, record.submit_request_id))
        {
            entry.response = response;
            entry.terminal_mono_ms = Some(now_ms);
        }
        response
    }

    pub(crate) fn status(
        &mut self,
        proof: LeaseProof,
        session_id: SessionBinding,
        request: ComputeJobRequest,
        now_ms: u64,
    ) -> ComputeResponse {
        self.purge(now_ms);
        let Some(record) = self.jobs.get(&request.job_id) else {
            return absent_status(request, proof.incarnation, ComputeReason::JobNotFound);
        };
        if !Self::owns(record, proof, session_id) {
            return absent_status(request, proof.incarnation, ComputeReason::JobNotOwned);
        }
        if record.state.is_terminal() {
            record.result()
        } else {
            ComputeResponse::Status(ComputeStatus {
                state: record.state,
                reason: ComputeReason::None,
                lease_id: request.lease_id,
                worker_incarnation_id: proof.incarnation.into_bytes(),
                job: Some(record.job_ref()),
            })
        }
    }

    pub(crate) fn cancel(
        &mut self,
        guard: &mut ResourceGuard,
        proof: LeaseProof,
        session_id: SessionBinding,
        request_id: u64,
        request: ComputeJobRequest,
        now_ms: u64,
    ) -> ComputeResponse {
        self.purge(now_ms);
        let key = (proof.lease_id, request_id);
        if let Some(entry) = self.cancels.get(&key) {
            if entry.session_id == session_id && entry.request == request {
                return entry.response;
            }
            return absent_cancel(request, proof.incarnation, ComputeReason::InternalError);
        }
        let response = match self.jobs.get_mut(&request.job_id) {
            None => absent_cancel(request, proof.incarnation, ComputeReason::JobNotFound),
            Some(record) if !Self::owns(record, proof, session_id) => {
                absent_cancel(request, proof.incarnation, ComputeReason::JobNotOwned)
            }
            Some(record) if record.state.is_terminal() => ComputeResponse::Cancel(ComputeStatus {
                state: record.state,
                reason: ComputeReason::JobNotCancellable,
                lease_id: request.lease_id,
                worker_incarnation_id: proof.incarnation.into_bytes(),
                job: Some(record.job_ref()),
            }),
            Some(record) => {
                Self::finish_record(
                    record,
                    guard,
                    ComputeJobState::Cancelled,
                    ComputeReason::None,
                    None,
                    now_ms,
                );
                let response = ComputeResponse::Cancel(ComputeStatus {
                    state: ComputeJobState::Cancelled,
                    reason: ComputeReason::None,
                    lease_id: request.lease_id,
                    worker_incarnation_id: proof.incarnation.into_bytes(),
                    job: Some(record.job_ref()),
                });
                if let Some(entry) = self
                    .submits
                    .get_mut(&(record.controller_lease_id, record.submit_request_id))
                {
                    entry.response = record.result();
                    entry.terminal_mono_ms = Some(now_ms);
                }
                response
            }
        };
        self.cancels.insert(
            key,
            CancelCacheEntry {
                session_id,
                request,
                response,
                terminal_mono_ms: now_ms,
            },
        );
        response
    }

    pub(crate) fn session_lost(
        &mut self,
        guard: &mut ResourceGuard,
        session_id: SessionBinding,
        now_ms: u64,
    ) {
        for record in self.jobs.values_mut() {
            if record.session_id == session_id && !record.state.is_terminal() {
                Self::finish_record(
                    record,
                    guard,
                    ComputeJobState::Failed,
                    ComputeReason::SessionLost,
                    None,
                    now_ms,
                );
            }
        }
        self.submits
            .retain(|_, entry| entry.session_id != session_id);
        self.cancels
            .retain(|_, entry| entry.session_id != session_id);
    }

    pub(crate) fn lease_ended(
        &mut self,
        guard: &mut ResourceGuard,
        lease_id: LeaseId,
        now_ms: u64,
    ) {
        for record in self.jobs.values_mut() {
            if record.controller_lease_id == lease_id && !record.state.is_terminal() {
                Self::finish_record(
                    record,
                    guard,
                    ComputeJobState::Cancelled,
                    ComputeReason::None,
                    None,
                    now_ms,
                );
            }
        }
        self.submits
            .retain(|(cached_lease, _), _| *cached_lease != lease_id);
        self.cancels
            .retain(|(cached_lease, _), _| *cached_lease != lease_id);
    }

    fn owns(record: &JobRecord, proof: LeaseProof, session_id: SessionBinding) -> bool {
        record.owner_peer_id == proof.peer_id
            && record.controller_lease_id == proof.lease_id
            && record.worker_incarnation_id == proof.incarnation
            && record.session_id == session_id
    }

    fn finish_record(
        record: &mut JobRecord,
        guard: &mut ResourceGuard,
        state: ComputeJobState,
        reason: ComputeReason,
        digest: Option<[u8; 32]>,
        now_ms: u64,
    ) {
        if record.state.is_terminal() {
            return;
        }
        record.state = state;
        record.reason = reason;
        record.digest = digest;
        record.terminal_mono_ms = Some(now_ms);
        if !record.scratch_released {
            guard.release_consumed_compute(
                record.scratch_reservation_id,
                record.scratch_bytes,
                now_ms,
            );
            record.scratch_released = true;
        }
    }

    fn purge(&mut self, now_ms: u64) {
        self.jobs.retain(|_, record| {
            record.terminal_mono_ms.is_none_or(|terminal| {
                now_ms.saturating_sub(terminal) <= COMPUTE_TERMINAL_RETENTION_MS
            })
        });
        self.submits.retain(|_, entry| {
            entry.terminal_mono_ms.is_none_or(|terminal| {
                now_ms.saturating_sub(terminal) <= COMPUTE_TERMINAL_RETENTION_MS
            })
        });
        self.cancels.retain(|_, entry| {
            now_ms.saturating_sub(entry.terminal_mono_ms) <= COMPUTE_TERMINAL_RETENTION_MS
        });
    }

    pub(crate) fn job_input(&self, job_id: [u8; 16]) -> Option<([u8; 16], u64, u64)> {
        self.jobs.get(&job_id).map(|record| {
            (
                record.input.buffer_id,
                record.input.offset,
                record.input.length,
            )
        })
    }
}

#[cfg(test)]
pub(crate) fn blake3_digest(input: &[u8]) -> [u8; 32] {
    blake3_digest_bounded(input, 0, || 0).expect("zero-duration local hash")
}

pub(crate) fn blake3_digest_bounded(
    input: &[u8],
    started_ms: u64,
    mut now_ms: impl FnMut() -> u64,
) -> Result<[u8; 32], ComputeReason> {
    let mut hasher = blake3::Hasher::new();
    for chunk in input.chunks(HASH_CHUNK_BYTES) {
        if now_ms().saturating_sub(started_ms) > PROVIDER_TIMEOUT_MS {
            return Err(ComputeReason::ProviderTimeout);
        }
        hasher.update(chunk);
    }
    if now_ms().saturating_sub(started_ms) > PROVIDER_TIMEOUT_MS {
        return Err(ComputeReason::ProviderTimeout);
    }
    Ok(*hasher.finalize().as_bytes())
}

pub(crate) fn absent_result(
    submit: ComputeSubmit,
    incarnation: WorkerIncarnationId,
    reason: ComputeReason,
) -> ComputeResponse {
    ComputeResponse::Result(ComputeResult {
        state: ComputeJobState::Invalid,
        reason,
        lease_id: submit.lease_id,
        worker_incarnation_id: incarnation.into_bytes(),
        job: None,
        digest: None,
    })
}

pub(crate) fn absent_status(
    request: ComputeJobRequest,
    incarnation: WorkerIncarnationId,
    reason: ComputeReason,
) -> ComputeResponse {
    ComputeResponse::Status(ComputeStatus {
        state: ComputeJobState::Invalid,
        reason,
        lease_id: request.lease_id,
        worker_incarnation_id: incarnation.into_bytes(),
        job: None,
    })
}

pub(crate) fn absent_cancel(
    request: ComputeJobRequest,
    incarnation: WorkerIncarnationId,
    reason: ComputeReason,
) -> ComputeResponse {
    ComputeResponse::Cancel(ComputeStatus {
        state: ComputeJobState::Invalid,
        reason,
        lease_id: request.lease_id,
        worker_incarnation_id: incarnation.into_bytes(),
        job: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lease::{AuthenticatedSession, ControllerLeaseManager};
    use crate::resource_guard::{
        HealthSample, RequestId, ReservationDecision, ReserveRequest, ResourceClass, ThermalBand,
    };

    fn setup() -> (
        ControllerLeaseManager,
        AuthenticatedSession,
        LeaseProof,
        ResourceGuard,
    ) {
        let now_ms = 10_000;
        let incarnation = WorkerIncarnationId([1; 16]);
        let session = AuthenticatedSession::test_only(PeerId::from_sha256_digest([7; 32]));
        let mut leases = ControllerLeaseManager::new(incarnation);
        let lease_id = leases.acquire(&session, now_ms).unwrap();
        let proof = leases.validate(&session, lease_id, now_ms).unwrap();
        let mut guard = ResourceGuard::new();
        for sample_time in [0, now_ms] {
            guard.record_health(HealthSample {
                available_memory_bytes: 2 * 1024 * 1024 * 1024,
                low_memory: false,
                thermal: ThermalBand::None,
                battery_percent: 80,
                charging: false,
                power_save: false,
                monotonic_ms: sample_time,
            });
        }
        (leases, session, proof, guard)
    }

    fn committed_scratch(
        leases: &mut ControllerLeaseManager,
        session: &AuthenticatedSession,
        proof: LeaseProof,
        guard: &mut ResourceGuard,
        request_id: u64,
    ) -> ReservationId {
        let ReservationDecision::Reserved { reservation_id } = guard.reserve(
            leases,
            session,
            proof.lease_id,
            ReserveRequest {
                request_id: RequestId::from_u64(request_id),
                class: ResourceClass::NativeOpScratchBytes,
                bytes: 1024,
            },
            10_000,
        ) else {
            panic!("native scratch must reserve");
        };
        assert_eq!(
            guard.commit(leases, session, proof.lease_id, reservation_id, 10_000),
            crate::CommitDecision::Committed
        );
        reservation_id
    }

    fn submit(proof: LeaseProof, reservation_id: ReservationId) -> ComputeSubmit {
        ComputeSubmit {
            lease_id: proof.lease_id.into_bytes(),
            worker_incarnation_id: proof.incarnation.into_bytes(),
            reservation_id: reservation_id.into_bytes(),
            provider_id: 1,
            provider_version: 1,
            input_kind: 1,
            buffer_id: [3; 16],
            input_offset: 0,
            input_length: 3,
        }
    }

    #[test]
    fn provider_known_vectors_are_deterministic_exact_and_nonmutating() {
        assert_eq!(
            blake3_digest(b"abc"),
            [
                0x64, 0x37, 0xb3, 0xac, 0x38, 0x46, 0x51, 0x33, 0xff, 0xb6, 0x3b, 0x75, 0x27, 0x3a,
                0x8d, 0xb5, 0x48, 0xc5, 0x58, 0x46, 0x5d, 0x79, 0xdb, 0x03, 0xfd, 0x35, 0x9c, 0x6c,
                0xd5, 0xbd, 0x9d, 0x85,
            ]
        );
        let one = [0x5a];
        let before = one;
        let first = blake3_digest(&one);
        assert_eq!(first.len(), 32);
        assert_eq!(first, blake3_digest(&one));
        assert_eq!(one, before);
        assert_eq!(
            blake3_digest(&[]),
            *blake3::hash(&[]).as_bytes(),
            "empty input remains a supported range"
        );

        let calls = std::cell::Cell::new(0_u8);
        let timeout = blake3_digest_bounded(&vec![0; 2 * HASH_CHUNK_BYTES], 0, || {
            let call = calls.get();
            calls.set(call + 1);
            if call == 0 {
                0
            } else {
                PROVIDER_TIMEOUT_MS + 1
            }
        });
        assert_eq!(timeout, Err(ComputeReason::ProviderTimeout));
    }

    #[test]
    fn job_timeout_replay_cancel_session_loss_and_budget_cleanup_are_terminal() {
        let (mut leases, session, proof, mut guard) = setup();
        let session_id = SessionBinding::test_only(1);
        let mut jobs = ComputeJobStore::new();

        let scratch = committed_scratch(&mut leases, &session, proof, &mut guard, 1);
        let request = submit(proof, scratch);
        assert!(matches!(
            jobs.check_submit(proof.lease_id, 10, session_id, request, 10_000),
            SubmitLookup::Vacant
        ));
        let staged = jobs
            .stage(proof, session_id, 10, request, 1024, 10_000)
            .unwrap();
        assert_eq!(guard.consume_for_compute(proof, scratch, 10_000), Ok(1024));
        let job_id = jobs.publish_running(staged, request, 10_000);
        let timeout = jobs.terminalize(&mut guard, job_id, session_id, Ok([9; 32]), 40_001);
        let ComputeResponse::Result(timeout) = timeout else {
            panic!("timeout must be RESULT");
        };
        assert_eq!(timeout.state, ComputeJobState::Failed);
        assert_eq!(timeout.reason, ComputeReason::ProviderTimeout);
        assert!(timeout.digest.is_none());
        assert_eq!(guard.held_bytes(), 0);
        assert!(matches!(
            jobs.check_submit(proof.lease_id, 10, session_id, request, 40_001),
            SubmitLookup::Replay(ComputeResponse::Result(result))
                if result.reason == ComputeReason::ProviderTimeout
        ));
        let mut conflict = request;
        conflict.input_length = 2;
        assert!(matches!(
            jobs.check_submit(proof.lease_id, 10, session_id, conflict, 40_001),
            SubmitLookup::Conflict
        ));
        let wrong_session = jobs.status(
            proof,
            SessionBinding::test_only(2),
            ComputeJobRequest {
                lease_id: proof.lease_id.into_bytes(),
                worker_incarnation_id: proof.incarnation.into_bytes(),
                job_id,
            },
            40_001,
        );
        assert!(matches!(
            wrong_session,
            ComputeResponse::Status(status) if status.reason == ComputeReason::JobNotOwned
        ));

        let scratch = committed_scratch(&mut leases, &session, proof, &mut guard, 2);
        let request = submit(proof, scratch);
        let staged = jobs
            .stage(proof, session_id, 11, request, 1024, 10_000)
            .unwrap();
        guard.consume_for_compute(proof, scratch, 10_000).unwrap();
        let lost_job = jobs.publish_running(staged, request, 10_000);
        jobs.session_lost(&mut guard, session_id, 10_001);
        assert_eq!(guard.held_bytes(), 0);
        let no_resurrection =
            jobs.terminalize(&mut guard, lost_job, session_id, Ok([4; 32]), 10_002);
        assert!(matches!(
            no_resurrection,
            ComputeResponse::Result(result)
                if result.state == ComputeJobState::Failed
                    && result.reason == ComputeReason::SessionLost
                    && result.digest.is_none()
        ));

        let scratch = committed_scratch(&mut leases, &session, proof, &mut guard, 3);
        let request = submit(proof, scratch);
        let staged = jobs
            .stage(proof, session_id, 12, request, 1024, 10_000)
            .unwrap();
        guard.consume_for_compute(proof, scratch, 10_000).unwrap();
        let cancel_job = jobs.publish_running(staged, request, 10_000);
        let cancel_request = ComputeJobRequest {
            lease_id: proof.lease_id.into_bytes(),
            worker_incarnation_id: proof.incarnation.into_bytes(),
            job_id: cancel_job,
        };
        let cancelled = jobs.cancel(&mut guard, proof, session_id, 20, cancel_request, 10_001);
        assert!(matches!(
            cancelled,
            ComputeResponse::Cancel(status)
                if status.state == ComputeJobState::Cancelled
                    && status.reason == ComputeReason::None
        ));
        assert_eq!(
            jobs.cancel(&mut guard, proof, session_id, 20, cancel_request, 10_002,),
            cancelled
        );
        assert_eq!(guard.held_bytes(), 0);
    }

    #[test]
    fn submit_idempotence_capacity_is_256_and_never_evicts_nonterminal() {
        let (mut leases, session, proof, mut guard) = setup();
        let session_id = SessionBinding::test_only(1);
        let mut jobs = ComputeJobStore::new();
        let scratch = committed_scratch(&mut leases, &session, proof, &mut guard, 1);
        let running_request = submit(proof, scratch);
        let staged = jobs
            .stage(proof, session_id, 1, running_request, 1024, 10_000)
            .unwrap();
        guard.consume_for_compute(proof, scratch, 10_000).unwrap();
        jobs.publish_running(staged, running_request, 10_000);

        for request_id in 2..=256_u64 {
            let mut refused_request = running_request;
            refused_request.provider_id = 2;
            refused_request.reservation_id[15] = request_id as u8;
            let response = absent_result(
                refused_request,
                proof.incarnation,
                ComputeReason::UnsupportedProvider,
            );
            jobs.cache_refusal(
                proof.lease_id,
                request_id,
                session_id,
                refused_request,
                response,
                10_000,
            );
        }
        assert!(matches!(
            jobs.check_submit(proof.lease_id, 257, session_id, running_request, 10_000,),
            SubmitLookup::Full
        ));
        assert!(matches!(
            jobs.check_submit(
                proof.lease_id,
                1,
                session_id,
                running_request,
                10_000 + COMPUTE_TERMINAL_RETENTION_MS + 1,
            ),
            SubmitLookup::Replay(ComputeResponse::Status(status))
                if status.state == ComputeJobState::Running
        ));
        jobs.session_lost(
            &mut guard,
            session_id,
            10_000 + COMPUTE_TERMINAL_RETENTION_MS + 2,
        );
        assert_eq!(guard.held_bytes(), 0);
    }
}
