use pb_types::{Channel, FLAG_ACK_REQUIRED, FLAG_END, FLAG_START};

use crate::{Frame, Header, PbmuxError, PbmuxErrorKind, read_u16, read_u32, read_u64};

pub const RESOURCE_REQUEST_LEN: usize = 48;
pub const RESOURCE_RESULT_LEN: usize = 72;
pub const RESOURCE_EXPIRE_NOTIFY_LEN: usize = 64;
pub const RESOURCE_RESERVATION_TTL_MS: u32 = 30_000;
pub const RESOURCE_MAX_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResourceRequest {
    Reserve {
        lease_id: [u8; 16],
        worker_incarnation_id: [u8; 16],
        requested_bytes: u64,
    },
    Commit {
        lease_id: [u8; 16],
        worker_incarnation_id: [u8; 16],
        reservation_id: [u8; 16],
    },
    Release {
        lease_id: [u8; 16],
        worker_incarnation_id: [u8; 16],
        reservation_id: [u8; 16],
    },
}

impl ResourceRequest {
    pub const fn lease_id(&self) -> &[u8; 16] {
        match self {
            Self::Reserve { lease_id, .. }
            | Self::Commit { lease_id, .. }
            | Self::Release { lease_id, .. } => lease_id,
        }
    }

    pub const fn worker_incarnation_id(&self) -> &[u8; 16] {
        match self {
            Self::Reserve {
                worker_incarnation_id,
                ..
            }
            | Self::Commit {
                worker_incarnation_id,
                ..
            }
            | Self::Release {
                worker_incarnation_id,
                ..
            } => worker_incarnation_id,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ResourceResultState {
    Completed = 2,
    Failed = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ResourceReason {
    None = 0,
    StaleControllerLease = 1,
    RefusedStaleState = 2,
    ResourceExhausted = 3,
    ReservationNotFound = 4,
    ReservationNotCommitted = 5,
    ReservationExpired = 6,
    ReservationAlreadyConsumed = 7,
    CommitRefusedSafety = 8,
    RequestIdConflict = 9,
    IdempotenceTableFull = 10,
    UnsupportedMessage = 11,
    InternalError = 12,
}

impl TryFrom<u16> for ResourceReason {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::None,
            1 => Self::StaleControllerLease,
            2 => Self::RefusedStaleState,
            3 => Self::ResourceExhausted,
            4 => Self::ReservationNotFound,
            5 => Self::ReservationNotCommitted,
            6 => Self::ReservationExpired,
            7 => Self::ReservationAlreadyConsumed,
            8 => Self::CommitRefusedSafety,
            9 => Self::RequestIdConflict,
            10 => Self::IdempotenceTableFull,
            11 => Self::UnsupportedMessage,
            12 => Self::InternalError,
            _ => return Err(()),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ReservationState {
    Reserved = 1,
    Committed = 2,
    Consumed = 3,
    Released = 4,
    Expired = 5,
    RefusedSafety = 6,
    ConsumedReleased = 7,
}

impl TryFrom<u8> for ReservationState {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            1 => Self::Reserved,
            2 => Self::Committed,
            3 => Self::Consumed,
            4 => Self::Released,
            5 => Self::Expired,
            6 => Self::RefusedSafety,
            7 => Self::ConsumedReleased,
            _ => return Err(()),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReservationResultRef {
    pub reservation_id: [u8; 16],
    pub state: ReservationState,
    pub granted_bytes: u64,
    pub ttl_remaining_ms: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceResult {
    pub state: ResourceResultState,
    pub reason: ResourceReason,
    pub lease_id: [u8; 16],
    pub worker_incarnation_id: [u8; 16],
    pub reservation: Option<ReservationResultRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceResponseKind {
    ReserveAck,
    Commit,
    Release,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpireNotification {
    pub lease_id: [u8; 16],
    pub worker_incarnation_id: [u8; 16],
    pub reservation_id: [u8; 16],
    pub granted_bytes: u64,
}

impl ResourceResponseKind {
    const fn message_type(self) -> u16 {
        match self {
            Self::ReserveAck => 2,
            Self::Commit => 3,
            Self::Release => 4,
        }
    }
}

fn invalid() -> PbmuxError {
    PbmuxError::logical(PbmuxErrorKind::InvalidResourcePayload)
}

fn nonzero(id: &[u8; 16]) -> bool {
    id.iter().any(|byte| *byte != 0)
}

fn fixed_request_profile(frame: &Frame) -> bool {
    frame.header.channel == Channel::Resource
        && matches!(frame.header.message_type, 1 | 3 | 4)
        && frame.header.flags == FLAG_START | FLAG_END | FLAG_ACK_REQUIRED
        && frame.header.request_id != 0
        && frame.header.fragment_index == 0
        && frame.header.payload_len == RESOURCE_REQUEST_LEN as u32
        && frame.header.logical_message_len == RESOURCE_REQUEST_LEN as u32
        && frame.payload.len() == RESOURCE_REQUEST_LEN
}

pub fn build_resource_request_frame(
    request: &ResourceRequest,
    request_id: u64,
    sequence: u64,
) -> Result<Frame, PbmuxError> {
    if request_id == 0 || !nonzero(request.lease_id()) || !nonzero(request.worker_incarnation_id())
    {
        return Err(invalid());
    }
    let (message_type, payload) = match request {
        ResourceRequest::Reserve {
            lease_id,
            worker_incarnation_id,
            requested_bytes,
        } => {
            if *requested_bytes == 0 || *requested_bytes > RESOURCE_MAX_BYTES {
                return Err(invalid());
            }
            let mut payload = Vec::with_capacity(RESOURCE_REQUEST_LEN);
            payload.extend_from_slice(lease_id);
            payload.extend_from_slice(worker_incarnation_id);
            payload.push(1);
            payload.extend_from_slice(&[0; 3]);
            payload.extend_from_slice(&requested_bytes.to_be_bytes());
            payload.extend_from_slice(&RESOURCE_RESERVATION_TTL_MS.to_be_bytes());
            (1, payload)
        }
        ResourceRequest::Commit {
            lease_id,
            worker_incarnation_id,
            reservation_id,
        }
        | ResourceRequest::Release {
            lease_id,
            worker_incarnation_id,
            reservation_id,
        } => {
            if !nonzero(reservation_id) {
                return Err(invalid());
            }
            let mut payload = Vec::with_capacity(RESOURCE_REQUEST_LEN);
            payload.extend_from_slice(lease_id);
            payload.extend_from_slice(worker_incarnation_id);
            payload.extend_from_slice(reservation_id);
            let message_type = if matches!(request, ResourceRequest::Commit { .. }) {
                3
            } else {
                4
            };
            (message_type, payload)
        }
    };
    Ok(Frame {
        header: Header {
            channel: Channel::Resource,
            flags: FLAG_START | FLAG_END | FLAG_ACK_REQUIRED,
            message_type,
            request_id,
            sequence,
            fragment_index: 0,
            payload_len: RESOURCE_REQUEST_LEN as u32,
            logical_message_len: RESOURCE_REQUEST_LEN as u32,
        },
        payload,
    })
}

pub fn parse_resource_request_frame(frame: &Frame) -> Result<ResourceRequest, PbmuxError> {
    if !fixed_request_profile(frame) {
        return Err(invalid());
    }
    let payload = &frame.payload;
    let lease_id = payload[0..16].try_into().expect("fixed lease id");
    let worker_incarnation_id = payload[16..32].try_into().expect("fixed incarnation id");
    if !nonzero(&lease_id) || !nonzero(&worker_incarnation_id) {
        return Err(invalid());
    }
    match frame.header.message_type {
        1 => {
            if payload[32] != 1
                || payload[33..36] != [0; 3]
                || payload[44..48] != RESOURCE_RESERVATION_TTL_MS.to_be_bytes()
            {
                return Err(invalid());
            }
            let requested_bytes = read_u64(payload, 36);
            if requested_bytes == 0 || requested_bytes > RESOURCE_MAX_BYTES {
                return Err(invalid());
            }
            Ok(ResourceRequest::Reserve {
                lease_id,
                worker_incarnation_id,
                requested_bytes,
            })
        }
        3 | 4 => {
            let reservation_id = payload[32..48].try_into().expect("fixed reservation id");
            if !nonzero(&reservation_id) {
                return Err(invalid());
            }
            if frame.header.message_type == 3 {
                Ok(ResourceRequest::Commit {
                    lease_id,
                    worker_incarnation_id,
                    reservation_id,
                })
            } else {
                Ok(ResourceRequest::Release {
                    lease_id,
                    worker_incarnation_id,
                    reservation_id,
                })
            }
        }
        _ => Err(invalid()),
    }
}

pub fn build_expire_notification_frame(
    notification: &ExpireNotification,
    sequence: u64,
) -> Result<Frame, PbmuxError> {
    if !nonzero(&notification.lease_id)
        || !nonzero(&notification.worker_incarnation_id)
        || !nonzero(&notification.reservation_id)
        || notification.granted_bytes == 0
    {
        return Err(invalid());
    }
    let mut payload = Vec::with_capacity(RESOURCE_EXPIRE_NOTIFY_LEN);
    payload.extend_from_slice(&notification.lease_id);
    payload.extend_from_slice(&notification.worker_incarnation_id);
    payload.extend_from_slice(&notification.reservation_id);
    payload.push(1);
    payload.push(ReservationState::Expired as u8);
    payload.extend_from_slice(&(ResourceReason::ReservationExpired as u16).to_be_bytes());
    payload.extend_from_slice(&notification.granted_bytes.to_be_bytes());
    payload.extend_from_slice(&[0; 4]);
    Ok(Frame {
        header: Header {
            channel: Channel::Resource,
            flags: FLAG_START | FLAG_END,
            message_type: 5,
            request_id: 0,
            sequence,
            fragment_index: 0,
            payload_len: RESOURCE_EXPIRE_NOTIFY_LEN as u32,
            logical_message_len: RESOURCE_EXPIRE_NOTIFY_LEN as u32,
        },
        payload,
    })
}

pub fn parse_expire_notification_frame(frame: &Frame) -> Result<ExpireNotification, PbmuxError> {
    if frame.header.channel != Channel::Resource
        || frame.header.message_type != 5
        || frame.header.flags != FLAG_START | FLAG_END
        || frame.header.request_id != 0
        || frame.header.fragment_index != 0
        || frame.header.payload_len != RESOURCE_EXPIRE_NOTIFY_LEN as u32
        || frame.header.logical_message_len != RESOURCE_EXPIRE_NOTIFY_LEN as u32
        || frame.payload.len() != RESOURCE_EXPIRE_NOTIFY_LEN
    {
        return Err(invalid());
    }
    let bytes = &frame.payload;
    let notification = ExpireNotification {
        lease_id: bytes[0..16].try_into().expect("fixed lease id"),
        worker_incarnation_id: bytes[16..32].try_into().expect("fixed incarnation id"),
        reservation_id: bytes[32..48].try_into().expect("fixed reservation id"),
        granted_bytes: read_u64(bytes, 52),
    };
    if bytes[48] != 1
        || bytes[49] != ReservationState::Expired as u8
        || read_u16(bytes, 50) != ResourceReason::ReservationExpired as u16
        || bytes[60..64] != [0; 4]
        || !nonzero(&notification.lease_id)
        || !nonzero(&notification.worker_incarnation_id)
        || !nonzero(&notification.reservation_id)
        || notification.granted_bytes == 0
    {
        return Err(invalid());
    }
    Ok(notification)
}

fn validate_result(kind: ResourceResponseKind, result: &ResourceResult) -> Result<(), PbmuxError> {
    if !nonzero(&result.lease_id) || !nonzero(&result.worker_incarnation_id) {
        return Err(invalid());
    }
    if (result.state == ResourceResultState::Completed) != (result.reason == ResourceReason::None) {
        return Err(invalid());
    }
    if let Some(reservation) = result.reservation {
        if !nonzero(&reservation.reservation_id) || reservation.granted_bytes == 0 {
            return Err(invalid());
        }
        let ttl_valid = match reservation.state {
            ReservationState::Reserved => {
                (1..=RESOURCE_RESERVATION_TTL_MS).contains(&reservation.ttl_remaining_ms)
            }
            _ => reservation.ttl_remaining_ms == 0,
        };
        if !ttl_valid {
            return Err(invalid());
        }
    }
    if result.state == ResourceResultState::Completed {
        let expected = match kind {
            ResourceResponseKind::ReserveAck => ReservationState::Reserved,
            ResourceResponseKind::Commit => ReservationState::Committed,
            ResourceResponseKind::Release => ReservationState::Released,
        };
        if result.reservation.map(|reservation| reservation.state) != Some(expected) {
            return Err(invalid());
        }
    }
    Ok(())
}

fn encode_result(
    kind: ResourceResponseKind,
    result: &ResourceResult,
) -> Result<Vec<u8>, PbmuxError> {
    validate_result(kind, result)?;
    let mut bytes = Vec::with_capacity(RESOURCE_RESULT_LEN);
    bytes.push(result.state as u8);
    bytes.push(u8::from(result.reservation.is_some()));
    bytes.extend_from_slice(&(result.reason as u16).to_be_bytes());
    bytes.extend_from_slice(&result.lease_id);
    bytes.extend_from_slice(&result.worker_incarnation_id);
    if let Some(reservation) = result.reservation {
        bytes.extend_from_slice(&reservation.reservation_id);
        bytes.push(reservation.state as u8);
        bytes.push(1);
        bytes.extend_from_slice(&[0; 2]);
        bytes.extend_from_slice(&reservation.granted_bytes.to_be_bytes());
        bytes.extend_from_slice(&reservation.ttl_remaining_ms.to_be_bytes());
    } else {
        bytes.extend_from_slice(&[0; 32]);
    }
    bytes.extend_from_slice(&[0; 4]);
    debug_assert_eq!(bytes.len(), RESOURCE_RESULT_LEN);
    Ok(bytes)
}

pub fn build_resource_result_frame(
    kind: ResourceResponseKind,
    result: &ResourceResult,
    request_id: u64,
    sequence: u64,
) -> Result<Frame, PbmuxError> {
    if request_id == 0 {
        return Err(invalid());
    }
    let payload = encode_result(kind, result)?;
    Ok(Frame {
        header: Header {
            channel: Channel::Resource,
            flags: FLAG_START | FLAG_END,
            message_type: kind.message_type(),
            request_id,
            sequence,
            fragment_index: 0,
            payload_len: RESOURCE_RESULT_LEN as u32,
            logical_message_len: RESOURCE_RESULT_LEN as u32,
        },
        payload,
    })
}

pub fn parse_resource_result_frame(
    frame: &Frame,
) -> Result<(ResourceResponseKind, ResourceResult), PbmuxError> {
    let kind = match frame.header.message_type {
        2 => ResourceResponseKind::ReserveAck,
        3 => ResourceResponseKind::Commit,
        4 => ResourceResponseKind::Release,
        _ => return Err(invalid()),
    };
    if frame.header.channel != Channel::Resource
        || frame.header.flags != FLAG_START | FLAG_END
        || frame.header.request_id == 0
        || frame.header.fragment_index != 0
        || frame.header.payload_len != RESOURCE_RESULT_LEN as u32
        || frame.header.logical_message_len != RESOURCE_RESULT_LEN as u32
        || frame.payload.len() != RESOURCE_RESULT_LEN
    {
        return Err(invalid());
    }
    let bytes = &frame.payload;
    if bytes[1] > 1 || bytes[54..56] != [0; 2] || bytes[68..72] != [0; 4] {
        return Err(invalid());
    }
    let state = match bytes[0] {
        2 => ResourceResultState::Completed,
        3 => ResourceResultState::Failed,
        _ => return Err(invalid()),
    };
    let reason = ResourceReason::try_from(read_u16(bytes, 2)).map_err(|()| invalid())?;
    let lease_id = bytes[4..20].try_into().expect("fixed lease id");
    let worker_incarnation_id = bytes[20..36].try_into().expect("fixed incarnation id");
    let reservation = if bytes[1] == 0 {
        if bytes[36..68] != [0; 32] {
            return Err(invalid());
        }
        None
    } else {
        let reservation_id = bytes[36..52].try_into().expect("fixed reservation id");
        if bytes[53] != 1 {
            return Err(invalid());
        }
        Some(ReservationResultRef {
            reservation_id,
            state: ReservationState::try_from(bytes[52]).map_err(|()| invalid())?,
            granted_bytes: read_u64(bytes, 56),
            ttl_remaining_ms: read_u32(bytes, 64),
        })
    };
    let result = ResourceResult {
        state,
        reason,
        lease_id,
        worker_incarnation_id,
        reservation,
    };
    validate_result(kind, &result)?;
    Ok((kind, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode, encode};

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/../../docs/protocol/c08_c09_wire_v0_1_vectors_001/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn locked_resource_vectors_round_trip_and_malformed_rejects() {
        for (name, response) in [
            ("GV-C08-01-reserve-request.bin", false),
            ("GV-C08-02-reserve-success.bin", true),
            ("GV-C08-03-reserve-refused-stale.bin", true),
            ("GV-C08-04-commit-request.bin", false),
            ("GV-C08-05-commit-result.bin", true),
            ("GV-C08-06-release-request.bin", false),
            ("GV-C08-07-release-result.bin", true),
            ("GV-C08-10-request-id-conflict.bin", false),
        ] {
            let bytes = fixture(name);
            let frame = decode(&bytes).unwrap();
            if response {
                let (kind, result) = parse_resource_result_frame(&frame).unwrap();
                let rebuilt = build_resource_result_frame(
                    kind,
                    &result,
                    frame.header.request_id,
                    frame.header.sequence,
                )
                .unwrap();
                assert_eq!(encode(&rebuilt).unwrap(), bytes, "{name}");
            } else {
                parse_resource_request_frame(&frame).unwrap();
            }
        }
        let malformed = fixture("GV-C08-09-malformed-presence.bin");
        assert!(parse_resource_result_frame(&decode(&malformed).unwrap()).is_err());

        let expire = fixture("GV-C08-08-expire-notify.bin");
        let frame = decode(&expire).unwrap();
        let notification = parse_expire_notification_frame(&frame).unwrap();
        assert_eq!(
            encode(&build_expire_notification_frame(&notification, frame.header.sequence).unwrap())
                .unwrap(),
            expire
        );
    }
}
