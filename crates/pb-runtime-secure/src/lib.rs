#![forbid(unsafe_code)]

mod runtime;
mod storage;
mod wire;

pub use pb_pbmux::{AckPayload, CommandPayload};
pub use runtime::{
    AuthenticatedCommandHandler, AuthenticatedCommandHandlerError, EndpointRole,
    PairingActionResult, RuntimeError, RuntimeSnapshot, RuntimeState, SecureRuntime,
    SessionOutcome, VerifiedPeerSession, run_initiator_session, run_responder_session,
    run_responder_session_with_handler,
};
pub use storage::{Identity, PeerRecord, StateStore, StorageError};
pub use wire::{RECORD_PREFIX_BYTES, SecureWireError};
