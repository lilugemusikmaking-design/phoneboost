use pb_types::{
    Channel, FLAG_ACK_REQUIRED, FLAG_END, FLAG_START, MAX_LOGICAL_MESSAGE, MAX_PBMUX_PAYLOAD,
};

use crate::{Frame, Header, PbmuxError, PbmuxErrorKind, read_u16, read_u32, read_u64};

pub const REMOTE_BUFFER_ALLOC_LEN: usize = 64;
pub const REMOTE_BUFFER_RANGE_PREFIX_LEN: usize = 64;
pub const REMOTE_BUFFER_HANDLE_LEN: usize = 48;
pub const REMOTE_BUFFER_RESULT_PREFIX_LEN: usize = 100;
pub const MAX_PUT_BODY: usize = MAX_LOGICAL_MESSAGE - REMOTE_BUFFER_RANGE_PREFIX_LEN;
pub const MAX_DATA_BODY: usize = MAX_LOGICAL_MESSAGE - REMOTE_BUFFER_RESULT_PREFIX_LEN;
pub const REMOTE_BUFFER_DEFAULT_TTL_MS: u32 = 300_000;
pub const REMOTE_BUFFER_MAX_TTL_MS: u32 = 1_800_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AllocationFlags(u32);

impl AllocationFlags {
    pub const EVICTABLE: Self = Self(1);
    pub const RECONSTRUCTIBLE: Self = Self(2);
    pub const SENSITIVE_D3: Self = Self(4);
    pub const NONE: Self = Self(0);

