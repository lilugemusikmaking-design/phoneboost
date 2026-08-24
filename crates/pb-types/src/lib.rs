#![forbid(unsafe_code)]

pub const PBMUX_MAGIC: [u8; 4] = *b"PBM1";
pub const PBMUX_VERSION: u8 = 1;
pub const PBMUX_HEADER_LEN: usize = 40;
pub const MAX_PBMUX_PAYLOAD: usize = 61_440;
pub const MAX_PBMUX_PLAINTEXT: usize = 61_480;
pub const MAX_NOISE_CIPHERTEXT: usize = 61_496;
pub const MAX_LOGICAL_MESSAGE: usize = 4 * 1024 * 1024;
pub const MAX_REASSEMBLY_PER_CHANNEL: usize = 8 * 1024 * 1024;
pub const MAX_REASSEMBLY_PER_SESSION: usize = 16 * 1024 * 1024;
pub const MAX_INFLIGHT_CONTROL: usize = 64;
pub const MAX_INFLIGHT_PER_DATA_CHANNEL: usize = 16;

pub const FLAG_START: u16 = 0x0001;
pub const FLAG_END: u16 = 0x0002;
pub const FLAG_ACK_REQUIRED: u16 = 0x0004;
pub const FLAG_ERROR: u16 = 0x0008;
pub const KNOWN_FLAGS: u16 = FLAG_START | FLAG_END | FLAG_ACK_REQUIRED | FLAG_ERROR;

pub const PAIRING_TIMEOUT_MS: u64 = 120_000;
pub const PAIRING_COOLDOWN_MS: u64 = 600_000;
pub const PAIRING_MISMATCH_LIMIT: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PeerId([u8; 32]);

