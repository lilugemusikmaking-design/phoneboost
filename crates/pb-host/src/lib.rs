//! PhoneBoost host startup and local connection admission authority.
//!
//! This crate owns host startup/local admission plus the authenticated
//! plug-and-boost controller and its logical BLAKE3 operation API.

mod admission;
mod auto_use;
mod discovery;
mod import_store;
mod local_api;
mod planner;
mod profile_store;
mod remote_secure;
mod startup;
mod transport;

pub use admission::{
    AuthenticatedLocalClient, FramedLocalClient, LocalAdmissionError, LocalAdmissionErrorKind,
    LocalAdmissionEvent, LocalAdmissionEventKind, LocalAdmissionScope, LocalClientLimits,
    LocalConnectionState, LocalFrameOutcome, LocalFramingError, LocalFramingErrorKind, LocalMethod,
    LocalMethodClass, LocalRequestFrame, LocalValidationCause, LocalValidationError,
    LocalValidationErrorKind, LocalValidationScope, ValidatedLocalRequest,
};
pub use auto_use::{
    AutoUseController, AutoUseError, AutoUseReason, AutoUseState, Blake3Execution,
    ControllerObservabilitySnapshot, DeviceDiscovery, DiscoveryError, ExecutionSource,
    FixedDeviceDiscovery, GateObservation, GateObservations, NodeStatus,
};
pub use discovery::{AvahiDiscovery, DISCOVERY_CANDIDATE_LIFETIME, DNS_SD_SERVICE_TYPE};
pub use import_store::{
    ConsumedImport, IMPORT_CANCELLATION_DEADLINE, IMPORT_COPY_CHUNK_BYTES, IMPORT_READY_TTL,
    IMPORT_RECEIVING_TIMEOUT, IMPORT_TOMBSTONE_TTL, ImportCancellation, ImportHandle,
    ImportRequester, ImportStore, ImportStoreError, ImportTerminalState,
    MAXIMUM_AGGREGATE_BYTES_PER_UID, MAXIMUM_IMPORT_BYTES, MAXIMUM_IMPORTS_PER_REQUESTER,
    MAXIMUM_IMPORTS_PER_UID, MAXIMUM_RECEIVING_PER_REQUESTER, MAXIMUM_RECEIVING_PER_UID,
    MAXIMUM_TOMBSTONES_PER_UID, ReadyImport,
};
pub use local_api::{
    A4_DEFERRED_HANDLER, LocalApiContext, LocalApiRequestEvent, LocalHandlerDomain,
    serve_local_client,
};
pub use planner::{
    BatteryPolicy, CostInput, PERFORMANCE_PROFILE_TTL, PerformanceEvidence, PerformanceProfile,
    PerformanceProfileKey, PlacementDecision, PlacementMode, PlacementReason, PlacementTarget,
    PlannerInput, ProfileInvalidationReason, ProfileStatus, RemoteGates, ThermalClass,
};
pub use profile_store::{
    C13BenchmarkProvenanceV1, C13PerformanceEvidenceV1, C13PerformanceProfileV1, C13ProfileKeyV1,
    C13RequesterPath, PerformanceProfileLoadOutcome, PerformanceProfileStore,
    PerformanceProfileStoreError, ProfileStoreInvalidationReason,
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
    AUTHENTICATED_BACKOFF_RESET, AuthenticatedGoodputMeasurement, AuthenticatedRttMeasurement,
    CONNECT_TIMEOUT, ConnectAttemptLimiter, LIVE_LINK_FRESHNESS, LinkClassObservation,
    LinkClassSource, LinkCostRequirements, LiveLinkSnapshot, MAX_CONCURRENT_ATTEMPTS_PER_DEVICE,
    MINIMUM_GOODPUT_WINDOW, MINIMUM_RTT_SAMPLES, NormalizedLinkClass, PermissionState,
    RETRY_BASE_MS, TransportCandidate, TransportError, TransportManager, TransportMetrics,
    TransportState, TransportType, ValidatedLinkCostInput, os_jitter_sample, retry_delay_ms,
};
