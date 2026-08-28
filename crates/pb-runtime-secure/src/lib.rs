#![forbid(unsafe_code)]

mod runtime;
mod storage;
mod wire;

pub use pb_pbmux::{
    AckPayload, BufferResult, CommandPayload, RemoteBufferRequest, RemoteBufferResponseKind,
    ResourceRequest, ResourceResponseKind, ResourceResult,
};
pub use runtime::{
    AuthenticatedCommandHandler, AuthenticatedCommandHandlerError, EndpointRole,
    PairingActionResult, RuntimeError, RuntimeSnapshot, RuntimeState, SecureRuntime,
    SessionOutcome, VerifiedPeerSession, VerifiedSessionId, run_initiator_session,
    run_responder_session, run_responder_session_with_handler,
};
pub use storage::{Identity, PeerRecord, StateStore, StorageError};
pub use wire::{RECORD_PREFIX_BYTES, SecureWireError};
