use std::collections::{BTreeSet, VecDeque};

use crate::{WorkerIncarnationId, random_nonzero_128};

pub const LEASE_TTL_MS: u64 = 60_000;
pub const RECOMMENDED_RENEWAL_MS: u64 = 20_000;
const TERMINAL_CACHE_CAPACITY: usize = 256;
const PENDING_MUTATION_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LeaseId([u8; 16]);

impl LeaseId {
    pub fn is_nonzero(self) -> bool {
        self.0.iter().any(|byte| *byte != 0)
    }

    #[cfg(test)]
    pub(crate) const fn test_only(byte: u8) -> Self {
        Self([byte; 16])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseState {
    Free,
    Active,
    Revoking,
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseTransition {
    FreeToActive,
    ActiveToRevoking,
    RevokingToFree,
    ActiveToExpired,
    ExpiredToFree,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseError {
    ControllerBusy,
    StaleLease,
    EntropyUnavailable,
    Unauthenticated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalCode {
    Ok,
    RefusedSafety,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandAdmission {
    ExecuteOnce,
    Replay(TerminalCode),
    DuplicateResultEvicted,
    OutOfOrder { expected: u64 },
    AlreadyPending,
    PendingFull,
    StaleLease,
}

/// This capability deliberately has no constructor in product builds. C04-C06
/// will eventually own its creation after authenticated, pinned admission.
pub struct AuthenticatedSession {
    peer_id: u64,
    authenticated_and_pinned: bool,
}

impl AuthenticatedSession {
    #[cfg(test)]
    pub(crate) const fn test_only(peer_id: u64) -> Self {
        Self {
            peer_id,
            authenticated_and_pinned: true,
        }
    }

    #[cfg(test)]
    pub(crate) const fn test_unauthenticated(peer_id: u64) -> Self {
        Self {
            peer_id,
            authenticated_and_pinned: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LeaseProof {
    pub(crate) lease_id: LeaseId,
    pub(crate) incarnation: WorkerIncarnationId,
    pub(crate) peer_id: u64,
}

struct TerminalEntry {
    sequence: u64,
    code: TerminalCode,
}

struct ActiveLease {
    id: LeaseId,
    peer_id: u64,
    incarnation: WorkerIncarnationId,
    expires_at_ms: u64,
    next_command_sequence: u64,
    pending: BTreeSet<u64>,
    terminal: VecDeque<TerminalEntry>,
    evicted_through: Option<u64>,
}

/// Single-writer C07 authority. Callers must serialize mutable access.
pub struct ControllerLeaseManager {
    incarnation: WorkerIncarnationId,
    active: Option<ActiveLease>,
    state: LeaseState,
    last_transition: Option<LeaseTransition>,
    last_released: Option<(LeaseId, u64, WorkerIncarnationId)>,
}

impl ControllerLeaseManager {
    pub const fn new(incarnation: WorkerIncarnationId) -> Self {
        Self {
            incarnation,
            active: None,
            state: LeaseState::Free,
            last_transition: None,
            last_released: None,
        }
    }

    pub const fn state(&self) -> LeaseState {
        self.state
    }

    pub const fn last_transition(&self) -> Option<LeaseTransition> {
        self.last_transition
    }

    pub fn acquire(
        &mut self,
        session: &AuthenticatedSession,
        now_ms: u64,
    ) -> Result<LeaseId, LeaseError> {
        self.expire_if_needed(now_ms);
        if !session.authenticated_and_pinned {
            return Err(LeaseError::Unauthenticated);
        }
        if self.active.is_some() {
            return Err(LeaseError::ControllerBusy);
        }
        let id = LeaseId(random_nonzero_128().map_err(|_| LeaseError::EntropyUnavailable)?);
        self.active = Some(ActiveLease {
            id,
            peer_id: session.peer_id,
            incarnation: self.incarnation,
            expires_at_ms: now_ms.saturating_add(LEASE_TTL_MS),
            next_command_sequence: 0,
            pending: BTreeSet::new(),
            terminal: VecDeque::with_capacity(TERMINAL_CACHE_CAPACITY),
            evicted_through: None,
        });
        self.state = LeaseState::Active;
        self.last_transition = Some(LeaseTransition::FreeToActive);
        self.last_released = None;
        Ok(id)
    }

    pub fn renew(
        &mut self,
        session: &AuthenticatedSession,
        lease_id: LeaseId,
        now_ms: u64,
    ) -> Result<(), LeaseError> {
        let proof = self.validate(session, lease_id, now_ms)?;
        let active = self.active.as_mut().ok_or(LeaseError::StaleLease)?;
        if active.peer_id != proof.peer_id {
            return Err(LeaseError::StaleLease);
        }
        active.expires_at_ms = now_ms.saturating_add(LEASE_TTL_MS);
        Ok(())
    }

    pub fn release(
        &mut self,
        session: &AuthenticatedSession,
        lease_id: LeaseId,
        now_ms: u64,
    ) -> Result<(), LeaseError> {
        self.expire_if_needed(now_ms);
        if self.last_released == Some((lease_id, session.peer_id, self.incarnation)) {
            return Ok(());
        }
        self.validate(session, lease_id, now_ms)?;
        self.state = LeaseState::Revoking;
        self.last_transition = Some(LeaseTransition::ActiveToRevoking);
        self.active = None;
        self.last_released = Some((lease_id, session.peer_id, self.incarnation));
        self.state = LeaseState::Free;
        self.last_transition = Some(LeaseTransition::RevokingToFree);
        Ok(())
    }

    pub fn rotate_incarnation(&mut self, incarnation: WorkerIncarnationId) {
        if self.active.is_some() {
            self.state = LeaseState::Revoking;
            self.last_transition = Some(LeaseTransition::ActiveToRevoking);
            self.active = None;
            self.state = LeaseState::Free;
            self.last_transition = Some(LeaseTransition::RevokingToFree);
        }
        self.incarnation = incarnation;
        self.last_released = None;
    }

    pub fn begin_mutation(
        &mut self,
        session: &AuthenticatedSession,
        lease_id: LeaseId,
        sequence: u64,
        now_ms: u64,
    ) -> CommandAdmission {
        if self.validate(session, lease_id, now_ms).is_err() {
            return CommandAdmission::StaleLease;
        }
        let Some(active) = self.active.as_mut() else {
            return CommandAdmission::StaleLease;
        };
        if let Some(entry) = active
            .terminal
            .iter()
            .find(|entry| entry.sequence == sequence)
        {
            return CommandAdmission::Replay(entry.code);
        }
        if active
            .evicted_through
            .is_some_and(|evicted| sequence <= evicted)
        {
            return CommandAdmission::DuplicateResultEvicted;
        }
        if active.pending.contains(&sequence) {
            return CommandAdmission::AlreadyPending;
        }
        if sequence != active.next_command_sequence {
            return CommandAdmission::OutOfOrder {
                expected: active.next_command_sequence,
            };
        }
        if active.pending.len() >= PENDING_MUTATION_CAPACITY {
            return CommandAdmission::PendingFull;
        }
        active.pending.insert(sequence);
        active.next_command_sequence = active.next_command_sequence.saturating_add(1);
        CommandAdmission::ExecuteOnce
    }

    pub fn complete_mutation(
        &mut self,
        session: &AuthenticatedSession,
        lease_id: LeaseId,
        sequence: u64,
        code: TerminalCode,
        now_ms: u64,
    ) -> Result<(), LeaseError> {
        self.validate(session, lease_id, now_ms)?;
        let active = self.active.as_mut().ok_or(LeaseError::StaleLease)?;
        if !active.pending.remove(&sequence) {
            return Err(LeaseError::StaleLease);
        }
        active.terminal.push_back(TerminalEntry { sequence, code });
        if active.terminal.len() > TERMINAL_CACHE_CAPACITY
            && let Some(evicted) = active.terminal.pop_front()
        {
            active.evicted_through = Some(evicted.sequence);
        }
        Ok(())
    }

    pub(crate) fn validate(
        &mut self,
        session: &AuthenticatedSession,
        lease_id: LeaseId,
        now_ms: u64,
    ) -> Result<LeaseProof, LeaseError> {
        self.expire_if_needed(now_ms);
        if !session.authenticated_and_pinned {
            return Err(LeaseError::Unauthenticated);
        }
        let active = self.active.as_ref().ok_or(LeaseError::StaleLease)?;
        if active.id != lease_id
            || active.peer_id != session.peer_id
            || active.incarnation != self.incarnation
        {
            return Err(LeaseError::StaleLease);
        }
        Ok(LeaseProof {
            lease_id,
            incarnation: active.incarnation,
            peer_id: active.peer_id,
        })
    }

    fn expire_if_needed(&mut self, now_ms: u64) {
        let expired = self
            .active
            .as_ref()
            .is_some_and(|active| now_ms >= active.expires_at_ms);
        if expired {
            self.state = LeaseState::Expired;
            self.last_transition = Some(LeaseTransition::ActiveToExpired);
            self.active = None;
            self.state = LeaseState::Free;
            self.last_transition = Some(LeaseTransition::ExpiredToFree);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incarnation(byte: u8) -> WorkerIncarnationId {
        WorkerIncarnationId([byte; 16])
    }

    fn active() -> (ControllerLeaseManager, AuthenticatedSession, LeaseId) {
        let session = AuthenticatedSession::test_only(7);
        let mut manager = ControllerLeaseManager::new(incarnation(1));
        let lease = manager.acquire(&session, 1_000).expect("test lease");
        (manager, session, lease)
    }

    #[test]
    fn auth_t01_acquire_requires_authenticated_pinned_session() {
        let mut manager = ControllerLeaseManager::new(incarnation(1));
        let session = AuthenticatedSession::test_unauthenticated(7);
        assert_eq!(
            manager.acquire(&session, 0),
            Err(LeaseError::Unauthenticated)
        );
    }

    #[test]
    fn l_t03_second_peer_is_busy() {
        let (mut manager, _, _) = active();
        let second = AuthenticatedSession::test_only(8);
        assert_eq!(
            manager.acquire(&second, 1_001),
            Err(LeaseError::ControllerBusy)
        );
    }

    #[test]
    fn l_t02_lease_id_is_random_128_and_nonzero() {
        let (_, _, first) = active();
        let session = AuthenticatedSession::test_only(7);
        let mut manager = ControllerLeaseManager::new(incarnation(1));
        let second = manager.acquire(&session, 0).expect("second id");
        assert!(first.is_nonzero());
        assert_ne!(first, second);
    }

    #[test]
    fn l_t04_ttl_is_exactly_sixty_seconds_with_no_grace() {
        let (mut manager, session, lease) = active();
        assert!(manager.validate(&session, lease, 60_999).is_ok());
        assert_eq!(
            manager.validate(&session, lease, 61_000),
            Err(LeaseError::StaleLease)
        );
        assert_eq!(manager.state(), LeaseState::Free);
        assert_eq!(
            manager.last_transition(),
            Some(LeaseTransition::ExpiredToFree)
        );
    }

    #[test]
    fn l_t06_renewal_resets_full_ttl() {
        let (mut manager, session, lease) = active();
        manager.renew(&session, lease, 20_000).expect("renew");
        assert!(manager.validate(&session, lease, 79_999).is_ok());
        assert!(manager.validate(&session, lease, 80_000).is_err());
    }

    #[test]
    fn l_t08_new_lease_starts_sequence_zero_and_empty_cache() {
        let (mut manager, session, lease) = active();
        assert_eq!(
            manager.begin_mutation(&session, lease, 0, 1_001),
            CommandAdmission::ExecuteOnce
        );
    }

    #[test]
    fn l_t09_exact_next_executes_once_and_increments() {
        let (mut manager, session, lease) = active();
        assert_eq!(
            manager.begin_mutation(&session, lease, 0, 1_001),
            CommandAdmission::ExecuteOnce
        );
        assert_eq!(
            manager.begin_mutation(&session, lease, 1, 1_002),
            CommandAdmission::ExecuteOnce
        );
    }

    #[test]
    fn l_t10_duplicate_terminal_replays_without_execution() {
        let (mut manager, session, lease) = active();
        assert_eq!(
            manager.begin_mutation(&session, lease, 0, 1_001),
            CommandAdmission::ExecuteOnce
        );
        manager
            .complete_mutation(&session, lease, 0, TerminalCode::Ok, 1_002)
            .expect("complete");
        assert_eq!(
            manager.begin_mutation(&session, lease, 0, 1_003),
            CommandAdmission::Replay(TerminalCode::Ok)
        );
    }

    #[test]
    fn l_t11_old_evicted_duplicate_is_explicit() {
        let (mut manager, session, lease) = active();
        for sequence in 0..=256 {
            assert_eq!(
                manager.begin_mutation(&session, lease, sequence, 1_001),
                CommandAdmission::ExecuteOnce
            );
            manager
                .complete_mutation(&session, lease, sequence, TerminalCode::Ok, 1_001)
                .expect("complete");
        }
        assert_eq!(
            manager.begin_mutation(&session, lease, 0, 1_002),
            CommandAdmission::DuplicateResultEvicted
        );
    }

    #[test]
    fn l_t12_future_sequence_reports_expected() {
        let (mut manager, session, lease) = active();
        assert_eq!(
            manager.begin_mutation(&session, lease, 4, 1_001),
            CommandAdmission::OutOfOrder { expected: 0 }
        );
    }

    #[test]
    fn l_t13_wrong_peer_or_lease_is_stale() {
        let (mut manager, session, lease) = active();
        let other = AuthenticatedSession::test_only(8);
        assert_eq!(
            manager.begin_mutation(&other, lease, 0, 1_001),
            CommandAdmission::StaleLease
        );
        let wrong = LeaseId::test_only(9);
        assert_eq!(
            manager.begin_mutation(&session, wrong, 0, 1_001),
            CommandAdmission::StaleLease
        );
    }

    #[test]
    fn l_t05_release_is_idempotent_and_follows_revoking_to_free() {
        let (mut manager, session, lease) = active();
        manager.release(&session, lease, 1_001).expect("release");
        manager
            .release(&session, lease, 1_002)
            .expect("idempotent release");
        assert_eq!(manager.state(), LeaseState::Free);
        assert_eq!(
            manager.last_transition(),
            Some(LeaseTransition::RevokingToFree)
        );
    }

    #[test]
    fn l_t07_incarnation_rotation_invalidates_active_lease() {
        let (mut manager, session, lease) = active();
        manager.rotate_incarnation(incarnation(2));
        assert_eq!(
            manager.begin_mutation(&session, lease, 0, 1_001),
            CommandAdmission::StaleLease
        );
    }

    #[test]
    fn l_t15_pending_is_bounded() {
        let (mut manager, session, lease) = active();
        for sequence in 0..32 {
            assert_eq!(
                manager.begin_mutation(&session, lease, sequence, 1_001),
                CommandAdmission::ExecuteOnce
            );
        }
        assert_eq!(
            manager.begin_mutation(&session, lease, 32, 1_001),
            CommandAdmission::PendingFull
        );
    }

    #[test]
    fn reacquire_gets_new_id_sequence_and_cache() {
        let (mut manager, session, first) = active();
        assert_eq!(
            manager.begin_mutation(&session, first, 0, 1_001),
            CommandAdmission::ExecuteOnce
        );
        manager
            .complete_mutation(&session, first, 0, TerminalCode::Ok, 1_002)
            .expect("complete");
        manager.release(&session, first, 1_003).expect("release");
        let second = manager.acquire(&session, 1_004).expect("reacquire");
        assert_ne!(first, second);
        assert_eq!(
            manager.begin_mutation(&session, second, 0, 1_005),
            CommandAdmission::ExecuteOnce
        );
    }

    #[test]
    fn l_t14_cache_is_bounded_to_exactly_256_terminal_entries() {
        let (mut manager, session, lease) = active();
        for sequence in 0..=256 {
            assert_eq!(
                manager.begin_mutation(&session, lease, sequence, 1_001),
                CommandAdmission::ExecuteOnce
            );
            manager
                .complete_mutation(&session, lease, sequence, TerminalCode::Ok, 1_001)
                .expect("complete");
        }
        assert_eq!(manager.active.as_ref().expect("active").terminal.len(), 256);
    }

    #[test]
    fn l_t01_acquire_transitions_free_to_active() {
        let (manager, _, lease) = active();
        assert!(lease.is_nonzero());
        assert_eq!(manager.state(), LeaseState::Active);
        assert_eq!(
            manager.last_transition(),
            Some(LeaseTransition::FreeToActive)
        );
    }
}