    pub const fn from_bits(bits: u32) -> Option<Self> {
        if bits & !7 == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    pub const fn bits(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoteBufferRequest {
    Alloc {
        lease_id: [u8; 16],
        worker_incarnation_id: [u8; 16],
        reservation_id: [u8; 16],
        size_bytes: u64,
        allocation_flags: AllocationFlags,
    },
    Put {
        lease_id: [u8; 16],
        worker_incarnation_id: [u8; 16],
        buffer_id: [u8; 16],
        offset: u64,
        data: Vec<u8>,
    },
    Get {
        lease_id: [u8; 16],
        worker_incarnation_id: [u8; 16],
        buffer_id: [u8; 16],
        offset: u64,
        length: u32,
    },
    Free {
        lease_id: [u8; 16],
        worker_incarnation_id: [u8; 16],
        buffer_id: [u8; 16],
    },
    Stat {
        lease_id: [u8; 16],
        worker_incarnation_id: [u8; 16],
        buffer_id: [u8; 16],
    },
    Touch {
        lease_id: [u8; 16],
        worker_incarnation_id: [u8; 16],
        buffer_id: [u8; 16],
    },
}

impl RemoteBufferRequest {
    pub const fn lease_id(&self) -> &[u8; 16] {
        match self {
            Self::Alloc { lease_id, .. }
            | Self::Put { lease_id, .. }
            | Self::Get { lease_id, .. }
            | Self::Free { lease_id, .. }
            | Self::Stat { lease_id, .. }
            | Self::Touch { lease_id, .. } => lease_id,
        }
    }

    pub const fn worker_incarnation_id(&self) -> &[u8; 16] {
        match self {
            Self::Alloc {
                worker_incarnation_id,
                ..
            }
            | Self::Put {
                worker_incarnation_id,
                ..
            }
            | Self::Get {
                worker_incarnation_id,
                ..
            }
            | Self::Free {
                worker_incarnation_id,
                ..
            }
            | Self::Stat {
                worker_incarnation_id,
                ..
            }
            | Self::Touch {
                worker_incarnation_id,
                ..
            } => worker_incarnation_id,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum BufferReason {
    None = 0,
    StaleControllerLease = 1,
    BufferNotFound = 2,
    BufferNotOwned = 3,
    BufferWrongIncarnation = 4,
    BufferInvalidState = 5,
    BufferLost = 6,
    BufferFreed = 7,
    BufferEvicted = 8,
    BufferRangeInvalid = 9,
    BufferRangeBusy = 10,
    PayloadTooLarge = 11,
    ResourceExhausted = 12,
    ReservationInvalid = 13,
    UnsupportedMessage = 14,
    InternalError = 15,
}

impl TryFrom<u16> for BufferReason {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::None,
            1 => Self::StaleControllerLease,
            2 => Self::BufferNotFound,
            3 => Self::BufferNotOwned,
            4 => Self::BufferWrongIncarnation,
            5 => Self::BufferInvalidState,
            6 => Self::BufferLost,
            7 => Self::BufferFreed,
            8 => Self::BufferEvicted,
            9 => Self::BufferRangeInvalid,
            10 => Self::BufferRangeBusy,
            11 => Self::PayloadTooLarge,
            12 => Self::ResourceExhausted,
            13 => Self::ReservationInvalid,
            14 => Self::UnsupportedMessage,
            15 => Self::InternalError,
            _ => return Err(()),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum BufferState {
    Allocated = 1,
    Ready = 2,
    InUse = 3,
    Evicted = 4,
    Lost = 5,
    Freed = 6,
}

impl BufferState {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Evicted | Self::Lost | Self::Freed)
    }
}

impl TryFrom<u8> for BufferState {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            1 => Self::Allocated,
            2 => Self::Ready,
            3 => Self::InUse,
            4 => Self::Evicted,
            5 => Self::Lost,
            6 => Self::Freed,
            _ => return Err(()),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferResultRef {
    pub buffer_id: [u8; 16],
    pub state: BufferState,
    pub allocation_flags: AllocationFlags,
    pub size_bytes: u64,
    pub ttl_remaining_ms: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BufferResult {
    pub completed: bool,
    pub reason: BufferReason,
    pub lease_id: [u8; 16],
    pub worker_incarnation_id: [u8; 16],
    pub buffer: Option<BufferResultRef>,
    pub reservation_id: Option<[u8; 16]>,
    pub offset: u64,
    pub data_len: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteBufferResponseKind {
    AllocAck,
    Put,
    Data,
    Free,
    Stat,
    Touch,
}

impl RemoteBufferResponseKind {
    pub const fn message_type(self) -> u16 {
        match self {
            Self::AllocAck => 2,
            Self::Put => 3,
            Self::Data => 5,
            Self::Free => 6,
            Self::Stat => 7,
            Self::Touch => 8,
        }
    }
}

fn invalid() -> PbmuxError {
    PbmuxError::logical(PbmuxErrorKind::InvalidRemoteBufferPayload)
}

fn nonzero(id: &[u8; 16]) -> bool {
    id.iter().any(|byte| *byte != 0)
}

pub fn build_remote_buffer_request_frames(
    request: &RemoteBufferRequest,
    request_id: u64,
    first_sequence: u64,
) -> Result<Vec<Frame>, PbmuxError> {
    if request_id == 0 || !nonzero(request.lease_id()) || !nonzero(request.worker_incarnation_id())
    {
        return Err(invalid());
    }
    let (message_type, payload) = encode_request(request)?;
    let count = payload.len().div_ceil(MAX_PBMUX_PAYLOAD);
    if count > 1 && message_type != 3 {
        return Err(invalid());
    }
    let mut frames = Vec::with_capacity(count);
    for (index, chunk) in payload.chunks(MAX_PBMUX_PAYLOAD).enumerate() {
        let flags = if count == 1 {
            FLAG_START | FLAG_END | FLAG_ACK_REQUIRED
        } else if index == 0 {
            FLAG_START | FLAG_ACK_REQUIRED
        } else if index + 1 == count {
            FLAG_END | FLAG_ACK_REQUIRED
        } else {
            FLAG_ACK_REQUIRED
        };
        frames.push(Frame {
            header: Header {
                channel: Channel::RemoteBuffer,
                flags,
                message_type,
                request_id,
                sequence: first_sequence
                    .checked_add(index as u64)
                    .ok_or_else(invalid)?,
                fragment_index: index as u32,
                payload_len: chunk.len() as u32,
                logical_message_len: if index == 0 { payload.len() as u32 } else { 0 },
            },
            payload: chunk.to_vec(),
        });
    }
    Ok(frames)
}

fn encode_request(request: &RemoteBufferRequest) -> Result<(u16, Vec<u8>), PbmuxError> {
    let mut payload = Vec::new();
    payload.extend_from_slice(request.lease_id());
    payload.extend_from_slice(request.worker_incarnation_id());
    let message_type = match request {
        RemoteBufferRequest::Alloc {
            reservation_id,
            size_bytes,
            allocation_flags,
            ..
        } => {
            if !nonzero(reservation_id) {
                return Err(invalid());
            }
            payload.extend_from_slice(reservation_id);
            payload.extend_from_slice(&size_bytes.to_be_bytes());
            payload.extend_from_slice(&allocation_flags.bits().to_be_bytes());
            payload.extend_from_slice(&[0; 4]);
            1
        }
        RemoteBufferRequest::Put {
            buffer_id,
            offset,
            data,
            ..
        } => {
            if !nonzero(buffer_id) || data.len() > MAX_PUT_BODY {
                return Err(invalid());
            }
            payload.extend_from_slice(buffer_id);
            payload.extend_from_slice(&offset.to_be_bytes());
            payload.extend_from_slice(&(data.len() as u32).to_be_bytes());
            payload.extend_from_slice(&[0; 4]);
            payload.extend_from_slice(data);
            3
        }
        RemoteBufferRequest::Get {
            buffer_id,
            offset,
            length,
            ..
        } => {
            if !nonzero(buffer_id) {
                return Err(invalid());
            }
            payload.extend_from_slice(buffer_id);
            payload.extend_from_slice(&offset.to_be_bytes());
            payload.extend_from_slice(&length.to_be_bytes());
            payload.extend_from_slice(&[0; 4]);
            4
        }
        RemoteBufferRequest::Free { buffer_id, .. }
        | RemoteBufferRequest::Stat { buffer_id, .. }
        | RemoteBufferRequest::Touch { buffer_id, .. } => {
            if !nonzero(buffer_id) {
                return Err(invalid());
            }
            payload.extend_from_slice(buffer_id);
            match request {
                RemoteBufferRequest::Free { .. } => 6,
                RemoteBufferRequest::Stat { .. } => 7,
                RemoteBufferRequest::Touch { .. } => 8,
                _ => unreachable!(),
            }
        }
    };
    if payload.len() > MAX_LOGICAL_MESSAGE {
        return Err(invalid());
    }
    Ok((message_type, payload))
}

pub fn validate_remote_buffer_request_fragment(frame: &Frame) -> Result<(), PbmuxError> {
    let header = &frame.header;
    if header.channel != Channel::RemoteBuffer || header.request_id == 0 {
        return Err(invalid());
    }
    if header.message_type != 3 {
        let expected = match header.message_type {
            1 | 4 => REMOTE_BUFFER_ALLOC_LEN,
            6..=8 => REMOTE_BUFFER_HANDLE_LEN,
            _ => return Err(invalid()),
        };
        if header.flags != FLAG_START | FLAG_END | FLAG_ACK_REQUIRED
            || header.fragment_index != 0
            || header.payload_len != expected as u32
            || header.logical_message_len != expected as u32
            || frame.payload.len() != expected
        {
            return Err(invalid());
        }
        return Ok(());
    }

    let start = header.flags & FLAG_START != 0;
    let end = header.flags & FLAG_END != 0;
    if header.flags & FLAG_ACK_REQUIRED == 0 {
        return Err(invalid());
    }
    if start && end {
        if header.flags != FLAG_START | FLAG_END | FLAG_ACK_REQUIRED
            || header.fragment_index != 0
            || header.logical_message_len != header.payload_len
        {
            return Err(invalid());
        }
    } else if start {
        if header.flags != FLAG_START | FLAG_ACK_REQUIRED
            || header.fragment_index != 0
            || header.logical_message_len as usize > MAX_LOGICAL_MESSAGE
            || header.logical_message_len <= header.payload_len
        {
            return Err(invalid());
        }
    } else if end {
        if header.flags != FLAG_END | FLAG_ACK_REQUIRED
            || header.fragment_index == 0
            || header.logical_message_len != 0
        {
            return Err(invalid());
        }
    } else if header.flags != FLAG_ACK_REQUIRED
        || header.fragment_index == 0
        || header.logical_message_len != 0
    {
        return Err(invalid());
    }
    Ok(())
}

pub fn parse_remote_buffer_request_frame(frame: &Frame) -> Result<RemoteBufferRequest, PbmuxError> {
    validate_remote_buffer_request_fragment(frame)?;
    if frame.header.flags & (FLAG_START | FLAG_END) != FLAG_START | FLAG_END {
        return Err(invalid());
    }
    parse_remote_buffer_request_payload(frame.header.message_type, &frame.payload)
}

pub fn parse_remote_buffer_request_payload(
    message_type: u16,
    payload: &[u8],
) -> Result<RemoteBufferRequest, PbmuxError> {
    let expected = match message_type {
        1 | 4 => REMOTE_BUFFER_ALLOC_LEN,
        3 => {
            if payload.len() < REMOTE_BUFFER_RANGE_PREFIX_LEN || payload.len() > MAX_LOGICAL_MESSAGE
            {
                return Err(invalid());
            }
            payload.len()
        }
        6..=8 => REMOTE_BUFFER_HANDLE_LEN,
        _ => return Err(invalid()),
    };
    if payload.len() != expected {
        return Err(invalid());
    }
    let lease_id = payload[0..16].try_into().expect("fixed lease id");
    let worker_incarnation_id = payload[16..32].try_into().expect("fixed incarnation id");
    let object_id: [u8; 16] = payload[32..48].try_into().expect("fixed object id");
    if !nonzero(&lease_id) || !nonzero(&worker_incarnation_id) || !nonzero(&object_id) {
        return Err(invalid());
    }
    match message_type {
        1 => {
            if payload[60..64] != [0; 4] {
                return Err(invalid());
            }
            let allocation_flags =
                AllocationFlags::from_bits(read_u32(payload, 56)).ok_or_else(invalid)?;
            Ok(RemoteBufferRequest::Alloc {
                lease_id,
                worker_incarnation_id,
                reservation_id: object_id,
                size_bytes: read_u64(payload, 48),
                allocation_flags,
            })
        }
        3 => {
            if payload[60..64] != [0; 4] {
                return Err(invalid());
            }
            let data_len = read_u32(payload, 56) as usize;
            if data_len != payload.len() - REMOTE_BUFFER_RANGE_PREFIX_LEN || data_len > MAX_PUT_BODY
            {
                return Err(invalid());
            }
            Ok(RemoteBufferRequest::Put {
                lease_id,
                worker_incarnation_id,
                buffer_id: object_id,
                offset: read_u64(payload, 48),
                data: payload[REMOTE_BUFFER_RANGE_PREFIX_LEN..].to_vec(),
            })
        }
        4 => {
            if payload[60..64] != [0; 4] {
                return Err(invalid());
            }
            Ok(RemoteBufferRequest::Get {
                lease_id,
                worker_incarnation_id,
                buffer_id: object_id,
                offset: read_u64(payload, 48),
                length: read_u32(payload, 56),
            })
        }
        6 => Ok(RemoteBufferRequest::Free {
            lease_id,
            worker_incarnation_id,
            buffer_id: object_id,
        }),
        7 => Ok(RemoteBufferRequest::Stat {
            lease_id,
            worker_incarnation_id,
            buffer_id: object_id,
        }),
        8 => Ok(RemoteBufferRequest::Touch {
            lease_id,
            worker_incarnation_id,
            buffer_id: object_id,
        }),
        _ => Err(invalid()),
    }
}

fn validate_result(
    kind: RemoteBufferResponseKind,
    result: &BufferResult,
) -> Result<(), PbmuxError> {
    if !nonzero(&result.lease_id) || !nonzero(&result.worker_incarnation_id) {
        return Err(invalid());
    }
    if result.completed != (result.reason == BufferReason::None) {
        return Err(invalid());
    }
    if let Some(buffer) = result.buffer {
        if !nonzero(&buffer.buffer_id) || buffer.size_bytes == 0 {
            return Err(invalid());
        }
        if buffer.state.is_terminal() {
            if buffer.ttl_remaining_ms != 0 {
                return Err(invalid());
            }
        } else if buffer.ttl_remaining_ms == 0 || buffer.ttl_remaining_ms > REMOTE_BUFFER_MAX_TTL_MS
        {
            return Err(invalid());
        }
    }
    if result.reservation_id.is_some_and(|id| !nonzero(&id)) {
        return Err(invalid());
    }
    if !result.completed {
        if result.offset != 0
            || result.data_len != 0
            || !result.data.is_empty()
            || result.reservation_id.is_some()
        {
            return Err(invalid());
        }
        return Ok(());
    }
    let buffer = result.buffer.ok_or_else(invalid)?;
    match kind {
        RemoteBufferResponseKind::AllocAck => {
            if buffer.state != BufferState::Allocated
                || result.reservation_id.is_none()
                || result.offset != 0
                || result.data_len != 0
                || !result.data.is_empty()
                || buffer.ttl_remaining_ms > REMOTE_BUFFER_DEFAULT_TTL_MS
            {
                return Err(invalid());
            }
        }
        RemoteBufferResponseKind::Put => {
            if !matches!(buffer.state, BufferState::Allocated | BufferState::Ready)
                || result.reservation_id.is_some()
                || result.data_len == 0
                || !result.data.is_empty()
            {
                return Err(invalid());
            }
        }
        RemoteBufferResponseKind::Data => {
            if buffer.state != BufferState::Ready
                || result.reservation_id.is_some()
                || result.data_len as usize != result.data.len()
                || result.data.len() > MAX_DATA_BODY
            {
                return Err(invalid());
            }
        }
        RemoteBufferResponseKind::Free => {
            if buffer.state != BufferState::Freed
                || result.reservation_id.is_some()
                || result.offset != 0
                || result.data_len != 0
                || !result.data.is_empty()
            {
                return Err(invalid());
            }
        }
        RemoteBufferResponseKind::Stat => {
            if buffer.state.is_terminal()
                || result.reservation_id.is_none()
                || result.offset != 0
                || result.data_len != 0
                || !result.data.is_empty()
            {
                return Err(invalid());
            }
        }
        RemoteBufferResponseKind::Touch => {
            if buffer.state.is_terminal()
                || result.reservation_id.is_some()
                || result.offset != 0
                || result.data_len != 0
                || !result.data.is_empty()
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

fn encode_result(
    kind: RemoteBufferResponseKind,
    result: &BufferResult,
) -> Result<Vec<u8>, PbmuxError> {
    validate_result(kind, result)?;
    let mut bytes = Vec::with_capacity(REMOTE_BUFFER_RESULT_PREFIX_LEN + result.data.len());
    bytes.push(if result.completed { 2 } else { 3 });
    bytes.push(u8::from(result.buffer.is_some()));
    bytes.extend_from_slice(&(result.reason as u16).to_be_bytes());
    bytes.extend_from_slice(&result.lease_id);
    bytes.extend_from_slice(&result.worker_incarnation_id);
    let (buffer_id, state, flags, size, ttl) =
        result.buffer.map_or(([0; 16], 0, 0, 0, 0), |buffer| {
            (
                buffer.buffer_id,
                buffer.state as u8,
                buffer.allocation_flags.bits(),
                buffer.size_bytes,
                buffer.ttl_remaining_ms,
            )
        });
    bytes.extend_from_slice(&buffer_id);
    bytes.push(u8::from(result.reservation_id.is_some()));
    bytes.extend_from_slice(&result.reservation_id.unwrap_or([0; 16]));
    bytes.push(state);
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&flags.to_be_bytes());
    bytes.extend_from_slice(&size.to_be_bytes());
    bytes.extend_from_slice(&result.offset.to_be_bytes());
    bytes.extend_from_slice(&result.data_len.to_be_bytes());
    bytes.extend_from_slice(&ttl.to_be_bytes());
    bytes.extend_from_slice(&result.data);
    Ok(bytes)
}

pub fn build_remote_buffer_result_frames(
    kind: RemoteBufferResponseKind,
    result: &BufferResult,
    request_id: u64,
    first_sequence: u64,
) -> Result<Vec<Frame>, PbmuxError> {
    if request_id == 0 {
        return Err(invalid());
    }
    let payload = encode_result(kind, result)?;
    if payload.len() > MAX_LOGICAL_MESSAGE
        || (payload.len() > MAX_PBMUX_PAYLOAD && kind != RemoteBufferResponseKind::Data)
    {
        return Err(invalid());
    }
    let count = payload.len().div_ceil(MAX_PBMUX_PAYLOAD);
    let mut frames = Vec::with_capacity(count);
    for (index, chunk) in payload.chunks(MAX_PBMUX_PAYLOAD).enumerate() {
        let flags = if count == 1 {
            FLAG_START | FLAG_END
        } else if index == 0 {
            FLAG_START
        } else if index + 1 == count {
            FLAG_END
        } else {
            0
        };
        frames.push(Frame {
            header: Header {
                channel: Channel::RemoteBuffer,
                flags,
                message_type: kind.message_type(),
                request_id,
                sequence: first_sequence
                    .checked_add(index as u64)
                    .ok_or_else(invalid)?,
                fragment_index: index as u32,
                payload_len: chunk.len() as u32,
                logical_message_len: if index == 0 { payload.len() as u32 } else { 0 },
            },
            payload: chunk.to_vec(),
        });
    }
    Ok(frames)
}

pub fn parse_remote_buffer_result_frame(
    frame: &Frame,
) -> Result<(RemoteBufferResponseKind, BufferResult), PbmuxError> {
    let kind = match frame.header.message_type {
        2 => RemoteBufferResponseKind::AllocAck,
        3 => RemoteBufferResponseKind::Put,
        5 => RemoteBufferResponseKind::Data,
        6 => RemoteBufferResponseKind::Free,
        7 => RemoteBufferResponseKind::Stat,
        8 => RemoteBufferResponseKind::Touch,
        _ => return Err(invalid()),
    };
    validate_remote_buffer_result_fragment(frame)?;
    if frame.header.flags != FLAG_START | FLAG_END {
        return Err(invalid());
    }
    parse_remote_buffer_result_payload(kind, &frame.payload).map(|result| (kind, result))
}

pub fn validate_remote_buffer_result_fragment(frame: &Frame) -> Result<(), PbmuxError> {
    let header = &frame.header;
    if header.channel != Channel::RemoteBuffer
        || header.request_id == 0
        || !matches!(header.message_type, 2 | 3 | 5..=8)
    {
        return Err(invalid());
    }
    if header.message_type != 5 {
        if header.flags != FLAG_START | FLAG_END
            || header.fragment_index != 0
            || header.payload_len != REMOTE_BUFFER_RESULT_PREFIX_LEN as u32
            || header.logical_message_len != REMOTE_BUFFER_RESULT_PREFIX_LEN as u32
            || frame.payload.len() != REMOTE_BUFFER_RESULT_PREFIX_LEN
        {
            return Err(invalid());
        }
        return Ok(());
    }
    let start = header.flags & FLAG_START != 0;
    let end = header.flags & FLAG_END != 0;
    if header.flags & FLAG_ACK_REQUIRED != 0 {
        return Err(invalid());
    }
    if start && end {
        if header.flags != FLAG_START | FLAG_END
            || header.fragment_index != 0
            || header.logical_message_len != header.payload_len
        {
            return Err(invalid());
        }
    } else if start {
        if header.flags != FLAG_START
            || header.fragment_index != 0
            || header.logical_message_len as usize > MAX_LOGICAL_MESSAGE
            || header.logical_message_len <= header.payload_len
        {
            return Err(invalid());
        }
    } else if end {
        if header.flags != FLAG_END || header.fragment_index == 0 || header.logical_message_len != 0
        {
            return Err(invalid());
        }
    } else if header.flags != 0 || header.fragment_index == 0 || header.logical_message_len != 0 {
        return Err(invalid());
    }
    Ok(())
}

pub fn parse_remote_buffer_result_payload(
    kind: RemoteBufferResponseKind,
    bytes: &[u8],
) -> Result<BufferResult, PbmuxError> {
    if bytes.len() < REMOTE_BUFFER_RESULT_PREFIX_LEN
        || bytes.len() > MAX_LOGICAL_MESSAGE
        || bytes[1] > 1
        || bytes[52] > 1
        || bytes[70..72] != [0; 2]
    {
        return Err(invalid());
    }
    let completed = match bytes[0] {
        2 => true,
        3 => false,
        _ => return Err(invalid()),
    };
    let reason = BufferReason::try_from(read_u16(bytes, 2)).map_err(|()| invalid())?;
    let lease_id = bytes[4..20].try_into().expect("fixed lease id");
    let worker_incarnation_id = bytes[20..36].try_into().expect("fixed incarnation id");
    let data_len = read_u32(bytes, 92) as usize;
    let body_len = bytes.len() - REMOTE_BUFFER_RESULT_PREFIX_LEN;
    if (kind == RemoteBufferResponseKind::Data && data_len != body_len)
        || (kind != RemoteBufferResponseKind::Data && body_len != 0)
    {
        return Err(invalid());
    }
    let buffer = if bytes[1] == 0 {
        if bytes[36..52] != [0; 16]
            || bytes[69] != 0
            || read_u32(bytes, 72) != 0
            || read_u64(bytes, 76) != 0
            || read_u32(bytes, 96) != 0
        {
            return Err(invalid());
        }
        None
    } else {
        Some(BufferResultRef {
            buffer_id: bytes[36..52].try_into().expect("fixed buffer id"),
            state: BufferState::try_from(bytes[69]).map_err(|()| invalid())?,
            allocation_flags: AllocationFlags::from_bits(read_u32(bytes, 72))
                .ok_or_else(invalid)?,
            size_bytes: read_u64(bytes, 76),
            ttl_remaining_ms: read_u32(bytes, 96),
        })
    };
    let reservation_id = if bytes[52] == 0 {
        if bytes[53..69] != [0; 16] {
            return Err(invalid());
        }
        None
    } else {
        Some(bytes[53..69].try_into().expect("fixed reservation id"))
    };
    let result = BufferResult {
        completed,
        reason,
        lease_id,
        worker_incarnation_id,
        buffer,
        reservation_id,
        offset: read_u64(bytes, 84),
        data_len: read_u32(bytes, 92),
        data: bytes[REMOTE_BUFFER_RESULT_PREFIX_LEN..].to_vec(),
    };
    validate_result(kind, &result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Reassembler, decode, encode};

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/../../docs/protocol/c08_c09_wire_v0_1_vectors_001/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn locked_remote_buffer_vectors_parse_and_responses_round_trip() {
        for name in [
            "GV-C09-01-alloc-request.bin",
            "GV-C09-03-put-small-request.bin",
            "GV-C09-05-get-request.bin",
            "GV-C09-07-stat-request.bin",
            "GV-C09-09-touch-request.bin",
            "GV-C09-11-free-request.bin",
        ] {
            let bytes = fixture(name);
            parse_remote_buffer_request_frame(&decode(&bytes).unwrap()).unwrap();
        }
        for name in [
            "GV-C09-02-alloc-ack.bin",
            "GV-C09-04-put-result.bin",
            "GV-C09-06-data-response.bin",
            "GV-C09-08-stat-result.bin",
            "GV-C09-10-touch-result.bin",
            "GV-C09-12-free-result.bin",
            "GV-C09-13-lost-result.bin",
            "GV-C09-14-stale-lease-result.bin",
        ] {
            let bytes = fixture(name);
            let frame = decode(&bytes).unwrap();
            let (kind, result) = parse_remote_buffer_result_frame(&frame)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            let rebuilt = build_remote_buffer_result_frames(
                kind,
                &result,
                frame.header.request_id,
                frame.header.sequence,
            )
            .unwrap();
            assert_eq!(rebuilt.len(), 1);
            assert_eq!(encode(&rebuilt[0]).unwrap(), bytes, "{name}");
        }
    }

    #[test]
    fn malformed_put_length_rejects_and_exact_four_mib_reassembles_in_69_frames() {
        let malformed = fixture("GV-C09-15-malformed-put-length.bin");
        assert!(parse_remote_buffer_request_frame(&decode(&malformed).unwrap()).is_err());

        let fixture = fixture("GV-C09-16-put-4mib-fragmented.bin");
        let mut cursor = 0;
        let mut count = 0;
        let mut completed = None;
        let mut reassembler = Reassembler::default();
        while cursor < fixture.len() {
            let payload_len =
                u32::from_be_bytes(fixture[cursor + 32..cursor + 36].try_into().unwrap()) as usize;
            let end = cursor + 40 + payload_len;
            let frame = decode(&fixture[cursor..end]).unwrap();
            validate_remote_buffer_request_fragment(&frame).unwrap();
            let message_type = frame.header.message_type;
            if let Some(payload) = reassembler.accept(frame).unwrap() {
                completed =
                    Some(parse_remote_buffer_request_payload(message_type, &payload).unwrap());
            }
            cursor = end;
            count += 1;
        }
        assert_eq!(count, 69);
        match completed.unwrap() {
            RemoteBufferRequest::Put { data, .. } => assert_eq!(data.len(), MAX_PUT_BODY),
            _ => panic!("expected PUT"),
        }
    }

    #[test]
    fn maximum_data_response_fragments_and_reassembles_with_locked_profile() {
        let result = BufferResult {
            completed: true,
            reason: BufferReason::None,
            lease_id: [1; 16],
            worker_incarnation_id: [2; 16],
            buffer: Some(BufferResultRef {
                buffer_id: [3; 16],
                state: BufferState::Ready,
                allocation_flags: AllocationFlags::NONE,
                size_bytes: MAX_DATA_BODY as u64,
                ttl_remaining_ms: 300_000,
            }),
            reservation_id: None,
            offset: 0,
            data_len: MAX_DATA_BODY as u32,
            data: vec![0x5a; MAX_DATA_BODY],
        };
        let frames =
            build_remote_buffer_result_frames(RemoteBufferResponseKind::Data, &result, 7, 0)
                .unwrap();
        assert_eq!(frames.len(), 69);
        assert_eq!(frames[0].header.flags, FLAG_START);
        assert_eq!(frames[68].header.flags, FLAG_END);
        let mut reassembler = Reassembler::default();
        let mut parsed = None;
        for frame in frames {
            validate_remote_buffer_result_fragment(&frame).unwrap();
            if let Some(payload) = reassembler.accept(frame).unwrap() {
                parsed = Some(
                    parse_remote_buffer_result_payload(RemoteBufferResponseKind::Data, &payload)
                        .unwrap(),
                );
            }
        }
        assert_eq!(parsed.unwrap().data.len(), MAX_DATA_BODY);
    }
}
