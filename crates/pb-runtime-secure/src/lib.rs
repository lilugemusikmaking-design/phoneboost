#![forbid(unsafe_code)]

mod initiator;
mod runtime;
mod storage;
mod wire;

pub use initiator::{
    InitiatorClientError, InitiatorSessionClient, InitiatorSessionDriver, InitiatorSessionSnapshot,
    initiator_session_channel,
};
pub use pb_pbmux::{
    AckPayload, BufferResult, CommandPayload, ComputeRequest, ComputeResponse, RemoteBufferRequest,
    RemoteBufferResponseKind, ResourceRequest, ResourceResponseKind, ResourceResult,
};
pub use runtime::{
    AuthenticatedCommandHandler, AuthenticatedCommandHandlerError, EndpointRole,
    PairingActionResult, RuntimeError, RuntimeSnapshot, RuntimeState, SecureRuntime,
    SessionOutcome, VerifiedPeerSession, VerifiedSessionId, run_initiator_session,
    run_initiator_session_with_client, run_responder_session, run_responder_session_with_handler,
};
pub use storage::{Identity, PeerRecord, StateStore, StorageError};
pub use wire::{RECORD_PREFIX_BYTES, SecureWireError};
