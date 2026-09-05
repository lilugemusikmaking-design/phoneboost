//! PhoneBoost host startup and local connection admission authority.
//!
//! This crate owns host startup/local admission plus the authenticated
//! plug-and-boost controller and its logical BLAKE3 operation API.

mod admission;
mod auto_use;
mod discovery;
mod local_api;
mod remote_secure;
mod startup;
mod transport;

pub use admission::{
    AuthenticatedLocalClient, FramedLocalClient, LocalAdmissionError, LocalAdmissionErrorKind,
    LocalAdmissionEvent, LocalAdmissionEventKind, LocalAdmissionScope, LocalClientLimits,
    LocalConnectionState, LocalFrameOutcome, LocalFramingError, LocalFramingErrorKind, LocalMethod,
    LocalMethodClass, LocalValidationCause, LocalValidationError, LocalValidationErrorKind,
    LocalValidationScope, ValidatedLocalRequest,
};
pub use auto_use::{
    AutoUseController, AutoUseError, AutoUseReason, AutoUseState, Blake3Execution,
    ControllerObservabilitySnapshot, DeviceDiscovery, DiscoveryError, ExecutionSource,
    FixedDeviceDiscovery, GateObservation, GateObservations, NodeStatus,
};
pub use discovery::{AvahiDiscovery, DISCOVERY_CANDIDATE_LIFETIME, DNS_SD_SERVICE_TYPE};
pub use local_api::{
    A4_DEFERRED_HANDLER, LocalApiContext, LocalApiRequestEvent, LocalHandlerDomain,
    serve_local_client,
};
pub use remote_secure::{
    initialize_remote_secure, remote_pairing_begin, remote_pairing_cancel, remote_pairing_confirm,
    remote_secure_runtime, remote_status,
};
pub use startup::{
    ReadyRuntime, StartupError, StartupErrorKind, StartupEvent, StartupEventKind, StartupIssue,
    StartupOutcome, StartupReport, host_startup,
};
pub use transport::{
    AUTHENTICATED_BACKOFF_RESET, CONNECT_TIMEOUT, ConnectAttemptLimiter,
    MAX_CONCURRENT_ATTEMPTS_PER_DEVICE, PermissionState, RETRY_BASE_MS, TransportCandidate,
    TransportError, TransportManager, TransportMetrics, TransportState, TransportType,
    os_jitter_sample, retry_delay_ms,
};
