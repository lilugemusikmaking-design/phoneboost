use std::collections::BTreeMap;

use pb_pbmux::{
    AllocationFlags, BufferReason, BufferResult, BufferResultRef, BufferState, MAX_DATA_BODY,
    RemoteBufferRequest, RemoteBufferResponseKind,
};
use pb_runtime_secure::VerifiedSessionId;
use pb_types::PeerId;

use crate::lease::{LeaseId, LeaseProof};
use crate::resource_guard::{ReservationId, ResourceGuard};
use crate::{WorkerIncarnationId, random_nonzero_128};

pub const MAX_REMOTE_BUFFER_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_BUFFERS_PER_LEASE: usize = 8;
pub const DEFAULT_BUFFER_TTL_MS: u64 = 300_000;
pub const MAX_BUFFER_LIFETIME_MS: u64 = 1_800_000;
const TOMBSTONE_RETENTION_MS: u64 = 300_000;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum SessionBinding {
    Production(VerifiedSessionId),
    #[cfg(test)]
    Test(u64),
}

impl SessionBinding {
    pub(crate) const fn from_verified(session_id: VerifiedSessionId) -> Self {
        Self::Production(session_id)
    }

    #[cfg(test)]
    const fn test_only(value: u64) -> Self {
        Self::Test(value)
    }
}

struct BufferRecord {
    buffer_id: [u8; 16],
    owner_peer_id: PeerId,
    owner_controller_lease_id: LeaseId,
    worker_incarnation_id: WorkerIncarnationId,
    session_id: SessionBinding,
    reservation_id: ReservationId,
    size_bytes: u64,
    state: BufferState,
    created_mono_ms: u64,
    expires_mono_ms: u64,
    last_touch_mono_ms: u64,
    allocation_flags: AllocationFlags,
    initialized_ranges: Vec<(u64, u64)>,
    backing: Option<Vec<u8>>,
    budget_released: bool,
    terminal_mono_ms: Option<u64>,
}

struct StagedBuffer {
    buffer_id: [u8; 16],
    backing: Vec<u8>,
}

/// Single-writer C09 content and lifecycle authority.
pub(crate) struct RemoteBufferStore {
    buffers: BTreeMap<[u8; 16], BufferRecord>,
}

impl RemoteBufferStore {
    pub(crate) const fn new() -> Self {
        Self {
            buffers: BTreeMap::new(),
        }
    }

