use std::fmt;

use pb_types::{Mutation, PAIRING_COOLDOWN_MS, PAIRING_MISMATCH_LIMIT, PairingState, ReasonCode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistOutcome {
    Succeeded,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PairingTransition {
    pub state: PairingState,
    pub send_pair_confirm: bool,
    pub persist_commit: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PairingError {
    pub reason: ReasonCode,
}

impl fmt::Display for PairingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.reason.as_str())
    }
}

impl std::error::Error for PairingError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairingActor {
    state: PairingState,
    local_confirmed: bool,
    peer_confirmed: bool,
    pair_confirm_sent: bool,
    commit_intent_emitted: bool,
    fresh_sas_display_required: bool,
}

impl Default for PairingActor {
    fn default() -> Self {
        Self::new()
    }
}

impl PairingActor {
    pub const fn new() -> Self {
        Self {
            state: PairingState::SasPending,
            local_confirmed: false,
            peer_confirmed: false,
            pair_confirm_sent: false,
            commit_intent_emitted: false,
            fresh_sas_display_required: true,
        }
    }

    pub const fn state(&self) -> PairingState {
        self.state
    }

    pub const fn local_confirmed(&self) -> bool {
        self.local_confirmed
    }

    pub const fn peer_confirmed(&self) -> bool {
        self.peer_confirmed
    }

    pub const fn fresh_sas_display_required(&self) -> bool {
        self.fresh_sas_display_required
    }

    fn confirmation_transition(
        &mut self,
        confirmation_changed: bool,
        send_pair_confirm: bool,
    ) -> Mutation<PairingTransition> {
        if self.commit_intent_emitted {
            return Mutation {
                value: PairingTransition {
                    state: self.state,
                    send_pair_confirm: false,
                    persist_commit: false,
                },
                state_changed: false,
            };
        }
        let prior_state = self.state;
        if self.local_confirmed && self.peer_confirmed {
            self.state = PairingState::MutualConfirmed;
        } else if self.local_confirmed && !self.peer_confirmed {
            self.state = PairingState::LocalConfirmed;
        } else if self.peer_confirmed && !self.local_confirmed {
            self.state = PairingState::PeerConfirmed;
        }
        Mutation {
            value: PairingTransition {
                state: self.state,
                send_pair_confirm,
                persist_commit: false,
            },
            state_changed: confirmation_changed || send_pair_confirm || prior_state != self.state,
        }
    }

    pub fn local_confirm(&mut self) -> Mutation<PairingTransition> {
        self.confirm_local_basis()
    }

    pub fn prior_committed_local_basis(
        &mut self,
        committed: &[u8; 32],
        presented: &[u8; 32],
    ) -> Mutation<PairingTransition> {
        if !prior_committed_key_matches(committed, presented) {
            return Mutation {
                value: PairingTransition {
                    state: self.state,
                    send_pair_confirm: false,
                    persist_commit: false,
                },
                state_changed: false,
            };
        }
        self.confirm_local_basis()
    }

    fn confirm_local_basis(&mut self) -> Mutation<PairingTransition> {
        let newly_confirmed = !self.local_confirmed;
        self.local_confirmed = true;
        let send = !self.pair_confirm_sent;
        self.pair_confirm_sent = true;
        self.confirmation_transition(newly_confirmed, send)
    }

    pub fn peer_confirm(&mut self) -> Mutation<PairingTransition> {
        let changed = !self.peer_confirmed;
        self.peer_confirmed = true;
        self.confirmation_transition(changed, false)
    }

    pub fn begin_trust_commit(&mut self) -> Mutation<PairingTransition> {
        let should_commit =
            self.state == PairingState::MutualConfirmed && !self.commit_intent_emitted;
        if should_commit {
            self.state = PairingState::TrustCommitting;
            self.commit_intent_emitted = true;
        }
        Mutation {
            value: PairingTransition {
                state: self.state,
                send_pair_confirm: false,
                persist_commit: should_commit,
            },
            state_changed: should_commit,
        }
    }

    pub fn persist_result(
        &mut self,
        outcome: PersistOutcome,
    ) -> Result<Mutation<PairingTransition>, PairingError> {
        if self.state != PairingState::TrustCommitting || !self.commit_intent_emitted {
            return Err(PairingError {
                reason: ReasonCode::PairPersistFailed,
            });
        }
        match outcome {
            PersistOutcome::Succeeded => {
                self.state = PairingState::Paired;
                Ok(Mutation {
                    value: PairingTransition {
                        state: self.state,
                        send_pair_confirm: false,
                        persist_commit: false,
                    },
                    state_changed: true,
                })
            }
            PersistOutcome::Failed => {
                self.state = PairingState::PairingFailed;
                Err(PairingError {
                    reason: ReasonCode::PairPersistFailed,
                })
            }
        }
    }
}

