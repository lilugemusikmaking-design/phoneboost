//! PhoneBoost host startup and local connection admission authority.
//!
//! This pass implements `HOST_STARTUP`, LC-001 through LC-004 admission, and
//! bounded LC-005/LC-006 byte framing, and LC-007 request validation. C12
//! dispatch is absent.

mod admission;
mod local_api;
mod startup;

pub use admission::{
    AuthenticatedLocalClient, FramedLocalClient, LocalAdmissionError, LocalAdmissionErrorKind,
    LocalAdmissionEvent, LocalAdmissionEventKind, LocalAdmissionScope, LocalClientLimits,
    LocalConnectionState, LocalFrameOutcome, LocalFramingError, LocalFramingErrorKind, LocalMethod,
    LocalMethodClass, LocalValidationCause, LocalValidationError, LocalValidationErrorKind,
    LocalValidationScope, ValidatedLocalRequest,
};
pub use local_api::{
    A4_DEFERRED_HANDLER, LocalApiRequestEvent, LocalHandlerDomain, serve_local_client,
};
pub use startup::{
    ReadyRuntime, StartupError, StartupErrorKind, StartupEvent, StartupEventKind, StartupIssue,
    StartupOutcome, StartupReport, host_startup,
};