    pub(crate) fn apply(
        &mut self,
        guard: &mut ResourceGuard,
        proof: LeaseProof,
        session_id: SessionBinding,
        request: RemoteBufferRequest,
        now_ms: u64,
    ) -> (RemoteBufferResponseKind, BufferResult) {
        self.expire_and_purge(guard, now_ms);
        match request {
            RemoteBufferRequest::Alloc {
                lease_id,
                reservation_id,
                size_bytes,
                allocation_flags,
                ..
            } => (
                RemoteBufferResponseKind::AllocAck,
                self.alloc(
                    guard,
                    proof,
                    session_id,
                    lease_id,
                    reservation_id,
                    size_bytes,
                    allocation_flags,
                    now_ms,
                ),
            ),
            RemoteBufferRequest::Put {
                lease_id,
                buffer_id,
                offset,
                data,
                ..
            } => (
                RemoteBufferResponseKind::Put,
                self.put(
                    guard, proof, session_id, lease_id, buffer_id, offset, data, now_ms,
                ),
            ),
            RemoteBufferRequest::Get {
                lease_id,
                buffer_id,
                offset,
                length,
                ..
            } => (
                RemoteBufferResponseKind::Data,
                self.get(
                    guard, proof, session_id, lease_id, buffer_id, offset, length, now_ms,
                ),
            ),
            RemoteBufferRequest::Free {
                lease_id,
                buffer_id,
                ..
            } => (
                RemoteBufferResponseKind::Free,
                self.free(guard, proof, session_id, lease_id, buffer_id, now_ms),
            ),
            RemoteBufferRequest::Stat {
                lease_id,
                buffer_id,
                ..
            } => (
                RemoteBufferResponseKind::Stat,
                self.stat(proof, session_id, lease_id, buffer_id, now_ms),
            ),
            RemoteBufferRequest::Touch {
                lease_id,
                buffer_id,
                ..
            } => (
                RemoteBufferResponseKind::Touch,
                self.touch(proof, session_id, lease_id, buffer_id, now_ms),
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn alloc(
        &mut self,
        guard: &mut ResourceGuard,
        proof: LeaseProof,
        session_id: SessionBinding,
        wire_lease_id: [u8; 16],
        wire_reservation_id: [u8; 16],
        size_bytes: u64,
        allocation_flags: AllocationFlags,
        now_ms: u64,
    ) -> BufferResult {
        let failed = |reason| Self::failed(wire_lease_id, proof.incarnation, reason, None);
        if size_bytes == 0 || size_bytes > MAX_REMOTE_BUFFER_BYTES {
            return failed(BufferReason::ResourceExhausted);
        }
        let active_count = self
            .buffers
            .values()
            .filter(|record| {
                record.owner_controller_lease_id == proof.lease_id && !record.state.is_terminal()
            })
            .count();
        let held = self
            .buffers
            .values()
            .filter(|record| !record.budget_released)
            .fold(0_u64, |total, record| {
                total.saturating_add(record.size_bytes)
            });
        if active_count >= MAX_BUFFERS_PER_LEASE
            || held
                .checked_add(size_bytes)
                .is_none_or(|total| total > MAX_REMOTE_BUFFER_BYTES)
        {
            return failed(BufferReason::ResourceExhausted);
        }
        let reservation_id = ReservationId::from_bytes(wire_reservation_id);
        let Some(snapshot) = guard.reservation_snapshot(proof, reservation_id, now_ms) else {
            return failed(BufferReason::ReservationInvalid);
        };
        if snapshot.bytes != size_bytes
            || snapshot.wire_state() != pb_pbmux::WireReservationState::Committed
        {
            return failed(BufferReason::ReservationInvalid);
        }
        let staged = match self.stage_buffer(size_bytes) {
            Ok(staged) => staged,
            Err(()) => return failed(BufferReason::ResourceExhausted),
        };
        if guard
            .consume_for_buffer(proof, reservation_id, size_bytes, now_ms)
            .is_err()
        {
            return failed(BufferReason::ReservationInvalid);
        }
        let expires_mono_ms = now_ms.saturating_add(DEFAULT_BUFFER_TTL_MS);
        let buffer_id = staged.buffer_id;
        self.buffers.insert(
            buffer_id,
            BufferRecord {
                buffer_id,
                owner_peer_id: proof.peer_id,
                owner_controller_lease_id: proof.lease_id,
                worker_incarnation_id: proof.incarnation,
                session_id,
                reservation_id,
                size_bytes,
                state: BufferState::Allocated,
                created_mono_ms: now_ms,
                expires_mono_ms,
                last_touch_mono_ms: now_ms,
                allocation_flags,
                initialized_ranges: Vec::new(),
                backing: Some(staged.backing),
                budget_released: false,
                terminal_mono_ms: None,
            },
        );
        BufferResult {
            completed: true,
            reason: BufferReason::None,
            lease_id: wire_lease_id,
            worker_incarnation_id: proof.incarnation.into_bytes(),
            buffer: Some(BufferResultRef {
                buffer_id,
                state: BufferState::Allocated,
                allocation_flags,
                size_bytes,
                ttl_remaining_ms: DEFAULT_BUFFER_TTL_MS as u32,
            }),
            reservation_id: Some(wire_reservation_id),
            offset: 0,
            data_len: 0,
            data: Vec::new(),
        }
    }

    fn stage_buffer(&self, size_bytes: u64) -> Result<StagedBuffer, ()> {
        let size = usize::try_from(size_bytes).map_err(|_| ())?;
        let mut backing = Vec::new();
        backing.try_reserve_exact(size).map_err(|_| ())?;
        backing.resize(size, 0);
        for _ in 0..8 {
            let buffer_id = random_nonzero_128().map_err(|_| ())?;
            if !self.buffers.contains_key(&buffer_id) {
                return Ok(StagedBuffer { buffer_id, backing });
            }
        }
        Err(())
    }

    #[allow(clippy::too_many_arguments)]
    fn put(
        &mut self,
        guard: &mut ResourceGuard,
        proof: LeaseProof,
        session_id: SessionBinding,
        wire_lease_id: [u8; 16],
        buffer_id: [u8; 16],
        offset: u64,
        data: Vec<u8>,
        now_ms: u64,
    ) -> BufferResult {
        let data_len = data.len() as u32;
        let Some(record) =
            self.authorized_record_mut(guard, proof, session_id, wire_lease_id, buffer_id, now_ms)
        else {
            return self.authorization_failure(proof, wire_lease_id, buffer_id, session_id, now_ms);
        };
        if record.state.is_terminal() {
            return Self::terminal_failure(wire_lease_id, proof.incarnation, record, now_ms);
        }
        let Some(end) = offset.checked_add(data.len() as u64) else {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferRangeInvalid,
                Some(Self::result_ref(record, now_ms)),
            );
        };
        if data.is_empty() || end > record.size_bytes {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferRangeInvalid,
                Some(Self::result_ref(record, now_ms)),
            );
        }
        let mut staged_ranges = Vec::new();
        if staged_ranges
            .try_reserve_exact(record.initialized_ranges.len().saturating_add(1))
            .is_err()
        {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::ResourceExhausted,
                Some(Self::result_ref(record, now_ms)),
            );
        }
        staged_ranges.extend_from_slice(&record.initialized_ranges);
        merge_range(&mut staged_ranges, (offset, end));
        let start = offset as usize;
        let end = end as usize;
        record
            .backing
            .as_mut()
            .expect("nonterminal buffer has backing")[start..end]
            .copy_from_slice(&data);
        record.initialized_ranges = staged_ranges;
        record.state = if record.initialized_ranges == [(0, record.size_bytes)] {
            BufferState::Ready
        } else {
            BufferState::Allocated
        };
        BufferResult {
            completed: true,
            reason: BufferReason::None,
            lease_id: wire_lease_id,
            worker_incarnation_id: proof.incarnation.into_bytes(),
            buffer: Some(Self::result_ref(record, now_ms)),
            reservation_id: None,
            offset,
            data_len,
            data: Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn get(
        &mut self,
        guard: &mut ResourceGuard,
        proof: LeaseProof,
        session_id: SessionBinding,
        wire_lease_id: [u8; 16],
        buffer_id: [u8; 16],
        offset: u64,
        length: u32,
        now_ms: u64,
    ) -> BufferResult {
        let Some(record) =
            self.authorized_record_mut(guard, proof, session_id, wire_lease_id, buffer_id, now_ms)
        else {
            return self.authorization_failure(proof, wire_lease_id, buffer_id, session_id, now_ms);
        };
        if record.state.is_terminal() {
            return Self::terminal_failure(wire_lease_id, proof.incarnation, record, now_ms);
        }
        if record.state != BufferState::Ready {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferInvalidState,
                Some(Self::result_ref(record, now_ms)),
            );
        }
        if length == 0 || length as usize > MAX_DATA_BODY {
            let reason = if length as usize > MAX_DATA_BODY {
                BufferReason::PayloadTooLarge
            } else {
                BufferReason::BufferRangeInvalid
            };
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                reason,
                Some(Self::result_ref(record, now_ms)),
            );
        }
        let Some(end) = offset.checked_add(length as u64) else {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferRangeInvalid,
                Some(Self::result_ref(record, now_ms)),
            );
        };
        if end > record.size_bytes {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferRangeInvalid,
                Some(Self::result_ref(record, now_ms)),
            );
        }
        let mut data = Vec::new();
        if data.try_reserve_exact(length as usize).is_err() {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::ResourceExhausted,
                Some(Self::result_ref(record, now_ms)),
            );
        }
        data.extend_from_slice(
            &record.backing.as_ref().expect("READY has backing")[offset as usize..end as usize],
        );
        BufferResult {
            completed: true,
            reason: BufferReason::None,
            lease_id: wire_lease_id,
            worker_incarnation_id: proof.incarnation.into_bytes(),
            buffer: Some(Self::result_ref(record, now_ms)),
            reservation_id: None,
            offset,
            data_len: length,
            data,
        }
    }

    fn stat(
        &mut self,
        proof: LeaseProof,
        session_id: SessionBinding,
        wire_lease_id: [u8; 16],
        buffer_id: [u8; 16],
        now_ms: u64,
    ) -> BufferResult {
        let Some(record) = self.buffers.get(&buffer_id) else {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferNotFound,
                None,
            );
        };
        if !Self::same_owner(record, proof) {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferNotOwned,
                None,
            );
        }
        if record.state.is_terminal() {
            return Self::terminal_failure(wire_lease_id, proof.incarnation, record, now_ms);
        }
        if record.session_id != session_id {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferLost,
                None,
            );
        }
        BufferResult {
            completed: true,
            reason: BufferReason::None,
            lease_id: wire_lease_id,
            worker_incarnation_id: proof.incarnation.into_bytes(),
            buffer: Some(Self::result_ref(record, now_ms)),
            reservation_id: Some(record.reservation_id.into_bytes()),
            offset: 0,
            data_len: 0,
            data: Vec::new(),
        }
    }

    fn touch(
        &mut self,
        proof: LeaseProof,
        session_id: SessionBinding,
        wire_lease_id: [u8; 16],
        buffer_id: [u8; 16],
        now_ms: u64,
    ) -> BufferResult {
        let Some(record) = self.buffers.get_mut(&buffer_id) else {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferNotFound,
                None,
            );
        };
        if !Self::same_owner(record, proof) {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferNotOwned,
                None,
            );
        }
        if record.state.is_terminal() {
            return Self::terminal_failure(wire_lease_id, proof.incarnation, record, now_ms);
        }
        if record.session_id != session_id {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferLost,
                None,
            );
        }
        record.last_touch_mono_ms = now_ms;
        record.expires_mono_ms = now_ms.saturating_add(DEFAULT_BUFFER_TTL_MS).min(
            record
                .created_mono_ms
                .saturating_add(MAX_BUFFER_LIFETIME_MS),
        );
        BufferResult {
            completed: true,
            reason: BufferReason::None,
            lease_id: wire_lease_id,
            worker_incarnation_id: proof.incarnation.into_bytes(),
            buffer: Some(Self::result_ref(record, now_ms)),
            reservation_id: None,
            offset: 0,
            data_len: 0,
            data: Vec::new(),
        }
    }

    fn free(
        &mut self,
        guard: &mut ResourceGuard,
        proof: LeaseProof,
        session_id: SessionBinding,
        wire_lease_id: [u8; 16],
        buffer_id: [u8; 16],
        now_ms: u64,
    ) -> BufferResult {
        let Some(record) = self.buffers.get_mut(&buffer_id) else {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferNotFound,
                None,
            );
        };
        if !Self::same_owner(record, proof) {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferNotOwned,
                None,
            );
        }
        if record.state == BufferState::Freed {
            return Self::completed_free(wire_lease_id, proof.incarnation, record);
        }
        if record.state.is_terminal() {
            return Self::terminal_failure(wire_lease_id, proof.incarnation, record, now_ms);
        }
        if record.session_id != session_id {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferLost,
                None,
            );
        }
        Self::terminalize(record, guard, BufferState::Freed, now_ms);
        Self::completed_free(wire_lease_id, proof.incarnation, record)
    }

    fn completed_free(
        wire_lease_id: [u8; 16],
        incarnation: WorkerIncarnationId,
        record: &BufferRecord,
    ) -> BufferResult {
        BufferResult {
            completed: true,
            reason: BufferReason::None,
            lease_id: wire_lease_id,
            worker_incarnation_id: incarnation.into_bytes(),
            buffer: Some(Self::result_ref(
                record,
                record.terminal_mono_ms.unwrap_or(0),
            )),
            reservation_id: None,
            offset: 0,
            data_len: 0,
            data: Vec::new(),
        }
    }

    fn authorized_record_mut<'a>(
        &'a mut self,
        guard: &mut ResourceGuard,
        proof: LeaseProof,
        session_id: SessionBinding,
        _wire_lease_id: [u8; 16],
        buffer_id: [u8; 16],
        now_ms: u64,
    ) -> Option<&'a mut BufferRecord> {
        let record = self.buffers.get_mut(&buffer_id)?;
        if record.session_id != session_id && !record.state.is_terminal() {
            Self::terminalize(record, guard, BufferState::Lost, now_ms);
        }
        Self::owns(record, proof, session_id).then_some(record)
    }

    fn authorization_failure(
        &self,
        proof: LeaseProof,
        wire_lease_id: [u8; 16],
        buffer_id: [u8; 16],
        session_id: SessionBinding,
        now_ms: u64,
    ) -> BufferResult {
        let Some(record) = self.buffers.get(&buffer_id) else {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferNotFound,
                None,
            );
        };
        if record.owner_peer_id != proof.peer_id
            || record.owner_controller_lease_id != proof.lease_id
        {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferNotOwned,
                None,
            );
        }
        if record.session_id != session_id || record.state == BufferState::Lost {
            return Self::failed(
                wire_lease_id,
                proof.incarnation,
                BufferReason::BufferLost,
                Some(Self::result_ref(record, now_ms)),
            );
        }
        Self::failed(
            wire_lease_id,
            proof.incarnation,
            BufferReason::BufferNotOwned,
            None,
        )
    }

    fn owns(record: &BufferRecord, proof: LeaseProof, session_id: SessionBinding) -> bool {
        Self::same_owner(record, proof) && record.session_id == session_id
    }

    fn same_owner(record: &BufferRecord, proof: LeaseProof) -> bool {
        record.owner_peer_id == proof.peer_id
            && record.owner_controller_lease_id == proof.lease_id
            && record.worker_incarnation_id == proof.incarnation
    }

    fn terminal_failure(
        wire_lease_id: [u8; 16],
        incarnation: WorkerIncarnationId,
        record: &BufferRecord,
        now_ms: u64,
    ) -> BufferResult {
        let reason = match record.state {
            BufferState::Lost => BufferReason::BufferLost,
            BufferState::Freed => BufferReason::BufferFreed,
            BufferState::Evicted => BufferReason::BufferEvicted,
            _ => BufferReason::BufferInvalidState,
        };
        Self::failed(
            wire_lease_id,
            incarnation,
            reason,
            Some(Self::result_ref(record, now_ms)),
        )
    }

    fn failed(
        lease_id: [u8; 16],
        incarnation: WorkerIncarnationId,
        reason: BufferReason,
        buffer: Option<BufferResultRef>,
    ) -> BufferResult {
        BufferResult {
            completed: false,
            reason,
            lease_id,
            worker_incarnation_id: incarnation.into_bytes(),
            buffer,
            reservation_id: None,
            offset: 0,
            data_len: 0,
            data: Vec::new(),
        }
    }

    fn result_ref(record: &BufferRecord, now_ms: u64) -> BufferResultRef {
        BufferResultRef {
            buffer_id: record.buffer_id,
            state: record.state,
            allocation_flags: record.allocation_flags,
            size_bytes: record.size_bytes,
            ttl_remaining_ms: if record.state.is_terminal() {
                0
            } else {
                record
                    .expires_mono_ms
                    .saturating_sub(now_ms)
                    .min(u32::MAX as u64) as u32
            },
        }
    }

    fn terminalize(
        record: &mut BufferRecord,
        guard: &mut ResourceGuard,
        state: BufferState,
        now_ms: u64,
    ) {
        record.backing = None;
        record.initialized_ranges.clear();
        record.state = state;
        record.expires_mono_ms = now_ms;
        record.terminal_mono_ms.get_or_insert(now_ms);
        if !record.budget_released {
            guard.release_consumed_buffer(record.reservation_id, record.size_bytes, now_ms);
            record.budget_released = true;
        }
    }

    fn expire_and_purge(&mut self, guard: &mut ResourceGuard, now_ms: u64) {
        for record in self.buffers.values_mut() {
            if !record.state.is_terminal() && now_ms >= record.expires_mono_ms {
                Self::terminalize(record, guard, BufferState::Evicted, now_ms);
            }
        }
        self.buffers.retain(|_, record| {
            record
                .terminal_mono_ms
                .is_none_or(|terminal| now_ms.saturating_sub(terminal) <= TOMBSTONE_RETENTION_MS)
        });
    }

    pub(crate) fn session_lost(
        &mut self,
        guard: &mut ResourceGuard,
        session_id: SessionBinding,
        now_ms: u64,
    ) {
        self.expire_and_purge(guard, now_ms);
        for record in self.buffers.values_mut() {
            if record.session_id == session_id && !record.state.is_terminal() {
                Self::terminalize(record, guard, BufferState::Lost, now_ms);
            }
        }
    }

    pub(crate) fn tick(&mut self, guard: &mut ResourceGuard, now_ms: u64) {
        self.expire_and_purge(guard, now_ms);
    }
}