pub fn prior_committed_key_matches(committed: &[u8; 32], presented: &[u8; 32]) -> bool {
    committed == presented
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PairingGuard {
    pub mismatch_count: u8,
    pub cooldown_until_wall_ms: Option<u64>,
    pub updated_wall_ms: u64,
}

impl PairingGuard {
    pub const fn new(now_ms: u64) -> Self {
        Self {
            mismatch_count: 0,
            cooldown_until_wall_ms: None,
            updated_wall_ms: now_ms,
        }
    }

    pub fn admit(&mut self, now_ms: u64) -> Mutation<bool> {
        let Some(until) = self.cooldown_until_wall_ms else {
            return Mutation {
                value: true,
                state_changed: false,
            };
        };
        if now_ms < self.updated_wall_ms {
            self.cooldown_until_wall_ms = Some(now_ms.saturating_add(PAIRING_COOLDOWN_MS));
            self.updated_wall_ms = now_ms;
            return Mutation {
                value: false,
                state_changed: true,
            };
        }
        if now_ms < until {
            return Mutation {
                value: false,
                state_changed: false,
            };
        }
        self.mismatch_count = 0;
        self.cooldown_until_wall_ms = None;
        self.updated_wall_ms = now_ms;
        Mutation {
            value: true,
            state_changed: true,
        }
    }

    pub fn record_mismatch(&mut self, now_ms: u64) -> Mutation<()> {
        self.mismatch_count = self.mismatch_count.saturating_add(1);
        self.updated_wall_ms = now_ms;
        if self.mismatch_count >= PAIRING_MISMATCH_LIMIT {
            self.cooldown_until_wall_ms = Some(now_ms.saturating_add(PAIRING_COOLDOWN_MS));
        }
        Mutation {
            value: (),
            state_changed: true,
        }
    }

    pub const fn user_cancelled(&self) -> Mutation<()> {
        Mutation {
            value: (),
            state_changed: false,
        }
    }

    pub fn paired(&mut self, now_ms: u64) -> Mutation<()> {
        let changed = self.mismatch_count != 0 || self.cooldown_until_wall_ms.is_some();
        self.mismatch_count = 0;
        self.cooldown_until_wall_ms = None;
        self.updated_wall_ms = now_ms;
        Mutation {
            value: (),
            state_changed: changed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutual_confirmation_is_observable_before_commit_and_persists_once() {
        let mut actor = PairingActor::new();
        assert_eq!(actor.state(), PairingState::SasPending);
        let peer = actor.peer_confirm();
        assert!(peer.state_changed);
        assert!(!peer.value.persist_commit);
        assert_eq!(peer.value.state, PairingState::PeerConfirmed);
        let local = actor.local_confirm();
        assert!(local.value.send_pair_confirm);
        assert!(!local.value.persist_commit);
        assert_eq!(local.value.state, PairingState::MutualConfirmed);
        let duplicate = actor.peer_confirm();
        assert!(!duplicate.state_changed);
        assert!(!duplicate.value.persist_commit);
        assert_eq!(duplicate.value.state, PairingState::MutualConfirmed);
        let commit = actor.begin_trust_commit();
        assert!(commit.state_changed);
        assert!(commit.value.persist_commit);
        assert_eq!(commit.value.state, PairingState::TrustCommitting);
        let duplicate_commit = actor.begin_trust_commit();
        assert!(!duplicate_commit.state_changed);
        assert!(!duplicate_commit.value.persist_commit);
        let late_duplicate = actor.peer_confirm();
        assert!(!late_duplicate.state_changed);
        assert_eq!(late_duplicate.value.state, PairingState::TrustCommitting);
        actor.persist_result(PersistOutcome::Succeeded).unwrap();
        assert_eq!(actor.state(), PairingState::Paired);
    }

    #[test]
    fn failed_persist_never_pairs() {
        let mut actor = PairingActor::new();
        actor.local_confirm();
        actor.peer_confirm();
        actor.begin_trust_commit();
        let error = actor.persist_result(PersistOutcome::Failed).unwrap_err();
        assert_eq!(error.reason, ReasonCode::PairPersistFailed);
        assert_ne!(actor.state(), PairingState::Paired);
    }

    #[test]
    fn prior_committed_recovery_sends_once_only_for_exact_key() {
        assert!(prior_committed_key_matches(&[1; 32], &[1; 32]));
        assert!(!prior_committed_key_matches(&[1; 32], &[2; 32]));
        let mut actor = PairingActor::new();
        let mismatch = actor.prior_committed_local_basis(&[1; 32], &[2; 32]);
        assert!(!mismatch.state_changed);
        assert!(!mismatch.value.send_pair_confirm);
        assert_eq!(mismatch.value.state, PairingState::SasPending);
        let exact = actor.prior_committed_local_basis(&[1; 32], &[1; 32]);
        assert!(exact.value.send_pair_confirm);
        assert_eq!(exact.value.state, PairingState::LocalConfirmed);
        let duplicate = actor.prior_committed_local_basis(&[1; 32], &[1; 32]);
        assert!(!duplicate.state_changed);
        assert!(!duplicate.value.send_pair_confirm);
        assert!(actor.local_confirmed());
        assert!(actor.fresh_sas_display_required());
    }

    #[test]
    fn cooldown_is_global_durable_and_conservative() {
        let base = 1_700_000_000_000;
        let mut guard = PairingGuard::new(base);
        for offset in 0..3 {
            guard.record_mismatch(base + offset);
        }
        assert_eq!(guard.mismatch_count, 3);
        assert!(!guard.admit(base + 3).value);
        let persisted = guard;
        let mut reloaded = persisted;
        assert!(!reloaded.admit(base + 4).value);
        let backward = reloaded.admit(base - 1);
        assert!(!backward.value);
        assert!(backward.state_changed);
        assert_eq!(
            reloaded.cooldown_until_wall_ms,
            Some(base - 1 + PAIRING_COOLDOWN_MS)
        );
        assert!(!reloaded.user_cancelled().state_changed);
    }
}
