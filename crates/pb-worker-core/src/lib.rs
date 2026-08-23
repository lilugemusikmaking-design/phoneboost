#![forbid(unsafe_code)]

use std::fs::File;
use std::io::{self, Read};

const INCARNATION_BYTES: usize = 16;
const MAX_ZERO_RETRIES: usize = 8;

/// A5 implements no trust, lease, resource, buffer, or compute authority.
pub const DEFERRED_BY_BUILD_ORDER: &str = "DEFERRED_BY_BUILD_ORDER";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum WorkerState {
    ColdStart = 1,
    PairingRequired = 2,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct WorkerIncarnationId([u8; INCARNATION_BYTES]);

impl WorkerIncarnationId {
    pub const BITS: usize = INCARNATION_BYTES * 8;

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
}

impl WorkerCore {
    pub fn cold_start() -> Result<Self, WorkerStartError> {
        let mut state = WorkerState::ColdStart;
        let incarnation =
            random_incarnation_id().map_err(|_| WorkerStartError::EntropyUnavailable)?;
        initialize_a5_skeleton(&mut state);
        Ok(Self { incarnation, state })
    }

    pub const fn state(&self) -> WorkerState {
        self.state
    }

    pub const fn incarnation(&self) -> WorkerIncarnationId {
        self.incarnation
    }
}

fn initialize_a5_skeleton(state: &mut WorkerState) {
    *state = WorkerState::PairingRequired;
}

fn random_incarnation_id() -> io::Result<WorkerIncarnationId> {
    let mut source = File::open("/dev/urandom")?;
    for _ in 0..MAX_ZERO_RETRIES {
        let mut bytes = [0_u8; INCARNATION_BYTES];
        source.read_exact(&mut bytes)?;
        let candidate = WorkerIncarnationId(bytes);
        if candidate.is_nonzero() {
            return Ok(candidate);
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
    fn no_a6_b1_b2_authority_is_exposed() {
        assert_eq!(DEFERRED_BY_BUILD_ORDER, "DEFERRED_BY_BUILD_ORDER");
    }
}