fn merge_range(ranges: &mut Vec<(u64, u64)>, new_range: (u64, u64)) {
    ranges.push(new_range);
    ranges.sort_unstable_by_key(|range| range.0);
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges.drain(..) {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    *ranges = merged;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lease::{AuthenticatedSession, ControllerLeaseManager};
    use crate::resource_guard::{
        HealthSample, RequestId, ReservationDecision, ReserveRequest, ResourceClass, ThermalBand,
    };

    fn setup(
        now_ms: u64,
    ) -> (
        ResourceGuard,
        RemoteBufferStore,
        LeaseProof,
        ControllerLeaseManager,
        AuthenticatedSession,
    ) {
        let incarnation = WorkerIncarnationId([1; 16]);
        let session = AuthenticatedSession::test_only(PeerId::from_sha256_digest([7; 32]));
        let mut leases = ControllerLeaseManager::new(incarnation);
        let lease_id = leases.acquire(&session, now_ms).unwrap();
        let proof = leases.validate(&session, lease_id, now_ms).unwrap();
        let mut guard = ResourceGuard::new();
        for sample_time in [now_ms.saturating_sub(10_000), now_ms] {
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
        (guard, RemoteBufferStore::new(), proof, leases, session)
    }

    fn committed_reservation(
        guard: &mut ResourceGuard,
        leases: &mut ControllerLeaseManager,
        session: &AuthenticatedSession,
        proof: LeaseProof,
        request_id: u64,
        size: u64,
        now_ms: u64,
    ) -> ReservationId {
        let decision = guard.reserve(
            leases,
            session,
            proof.lease_id,
            ReserveRequest {
                request_id: RequestId::from_u64(request_id),
                class: ResourceClass::Poc,
                bytes: size,
            },
            now_ms,
        );
        let ReservationDecision::Reserved { reservation_id } = decision else {
            panic!("reservation refused: {decision:?}");
        };
        assert_eq!(
            guard.commit(leases, session, proof.lease_id, reservation_id, now_ms),
            crate::CommitDecision::Committed
        );
        reservation_id
    }

    fn alloc_request(
        proof: LeaseProof,
        reservation_id: ReservationId,
        size: u64,
    ) -> RemoteBufferRequest {
        RemoteBufferRequest::Alloc {
            lease_id: proof.lease_id.into_bytes(),
            worker_incarnation_id: proof.incarnation.into_bytes(),
            reservation_id: reservation_id.into_bytes(),
            size_bytes: size,
            allocation_flags: AllocationFlags::EVICTABLE,
        }
    }

    #[test]
    fn e_gen_03_store_lifecycle_is_atomic_and_content_exact() {
        let now = 20_000;
        let (mut guard, mut store, proof, mut leases, session) = setup(now);
        let reservation =
            committed_reservation(&mut guard, &mut leases, &session, proof, 1, 8, now);
        assert_eq!(guard.held_bytes(), 8);
        let (_, alloc) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(1),
            alloc_request(proof, reservation, 8),
            now,
        );
        assert!(alloc.completed);
        assert_eq!(
            guard.held_bytes(),
            8,
            "reservation-to-buffer transfer is neutral"
        );
        let buffer_id = alloc.buffer.unwrap().buffer_id;

        let (_, pre_ready) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(1),
            RemoteBufferRequest::Get {
                lease_id: proof.lease_id.into_bytes(),
                worker_incarnation_id: proof.incarnation.into_bytes(),
                buffer_id,
                offset: 0,
                length: 8,
            },
            now,
        );
        assert_eq!(pre_ready.reason, BufferReason::BufferInvalidState);

        for (offset, data) in [(0, b"abcd".to_vec()), (4, b"EFGH".to_vec())] {
            let (_, put) = store.apply(
                &mut guard,
                proof,
                SessionBinding::test_only(1),
                RemoteBufferRequest::Put {
                    lease_id: proof.lease_id.into_bytes(),
                    worker_incarnation_id: proof.incarnation.into_bytes(),
                    buffer_id,
                    offset,
                    data,
                },
                now,
            );
            assert!(put.completed);
        }
        let (_, invalid_put) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(1),
            RemoteBufferRequest::Put {
                lease_id: proof.lease_id.into_bytes(),
                worker_incarnation_id: proof.incarnation.into_bytes(),
                buffer_id,
                offset: 7,
                data: b"bad".to_vec(),
            },
            now,
        );
        assert_eq!(invalid_put.reason, BufferReason::BufferRangeInvalid);

        let (_, get) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(1),
            RemoteBufferRequest::Get {
                lease_id: proof.lease_id.into_bytes(),
                worker_incarnation_id: proof.incarnation.into_bytes(),
                buffer_id,
                offset: 0,
                length: 8,
            },
            now,
        );
        assert_eq!(get.data, b"abcdEFGH");
        assert_eq!(get.buffer.unwrap().state, BufferState::Ready);

        let (_, stat) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(1),
            RemoteBufferRequest::Stat {
                lease_id: proof.lease_id.into_bytes(),
                worker_incarnation_id: proof.incarnation.into_bytes(),
                buffer_id,
            },
            now,
        );
        assert!(stat.data.is_empty());
        assert_eq!(stat.reservation_id, Some(reservation.into_bytes()));

        let free_request = RemoteBufferRequest::Free {
            lease_id: proof.lease_id.into_bytes(),
            worker_incarnation_id: proof.incarnation.into_bytes(),
            buffer_id,
        };
        let (_, free) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(1),
            free_request.clone(),
            now,
        );
        assert!(free.completed);
        assert_eq!(guard.held_bytes(), 0);
        let (_, replay) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(1),
            free_request,
            now,
        );
        assert!(replay.completed);
        assert_eq!(guard.held_bytes(), 0, "repeat FREE cannot double release");
    }

    #[test]
    fn consumed_reservation_session_loss_and_ttl_never_resurrect() {
        let now = 20_000;
        let (mut guard, mut store, proof, mut leases, session) = setup(now);
        let reservation =
            committed_reservation(&mut guard, &mut leases, &session, proof, 2, 4, now);
        let (_, alloc) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(9),
            alloc_request(proof, reservation, 4),
            now,
        );
        let buffer_id = alloc.buffer.unwrap().buffer_id;
        let (_, second_alloc) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(9),
            alloc_request(proof, reservation, 4),
            now,
        );
        assert_eq!(second_alloc.reason, BufferReason::ReservationInvalid);
        assert_eq!(guard.held_bytes(), 4);

        store.session_lost(&mut guard, SessionBinding::test_only(9), now + 1);
        assert_eq!(guard.held_bytes(), 0);
        let (_, lost) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(10),
            RemoteBufferRequest::Stat {
                lease_id: proof.lease_id.into_bytes(),
                worker_incarnation_id: proof.incarnation.into_bytes(),
                buffer_id,
            },
            now + 2,
        );
        assert_eq!(lost.reason, BufferReason::BufferLost);
        assert_eq!(lost.buffer.unwrap().state, BufferState::Lost);

        let other = committed_reservation(&mut guard, &mut leases, &session, proof, 3, 4, now + 2);
        let (_, other_alloc) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(10),
            alloc_request(proof, other, 4),
            now + 2,
        );
        let other_id = other_alloc.buffer.unwrap().buffer_id;
        store.session_lost(&mut guard, SessionBinding::test_only(9), now + 3);
        let (_, other_stat) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(10),
            RemoteBufferRequest::Stat {
                lease_id: proof.lease_id.into_bytes(),
                worker_incarnation_id: proof.incarnation.into_bytes(),
                buffer_id: other_id,
            },
            now + 3,
        );
        assert!(
            other_stat.completed,
            "unrelated session buffer remains live"
        );

        let (_, expired) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(10),
            RemoteBufferRequest::Stat {
                lease_id: proof.lease_id.into_bytes(),
                worker_incarnation_id: proof.incarnation.into_bytes(),
                buffer_id: other_id,
            },
            now + DEFAULT_BUFFER_TTL_MS + 3,
        );
        assert_eq!(expired.reason, BufferReason::BufferEvicted);
        assert_eq!(guard.held_bytes(), 0);
    }

    #[test]
    fn allocation_quotas_uncommitted_rejection_and_touch_absolute_cap_are_exact() {
        let now = 20_000;
        let (mut guard, mut store, proof, mut leases, session) = setup(now);
        let reserved = guard.reserve(
            &mut leases,
            &session,
            proof.lease_id,
            ReserveRequest {
                request_id: RequestId::from_u64(50),
                class: ResourceClass::Poc,
                bytes: 1,
            },
            now,
        );
        let ReservationDecision::Reserved { reservation_id } = reserved else {
            panic!("expected reserved")
        };
        let (_, uncommitted) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(1),
            alloc_request(proof, reservation_id, 1),
            now,
        );
        assert_eq!(uncommitted.reason, BufferReason::ReservationInvalid);
        assert_eq!(guard.held_bytes(), 1);
        assert_eq!(
            guard.commit(&mut leases, &session, proof.lease_id, reservation_id, now),
            crate::CommitDecision::Committed
        );

        let mut first_buffer = None;
        for index in 0..MAX_BUFFERS_PER_LEASE {
            let reservation = if index == 0 {
                reservation_id
            } else {
                committed_reservation(
                    &mut guard,
                    &mut leases,
                    &session,
                    proof,
                    50 + index as u64,
                    1,
                    now,
                )
            };
            let (_, alloc) = store.apply(
                &mut guard,
                proof,
                SessionBinding::test_only(1),
                alloc_request(proof, reservation, 1),
                now,
            );
            assert!(alloc.completed);
            first_buffer.get_or_insert(alloc.buffer.unwrap().buffer_id);
        }
        let ninth_reservation =
            committed_reservation(&mut guard, &mut leases, &session, proof, 99, 1, now);
        let (_, ninth) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(1),
            alloc_request(proof, ninth_reservation, 1),
            now,
        );
        assert_eq!(ninth.reason, BufferReason::ResourceExhausted);

        let buffer_id = first_buffer.unwrap();
        let mut touch = None;
        for elapsed in [
            250_000, 500_000, 750_000, 1_000_000, 1_250_000, 1_500_000, 1_750_000,
        ] {
            let (_, result) = store.apply(
                &mut guard,
                proof,
                SessionBinding::test_only(1),
                RemoteBufferRequest::Touch {
                    lease_id: proof.lease_id.into_bytes(),
                    worker_incarnation_id: proof.incarnation.into_bytes(),
                    buffer_id,
                },
                now + elapsed,
            );
            assert!(result.completed);
            touch = Some(result);
        }
        assert_eq!(
            touch.unwrap().buffer.unwrap().ttl_remaining_ms,
            50_000,
            "expiry is capped at created + 30 minutes"
        );

        let (_, zero) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(1),
            RemoteBufferRequest::Alloc {
                lease_id: proof.lease_id.into_bytes(),
                worker_incarnation_id: proof.incarnation.into_bytes(),
                reservation_id: [9; 16],
                size_bytes: 0,
                allocation_flags: AllocationFlags::NONE,
            },
            now,
        );
        assert_eq!(zero.reason, BufferReason::ResourceExhausted);
        let (_, oversized) = store.apply(
            &mut guard,
            proof,
            SessionBinding::test_only(1),
            RemoteBufferRequest::Alloc {
                lease_id: proof.lease_id.into_bytes(),
                worker_incarnation_id: proof.incarnation.into_bytes(),
                reservation_id: [9; 16],
                size_bytes: MAX_REMOTE_BUFFER_BYTES + 1,
                allocation_flags: AllocationFlags::NONE,
            },
            now,
        );
        assert_eq!(oversized.reason, BufferReason::ResourceExhausted);
    }
}
