use std::collections::{BTreeSet, VecDeque};

use crate::{WorkerIncarnationId, random_nonzero_128};
use pb_runtime_secure::VerifiedPeerSession;
use pb_types::PeerId;

pub const LEASE_TTL_MS: u64 = 60_000;
pub const RECOMMENDED_RENEWAL_MS: u64 = 20_000;
const TERMINAL_CACHE_CAPACITY: usize = 256;
const PENDING_MUTATION_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LeaseId([u8; 16]);

impl LeaseId {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn into_bytes(self) -> [u8; 16] {
        self.0
    }

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
pub(crate) enum LeaseError {
    #[cfg(test)]
    ControllerBusy,
    StaleLease,
    #[cfg(test)]
    EntropyUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TerminalCode {
    Ok,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandAdmission {
    ExecuteOnce,
    Replay(TerminalCode),
    DuplicateResultEvicted,
    OutOfOrder { expected: u64 },
    AlreadyPending,
    PendingFull,
    StaleLease,
    SequenceExhausted,
}

/// Internal worker view of identity proven by a `VerifiedPeerSession`.
/// It has no production constructor from a bare `PeerId` or boolean.
pub struct AuthenticatedSession {
    peer_id: PeerId,
}

impl AuthenticatedSession {
    pub(crate) fn from_verified(verified_session: &VerifiedPeerSession<'_>) -> Self {
        Self {
            peer_id: *verified_session.peer_id(),
        }
    }

    #[cfg(test)]
    pub(crate) const fn test_only(peer_id: PeerId) -> Self {
        Self { peer_id }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerCommand {
    Acquire,
    Renew { lease_id: LeaseId, command_seq: u64 },
    Release { lease_id: LeaseId, command_seq: u64 },
    Unsupported { command_seq: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerFailureReason {
    ControllerBusy,
    StaleControllerLease,
    OutOfOrder,
    DuplicateResultEvicted,
    UnsupportedMessage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControllerLeaseRef {
    pub lease_id: LeaseId,
    pub worker_incarnation_id: WorkerIncarnationId,
    pub ttl_remaining_ms: u32,
    pub next_command_seq: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerCommandResult {
    Completed {
        command_seq: u64,
        lease: Option<ControllerLeaseRef>,
    },
    Failed {
        command_seq: u64,
        reason: ControllerFailureReason,
        expected_next_seq: Option<u64>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerCommandError {
    EntropyUnavailable,
    InternalInvariant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LeaseProof {
    pub(crate) lease_id: LeaseId,
    pub(crate) incarnation: WorkerIncarnationId,
    pub(crate) peer_id: PeerId,
}

#[derive(Clone, Copy)]
struct TerminalEntry {
    sequence: u64,
    code: TerminalCode,
    lease: Option<ControllerLeaseRef>,
}

struct ActiveLease {
    id: LeaseId,
    peer_id: PeerId,
    incarnation: WorkerIncarnationId,
    acquired_at_ms: u64,
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
    released: Option<ActiveLease>,
}

impl ControllerLeaseManager {
    pub const fn new(incarnation: WorkerIncarnationId) -> Self {
        Self {
            incarnation,
            active: None,
            state: LeaseState::Free,
            last_transition: None,
            released: None,
        }
    }

    pub const fn state(&self) -> LeaseState {
        self.state
    }

    pub const fn last_transition(&self) -> Option<LeaseTransition> {
        self.last_transition
    }

    pub(crate) fn apply_controller_command(
        &mut self,
        session: &AuthenticatedSession,
        command: ControllerCommand,
        now_ms: u64,
    ) -> Result<ControllerCommandResult, ControllerCommandError> {
        match command {
            ControllerCommand::Acquire => self.apply_acquire(session, now_ms),
            ControllerCommand::Renew {
                lease_id,
                command_seq,
            } => self.apply_renew(session, lease_id, command_seq, now_ms),
            ControllerCommand::Release {
                lease_id,
                command_seq,
            } => self.apply_release(session, lease_id, command_seq, now_ms),
            ControllerCommand::Unsupported { command_seq } => Ok(ControllerCommandResult::Failed {
                command_seq,
                reason: ControllerFailureReason::UnsupportedMessage,
                expected_next_seq: None,
            }),
        }
    }

    fn apply_acquire(
        &mut self,
        session: &AuthenticatedSession,
        now_ms: u64,
    ) -> Result<ControllerCommandResult, ControllerCommandError> {
        self.expire_if_needed(now_ms);
        if let Some(active) = self.active.as_ref() {
            return Ok(if active.peer_id == session.peer_id {
                ControllerCommandResult::Completed {
                    command_seq: 0,
                    lease: Some(Self::lease_ref(active, now_ms)),
                }
            } else {
                ControllerCommandResult::Failed {
                    command_seq: 0,
                    reason: ControllerFailureReason::ControllerBusy,
                    expected_next_seq: None,
                }
            });
        }
        let id =
            LeaseId(random_nonzero_128().map_err(|_| ControllerCommandError::EntropyUnavailable)?);
        self.active = Some(ActiveLease {
            id,
            peer_id: session.peer_id,
            incarnation: self.incarnation,
            acquired_at_ms: now_ms,
            expires_at_ms: now_ms.saturating_add(LEASE_TTL_MS),
            next_command_sequence: 0,
            pending: BTreeSet::new(),
            terminal: VecDeque::with_capacity(TERMINAL_CACHE_CAPACITY),
            evicted_through: None,
        });
        self.released = None;
        self.state = LeaseState::Active;
        self.last_transition = Some(LeaseTransition::FreeToActive);
        Ok(ControllerCommandResult::Completed {
            command_seq: 0,
            lease: Some(Self::lease_ref(
                self.active.as_ref().expect("new active lease"),
                now_ms,
            )),
        })
    }

    fn apply_renew(
        &mut self,
        session: &AuthenticatedSession,
        lease_id: LeaseId,
        command_seq: u64,
        now_ms: u64,
    ) -> Result<ControllerCommandResult, ControllerCommandError> {
        self.expire_if_needed(now_ms);
        if self.validate(session, lease_id, now_ms).is_err() {
            return Ok(self.released_admission(session, lease_id, command_seq));
        }
        match self.begin_mutation_internal(session, lease_id, command_seq, now_ms) {
            CommandAdmission::ExecuteOnce => {
                let active = self
                    .active
                    .as_mut()
                    .ok_or(ControllerCommandError::InternalInvariant)?;
                active.expires_at_ms = now_ms.saturating_add(LEASE_TTL_MS);
                let lease = Self::lease_ref(active, now_ms);
                Self::complete_active(active, command_seq, Some(lease))?;
                Ok(ControllerCommandResult::Completed {
                    command_seq,
                    lease: Some(lease),
                })
            }
            admission => self.result_from_admission(admission, command_seq),
        }
    }

    fn apply_release(
        &mut self,
        session: &AuthenticatedSession,
        lease_id: LeaseId,
        command_seq: u64,
        now_ms: u64,
    ) -> Result<ControllerCommandResult, ControllerCommandError> {
        self.expire_if_needed(now_ms);
        if self.validate(session, lease_id, now_ms).is_err() {
            return Ok(self.released_admission(session, lease_id, command_seq));
        }
        match self.begin_mutation_internal(session, lease_id, command_seq, now_ms) {
            CommandAdmission::ExecuteOnce => {
                let active = self
                    .active
                    .as_mut()
                    .ok_or(ControllerCommandError::InternalInvariant)?;
                Self::complete_active(active, command_seq, None)?;
                self.state = LeaseState::Revoking;
                self.last_transition = Some(LeaseTransition::ActiveToRevoking);
                self.released = self.active.take();
                self.state = LeaseState::Free;
                self.last_transition = Some(LeaseTransition::RevokingToFree);
                Ok(ControllerCommandResult::Completed {
                    command_seq,
                    lease: None,
                })
            }
            admission => self.result_from_admission(admission, command_seq),
        }
    }

    fn released_admission(
        &self,
        session: &AuthenticatedSession,
        lease_id: LeaseId,
        command_seq: u64,
    ) -> ControllerCommandResult {
        let Some(released) = self.released.as_ref().filter(|released| {
            released.id == lease_id
                && released.peer_id == session.peer_id
                && released.incarnation == self.incarnation
        }) else {
            return Self::failed_stale(command_seq);
        };
        if let Some(entry) = released
            .terminal
            .iter()
            .find(|entry| entry.sequence == command_seq)
        {
            return Self::terminal_result(*entry);
        }
        if released
            .evicted_through
            .is_some_and(|evicted| command_seq <= evicted)
        {
            return ControllerCommandResult::Failed {
                command_seq,
                reason: ControllerFailureReason::DuplicateResultEvicted,
                expected_next_seq: None,
            };
        }
        Self::failed_stale(command_seq)
    }

    fn result_from_admission(
        &self,
        admission: CommandAdmission,
        command_seq: u64,
    ) -> Result<ControllerCommandResult, ControllerCommandError> {
        Ok(match admission {
            CommandAdmission::Replay(code) => {
                let active = self
                    .active
                    .as_ref()
                    .ok_or(ControllerCommandError::InternalInvariant)?;
                let entry = active
                    .terminal
                    .iter()
                    .find(|entry| entry.sequence == command_seq && entry.code == code)
                    .copied()
                    .ok_or(ControllerCommandError::InternalInvariant)?;
                Self::terminal_result(entry)
            }
            CommandAdmission::DuplicateResultEvicted => ControllerCommandResult::Failed {
                command_seq,
                reason: ControllerFailureReason::DuplicateResultEvicted,
                expected_next_seq: None,
            },
            CommandAdmission::OutOfOrder { expected } => ControllerCommandResult::Failed {
                command_seq,
                reason: ControllerFailureReason::OutOfOrder,
                expected_next_seq: Some(expected),
            },
            CommandAdmission::StaleLease => Self::failed_stale(command_seq),
            CommandAdmission::AlreadyPending
            | CommandAdmission::PendingFull
            | CommandAdmission::SequenceExhausted
            | CommandAdmission::ExecuteOnce => {
                return Err(ControllerCommandError::InternalInvariant);
            }
        })
    }

    fn terminal_result(entry: TerminalEntry) -> ControllerCommandResult {
        match entry.code {
            TerminalCode::Ok => ControllerCommandResult::Completed {
                command_seq: entry.sequence,
                lease: entry.lease,
            },
        }
    }

    fn failed_stale(command_seq: u64) -> ControllerCommandResult {
        ControllerCommandResult::Failed {
            command_seq,
            reason: ControllerFailureReason::StaleControllerLease,
            expected_next_seq: None,
        }
    }

    fn lease_ref(active: &ActiveLease, now_ms: u64) -> ControllerLeaseRef {
        debug_assert!(active.expires_at_ms >= active.acquired_at_ms);
        ControllerLeaseRef {
            lease_id: active.id,
            worker_incarnation_id: active.incarnation,
            ttl_remaining_ms: active.expires_at_ms.saturating_sub(now_ms) as u32,
            next_command_seq: active.next_command_sequence,
        }
    }

    fn complete_active(
        active: &mut ActiveLease,
        sequence: u64,
        lease: Option<ControllerLeaseRef>,
    ) -> Result<(), ControllerCommandError> {
        if !active.pending.remove(&sequence) {
            return Err(ControllerCommandError::InternalInvariant);
        }
        active.terminal.push_back(TerminalEntry {
            sequence,
            code: TerminalCode::Ok,
            lease,
        });
        Self::trim_terminal_cache(active);
        Ok(())
    }

    fn trim_terminal_cache(active: &mut ActiveLease) {
        if active.terminal.len() > TERMINAL_CACHE_CAPACITY
            && let Some(evicted) = active.terminal.pop_front()
        {
            active.evicted_through = Some(evicted.sequence);
        }
    }

    #[cfg(test)]
    pub(crate) fn acquire(
        &mut self,
        session: &AuthenticatedSession,
        now_ms: u64,
    ) -> Result<LeaseId, LeaseError> {
        match self
            .apply_acquire(session, now_ms)
            .map_err(|_| LeaseError::EntropyUnavailable)?
        {
            ControllerCommandResult::Completed {
                lease: Some(lease), ..
            } => Ok(lease.lease_id),
            ControllerCommandResult::Failed {
                reason: ControllerFailureReason::ControllerBusy,
                ..
            } => Err(LeaseError::ControllerBusy),
            _ => Err(LeaseError::StaleLease),
        }
    }

    #[cfg(test)]
    pub(crate) fn renew(
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

    #[cfg(test)]
    pub(crate) fn release(
        &mut self,
        session: &AuthenticatedSession,
        lease_id: LeaseId,
        now_ms: u64,
    ) -> Result<(), LeaseError> {
        self.expire_if_needed(now_ms);
        if self.released.as_ref().is_some_and(|released| {
            released.id == lease_id
                && released.peer_id == session.peer_id
                && released.incarnation == self.incarnation
        }) {
            return Ok(());
        }
        self.validate(session, lease_id, now_ms)?;
        self.state = LeaseState::Revoking;
        self.last_transition = Some(LeaseTransition::ActiveToRevoking);
        self.released = self.active.take();
        self.state = LeaseState::Free;
        self.last_transition = Some(LeaseTransition::RevokingToFree);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn rotate_incarnation(&mut self, incarnation: WorkerIncarnationId) {
        if self.active.is_some() {
            self.state = LeaseState::Revoking;
            self.last_transition = Some(LeaseTransition::ActiveToRevoking);
            self.active = None;
            self.state = LeaseState::Free;
            self.last_transition = Some(LeaseTransition::RevokingToFree);
        }
        self.incarnation = incarnation;
        self.released = None;
    }

    #[cfg(test)]
    pub(crate) fn begin_mutation(
        &mut self,
        session: &AuthenticatedSession,
        lease_id: LeaseId,
        sequence: u64,
        now_ms: u64,
    ) -> CommandAdmission {
        self.begin_mutation_internal(session, lease_id, sequence, now_ms)
    }

    fn begin_mutation_internal(
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
        let Some(next) = active.next_command_sequence.checked_add(1) else {
            return CommandAdmission::SequenceExhausted;
        };
        active.pending.insert(sequence);
        active.next_command_sequence = next;
        CommandAdmission::ExecuteOnce
    }

    #[cfg(test)]
    pub(crate) fn complete_mutation(
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
        let lease = Some(Self::lease_ref(active, now_ms));
        active.terminal.push_back(TerminalEntry {
            sequence,
            code,
            lease,
        });
        Self::trim_terminal_cache(active);
        Ok(())
    }

    pub(crate) fn validate(
        &mut self,
        session: &AuthenticatedSession,
        lease_id: LeaseId,
        now_ms: u64,
    ) -> Result<LeaseProof, LeaseError> {
        self.expire_if_needed(now_ms);
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
    use pb_types::PeerId;

    fn incarnation(byte: u8) -> WorkerIncarnationId {
        WorkerIncarnationId([byte; 16])
    }

    fn peer_id(byte: u8) -> PeerId {
        PeerId::from_sha256_digest([byte; 32])
    }

    fn active() -> (ControllerLeaseManager, AuthenticatedSession, LeaseId) {
        let session = AuthenticatedSession::test_only(peer_id(7));
        let mut manager = ControllerLeaseManager::new(incarnation(1));
        let lease = manager.acquire(&session, 1_000).expect("test lease");
        (manager, session, lease)
    }

    fn completed_lease(result: ControllerCommandResult) -> ControllerLeaseRef {
        match result {
            ControllerCommandResult::Completed {
                lease: Some(lease), ..
            } => lease,
            other => panic!("expected completed lease result, got {other:?}"),
        }
    }

    #[test]
    fn production_acquire_is_same_peer_idempotent_and_other_peer_busy() {
        let first_session = AuthenticatedSession::test_only(peer_id(7));
        let reconnect_session = AuthenticatedSession::test_only(peer_id(7));
        let other_session = AuthenticatedSession::test_only(peer_id(8));
        let mut manager = ControllerLeaseManager::new(incarnation(1));

        let first = completed_lease(
            manager
                .apply_controller_command(&first_session, ControllerCommand::Acquire, 1_000)
                .unwrap(),
        );
        let same_peer = completed_lease(
            manager
                .apply_controller_command(&reconnect_session, ControllerCommand::Acquire, 2_000)
                .unwrap(),
        );
        assert_eq!(same_peer.lease_id, first.lease_id);
        assert_eq!(same_peer.next_command_seq, 0);
        assert_eq!(same_peer.ttl_remaining_ms, 59_000);
        assert_eq!(manager.active.as_ref().unwrap().acquired_at_ms, 1_000);
        assert_eq!(manager.active.as_ref().unwrap().expires_at_ms, 61_000);
        assert_eq!(
            manager
                .apply_controller_command(&other_session, ControllerCommand::Acquire, 2_001)
                .unwrap(),
            ControllerCommandResult::Failed {
                command_seq: 0,
                reason: ControllerFailureReason::ControllerBusy,
                expected_next_seq: None,
            }
        );
    }

    #[test]
    fn production_sequence_is_atomic_replays_and_reports_exact_failures() {
        let session = AuthenticatedSession::test_only(peer_id(7));
        let mut manager = ControllerLeaseManager::new(incarnation(1));
        let lease = completed_lease(
            manager
                .apply_controller_command(&session, ControllerCommand::Acquire, 1_000)
                .unwrap(),
        )
        .lease_id;

        let renew = ControllerCommand::Renew {
            lease_id: lease,
            command_seq: 0,
        };
        let first = manager
            .apply_controller_command(&session, renew, 2_000)
            .unwrap();
        let expiry_after_first = manager.active.as_ref().unwrap().expires_at_ms;
        let replay = manager
            .apply_controller_command(&session, renew, 3_000)
            .unwrap();
        assert!(matches!(first, ControllerCommandResult::Completed { .. }));
        assert!(matches!(replay, ControllerCommandResult::Completed { .. }));
        assert_eq!(
            manager.active.as_ref().unwrap().expires_at_ms,
            expiry_after_first
        );
        assert_eq!(manager.active.as_ref().unwrap().next_command_sequence, 1);
        assert_eq!(
            manager
                .apply_controller_command(
                    &session,
                    ControllerCommand::Renew {
                        lease_id: lease,
                        command_seq: 4,
                    },
                    3_001,
                )
                .unwrap(),
            ControllerCommandResult::Failed {
                command_seq: 4,
                reason: ControllerFailureReason::OutOfOrder,
                expected_next_seq: Some(1),
            }
        );

        for sequence in 1..=256 {
            assert!(matches!(
                manager
                    .apply_controller_command(
                        &session,
                        ControllerCommand::Renew {
                            lease_id: lease,
                            command_seq: sequence,
                        },
                        3_001,
                    )
                    .unwrap(),
                ControllerCommandResult::Completed { .. }
            ));
        }
        assert_eq!(
            manager
                .apply_controller_command(&session, renew, 3_002)
                .unwrap(),
            ControllerCommandResult::Failed {
                command_seq: 0,
                reason: ControllerFailureReason::DuplicateResultEvicted,
                expected_next_seq: None,
            }
        );

        let release = ControllerCommand::Release {
            lease_id: lease,
            command_seq: 257,
        };
        let released = manager
            .apply_controller_command(&session, release, 3_003)
            .unwrap();
        assert_eq!(
            released,
            ControllerCommandResult::Completed {
                command_seq: 257,
                lease: None,
            }
        );
        assert_eq!(
            manager
                .apply_controller_command(&session, release, 3_004)
                .unwrap(),
            released
        );
        assert_eq!(manager.released.as_ref().unwrap().terminal.len(), 256);
    }

    #[test]
    fn production_release_terminal_survives_lost_ack_without_double_mutation() {
        let session = AuthenticatedSession::test_only(peer_id(7));
        let mut manager = ControllerLeaseManager::new(incarnation(1));
        let lease = completed_lease(
            manager
                .apply_controller_command(&session, ControllerCommand::Acquire, 1_000)
                .unwrap(),
        )
        .lease_id;
        let release = ControllerCommand::Release {
            lease_id: lease,
            command_seq: 0,
        };

        let first = manager
            .apply_controller_command(&session, release, 2_000)
            .unwrap();
        let transition = manager.last_transition();
        let replay = manager
            .apply_controller_command(&session, release, 3_000)
            .unwrap();
        assert_eq!(
            first,
            ControllerCommandResult::Completed {
                command_seq: 0,
                lease: None,
            }
        );
        assert_eq!(replay, first);
        assert_eq!(manager.state(), LeaseState::Free);
        assert!(manager.active.is_none());
        assert_eq!(manager.last_transition(), transition);
        assert_eq!(manager.released.as_ref().unwrap().terminal.len(), 1);
        assert_eq!(manager.released.as_ref().unwrap().next_command_sequence, 1);
    }

    #[test]
    fn reconnect_expiry_and_incarnation_rules_are_fail_closed() {
        let first_session = AuthenticatedSession::test_only(peer_id(7));
        let reconnect_session = AuthenticatedSession::test_only(peer_id(7));
        let mut manager = ControllerLeaseManager::new(incarnation(1));
        let lease = completed_lease(
            manager
                .apply_controller_command(&first_session, ControllerCommand::Acquire, 0)
                .unwrap(),
        )
        .lease_id;
        assert!(matches!(
            manager
                .apply_controller_command(
                    &reconnect_session,
                    ControllerCommand::Renew {
                        lease_id: lease,
                        command_seq: 0,
                    },
                    20_000,
                )
                .unwrap(),
            ControllerCommandResult::Completed { .. }
        ));
        assert_eq!(
            manager
                .apply_controller_command(
                    &reconnect_session,
                    ControllerCommand::Renew {
                        lease_id: lease,
                        command_seq: 1,
                    },
                    80_000,
                )
                .unwrap(),
            ControllerCommandResult::Failed {
                command_seq: 1,
                reason: ControllerFailureReason::StaleControllerLease,
                expected_next_seq: None,
            }
        );

        let mut manager = ControllerLeaseManager::new(incarnation(1));
        let lease = completed_lease(
            manager
                .apply_controller_command(&first_session, ControllerCommand::Acquire, 0)
                .unwrap(),
        )
        .lease_id;
        manager.rotate_incarnation(incarnation(2));
        assert_eq!(
            manager
                .apply_controller_command(
                    &reconnect_session,
                    ControllerCommand::Renew {
                        lease_id: lease,
                        command_seq: 0,
                    },
                    1,
                )
                .unwrap(),
            ControllerCommandResult::Failed {
                command_seq: 0,
                reason: ControllerFailureReason::StaleControllerLease,
                expected_next_seq: None,
            }
        );
    }

    #[test]
    fn l_t03_second_peer_is_busy() {
        let (mut manager, _, _) = active();
        let second = AuthenticatedSession::test_only(peer_id(8));
        assert_eq!(
            manager.acquire(&second, 1_001),
            Err(LeaseError::ControllerBusy)
        );
    }

    #[test]
    fn l_t02_lease_id_is_random_128_and_nonzero() {
        let (_, _, first) = active();
        let session = AuthenticatedSession::test_only(peer_id(7));
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
        let other = AuthenticatedSession::test_only(peer_id(8));
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