impl PeerId {
    pub const fn from_sha256_digest(digest: [u8; 32]) -> Self {
        Self(digest)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl From<[u8; 32]> for PeerId {
    fn from(digest: [u8; 32]) -> Self {
        Self::from_sha256_digest(digest)
    }
}

impl From<PeerId> for [u8; 32] {
    fn from(peer_id: PeerId) -> Self {
        peer_id.into_bytes()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum Channel {
    Control = 0,
    Resource = 1,
    RemoteBuffer = 2,
    Compute = 3,
    AiRpc = 4,
    Metrics = 5,
}

impl TryFrom<u8> for Channel {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Control),
            1 => Ok(Self::Resource),
            2 => Ok(Self::RemoteBuffer),
            3 => Ok(Self::Compute),
            4 => Ok(Self::AiRpc),
            5 => Ok(Self::Metrics),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ControlType {
    Ping = 1,
    Pong = 2,
    Error = 3,
    Capabilities = 4,
    Command = 5,
    CommandAck = 6,
    SessionClose = 7,
    PairConfirm = 8,
}

impl TryFrom<u16> for ControlType {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, ()> {
        match value {
            1 => Ok(Self::Ping),
            2 => Ok(Self::Pong),
            3 => Ok(Self::Error),
            4 => Ok(Self::Capabilities),
            5 => Ok(Self::Command),
            6 => Ok(Self::CommandAck),
            7 => Ok(Self::SessionClose),
            8 => Ok(Self::PairConfirm),
            _ => Err(()),
        }
    }
}

pub fn is_known_message_type(channel: Channel, message_type: u16) -> bool {
    match channel {
        Channel::Control => ControlType::try_from(message_type).is_ok(),
        Channel::Resource => (1..=5).contains(&message_type),
        Channel::RemoteBuffer => (1..=7).contains(&message_type),
        Channel::Compute => (1..=4).contains(&message_type),
        Channel::AiRpc => (1..=4).contains(&message_type),
        Channel::Metrics => (1..=2).contains(&message_type),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PairingState {
    Unpaired,
    PairingXx,
    SasPending,
    LocalConfirmed,
    PeerConfirmed,
    MutualConfirmed,
    TrustCommitting,
    Paired,
    PairRejected,
    PairingFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorScope {
    Request,
    LogicalMessage,
    Channel,
    Session,
    Pairing,
    Provider,
    Device,
    Process,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReasonCode {
    LocalRuntimeUnsafe,
    LocalAuthFailed,
    LocalBusy,
    LocalBadRequest,
    LocalUnsupportedMethod,
    FrameTooLarge,
    SequenceError,
    ReassemblyTimeout,
    BackpressureTimeout,
    UnsupportedMessage,
    ControllerBusy,
    StaleControllerLease,
    OutOfOrder,
    DuplicateResultEvicted,
    ResourceExhausted,
    RefusedStaleState,
    RefusedMemoryPressure,
    RefusedThermal,
    RefusedBattery,
    RequestIdConflict,
    BufferNotOwned,
    BufferRangeInvalid,
    BufferRangeBusy,
    BufferInvalidState,
    BufferLost,
    FailedReservation,
    FailedTimeout,
    Cancelled,
    UnknownAfterDisconnect,
    VersionMismatch,
    UnverifiedBuild,
    StateCorrupt,
    StateVersionUnsupported,
    CacheRebuilt,
    DeviceLost,
    PairingNotCommitted,
    PairConfirmUnexpected,
    PairPersistFailed,
    UnknownInitiatorIkRejected,
    SasRejected,
    PairCancelled,
    PairingTimeout,
}

impl ReasonCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalRuntimeUnsafe => "LOCAL_RUNTIME_UNSAFE",
            Self::LocalAuthFailed => "LOCAL_AUTH_FAILED",
            Self::LocalBusy => "LOCAL_BUSY",
            Self::LocalBadRequest => "LOCAL_BAD_REQUEST",
            Self::LocalUnsupportedMethod => "LOCAL_UNSUPPORTED_METHOD",
            Self::FrameTooLarge => "FRAME_TOO_LARGE",
            Self::SequenceError => "SEQUENCE_ERROR",
            Self::ReassemblyTimeout => "REASSEMBLY_TIMEOUT",
            Self::BackpressureTimeout => "BACKPRESSURE_TIMEOUT",
            Self::UnsupportedMessage => "UNSUPPORTED_MESSAGE",
            Self::ControllerBusy => "CONTROLLER_BUSY",
            Self::StaleControllerLease => "STALE_CONTROLLER_LEASE",
            Self::OutOfOrder => "OUT_OF_ORDER",
            Self::DuplicateResultEvicted => "DUPLICATE_RESULT_EVICTED",
            Self::ResourceExhausted => "RESOURCE_EXHAUSTED",
            Self::RefusedStaleState => "REFUSED_STALE_STATE",
            Self::RefusedMemoryPressure => "REFUSED_MEMORY_PRESSURE",
            Self::RefusedThermal => "REFUSED_THERMAL",
            Self::RefusedBattery => "REFUSED_BATTERY",
            Self::RequestIdConflict => "REQUEST_ID_CONFLICT",
            Self::BufferNotOwned => "BUFFER_NOT_OWNED",
            Self::BufferRangeInvalid => "BUFFER_RANGE_INVALID",
            Self::BufferRangeBusy => "BUFFER_RANGE_BUSY",
            Self::BufferInvalidState => "BUFFER_INVALID_STATE",
            Self::BufferLost => "BUFFER_LOST",
            Self::FailedReservation => "FAILED_RESERVATION",
            Self::FailedTimeout => "FAILED_TIMEOUT",
            Self::Cancelled => "CANCELLED",
            Self::UnknownAfterDisconnect => "UNKNOWN_AFTER_DISCONNECT",
            Self::VersionMismatch => "VERSION_MISMATCH",
            Self::UnverifiedBuild => "UNVERIFIED_BUILD",
            Self::StateCorrupt => "STATE_CORRUPT",
            Self::StateVersionUnsupported => "STATE_VERSION_UNSUPPORTED",
            Self::CacheRebuilt => "CACHE_REBUILT",
            Self::DeviceLost => "DEVICE_LOST",
            Self::PairingNotCommitted => "PAIRING_NOT_COMMITTED",
            Self::PairConfirmUnexpected => "PAIR_CONFIRM_UNEXPECTED",
            Self::PairPersistFailed => "PAIR_PERSIST_FAILED",
            Self::UnknownInitiatorIkRejected => "UNKNOWN_INITIATOR_IK_REJECTED",
            Self::SasRejected => "SAS_REJECTED",
            Self::PairCancelled => "PAIR_CANCELLED",
            Self::PairingTimeout => "PAIRING_TIMEOUT",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mutation<T> {
    pub value: T,
    pub state_changed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_constants_are_exact() {
        assert_eq!(PBMUX_HEADER_LEN + MAX_PBMUX_PAYLOAD, MAX_PBMUX_PLAINTEXT);
        assert_eq!(MAX_PBMUX_PLAINTEXT + 16, MAX_NOISE_CIPHERTEXT);
        assert_eq!(MAX_LOGICAL_MESSAGE, 4_194_304);
        assert_eq!(PAIRING_COOLDOWN_MS, 600_000);
    }

    #[test]
    fn registry_bounds_are_exact() {
        assert!(is_known_message_type(Channel::Control, 8));
        assert!(!is_known_message_type(Channel::Control, 9));
        assert!(is_known_message_type(Channel::RemoteBuffer, 7));
        assert!(!is_known_message_type(Channel::RemoteBuffer, 8));
    }
}
