#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fmt;

use pb_types::{
    Channel, ControlType, ErrorScope, FLAG_ACK_REQUIRED, FLAG_END, FLAG_START, KNOWN_FLAGS,
    MAX_INFLIGHT_CONTROL, MAX_INFLIGHT_PER_DATA_CHANNEL, MAX_LOGICAL_MESSAGE, MAX_PBMUX_PAYLOAD,
    MAX_PBMUX_PLAINTEXT, MAX_REASSEMBLY_PER_CHANNEL, MAX_REASSEMBLY_PER_SESSION, PBMUX_HEADER_LEN,
    PBMUX_MAGIC, PBMUX_VERSION, ReasonCode, is_known_message_type,
};

mod compute;
mod remote_buffer;
mod resource;

pub use compute::{
    BLAKE3_PROVIDER_ID, BLAKE3_PROVIDER_VERSION, ComputeJobRef, ComputeJobRequest, ComputeJobState,
    ComputeReason, ComputeRequest, ComputeResponse, ComputeResult, ComputeStatus, ComputeSubmit,
    MAX_COMPUTE_INPUT_BYTES, REMOTE_BUFFER_INPUT_KIND, build_compute_request_frame,
    build_compute_response_frame, parse_compute_request_frame, parse_compute_response_frame,
};

pub use remote_buffer::{
    AllocationFlags, BufferReason, BufferResult, BufferResultRef, BufferState, MAX_DATA_BODY,
    MAX_PUT_BODY, RemoteBufferRequest, RemoteBufferResponseKind,
    build_remote_buffer_request_frames, build_remote_buffer_result_frames,
    parse_remote_buffer_request_frame, parse_remote_buffer_request_payload,
    parse_remote_buffer_result_frame, parse_remote_buffer_result_payload,
    validate_remote_buffer_request_fragment, validate_remote_buffer_result_fragment,
};
pub use resource::{
    ExpireNotification, NATIVE_OP_SCRATCH_MAX_BYTES, ReservationResultRef,
    ReservationState as WireReservationState, ResourceClass as WireResourceClass, ResourceReason,
    ResourceRequest, ResourceResponseKind, ResourceResult, ResourceResultState,
    build_expire_notification_frame, build_resource_request_frame, build_resource_result_frame,
    parse_expire_notification_frame, parse_resource_request_frame, parse_resource_result_frame,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Header {
    pub channel: Channel,
    pub flags: u16,
    pub message_type: u16,
    pub request_id: u64,
    pub sequence: u64,
    pub fragment_index: u32,
    pub payload_len: u32,
    pub logical_message_len: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub header: Header,
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PbmuxErrorKind {
    FrameTooLarge,
    HeaderTooShort,
    InvalidMagic,
    InvalidVersion,
    InvalidHeaderLength,
    UnknownChannel,
    ReservedFlags,
    PayloadLengthMismatch,
    LogicalLengthInvalid,
    FragmentInvalid,
    UnsupportedMessage,
    InvalidCommandPayload,
    SequenceMismatch,
    ReassemblyQuota,
    ReassemblyMissing,
    PairingNotCommitted,
    PairConfirmUnexpected,
    InvalidResourcePayload,
    InvalidRemoteBufferPayload,
    InvalidComputePayload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PbmuxError {
    pub kind: PbmuxErrorKind,
    pub scope: ErrorScope,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandType {
    Acquire = 1,
    Renew = 2,
    Release = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandPayload {
    pub command_type: u8,
    pub lease_present: u8,
    pub lease_id: [u8; 16],
    pub command_seq: u64,
    pub trace_id: [u8; 16],
    pub provider_present: u8,
    pub provider_id: u8,
    pub payload_len: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AckPayload {
    pub ack_state: u8,
    pub reason_code: u16,
    pub command_seq: u64,
    pub expected_present: u8,
    pub expected: u64,
    pub result_ref_present: u8,
    pub lease_id: [u8; 16],
    pub worker_incarnation: [u8; 16],
    pub ttl_remaining_ms: u32,
    pub next_command_seq: u64,
    pub digest_present: u8,
    pub digest: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeartbeatPayload {
    pub peer_id: [u8; 32],
    pub worker_incarnation: [u8; 16],
    pub lease_present: u8,
    pub lease_id: [u8; 16],
    pub monotonic_ms: u64,
    pub thermal_status: u8,
    pub headroom_present: u8,
    pub headroom_f32_bits: u32,
    pub battery_pct: u8,
    pub charging: u8,
    pub power_save: u8,
    pub available_bytes: u64,
    pub safe_budget_bytes: u64,
    pub reserved_bytes: u64,
    pub queue_depth: u16,
    pub provider_count: u8,
    pub transport_count: u8,
}

impl PbmuxError {
    const fn session(kind: PbmuxErrorKind) -> Self {
        Self {
            kind,
            scope: ErrorScope::Session,
        }
    }

    const fn logical(kind: PbmuxErrorKind) -> Self {
        Self {
            kind,
            scope: ErrorScope::LogicalMessage,
        }
    }

    pub const fn reason_code(self) -> ReasonCode {
        match self.kind {
            PbmuxErrorKind::FrameTooLarge => ReasonCode::FrameTooLarge,
            PbmuxErrorKind::UnsupportedMessage => ReasonCode::UnsupportedMessage,
            PbmuxErrorKind::InvalidCommandPayload => ReasonCode::UnsupportedMessage,
            PbmuxErrorKind::InvalidResourcePayload
            | PbmuxErrorKind::InvalidRemoteBufferPayload
            | PbmuxErrorKind::InvalidComputePayload => ReasonCode::UnsupportedMessage,
            PbmuxErrorKind::SequenceMismatch => ReasonCode::SequenceError,
            PbmuxErrorKind::PairingNotCommitted => ReasonCode::PairingNotCommitted,
            PbmuxErrorKind::PairConfirmUnexpected => ReasonCode::PairConfirmUnexpected,
            _ => ReasonCode::FrameTooLarge,
        }
    }
}
impl fmt::Display for PbmuxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} ({:?})", self.kind, self.scope)
    }
}

impl std::error::Error for PbmuxError {}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().expect("fixed slice"))
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_be_bytes(bytes[offset..offset + 8].try_into().expect("fixed slice"))
}

const COMMAND_PAYLOAD_LEN: usize = 46;

fn encode_command_payload(payload: &CommandPayload) -> Result<Vec<u8>, PbmuxError> {
    // Validate payload
    if payload.lease_present > 1 || payload.provider_present > 1 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if payload.lease_present == 0 {
        if payload.lease_id != [0u8; 16] {
            return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
        }
    } else {
        // lease_present == 1, lease_id must be non-zero (checked later per command type)
    }
    if payload.provider_present != 0 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if payload.provider_id != 0 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if payload.payload_len != 0 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }

    // Command type specific validation
    match payload.command_type {
        1 => {
            // Acquire
            if payload.lease_present != 0 {
                return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
            }
            if payload.command_seq != 0 {
                return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
            }
        }
        2 => {
            // Renew
            if payload.lease_present != 1 {
                return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
            }
            if payload.lease_id == [0u8; 16] {
                return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
            }
        }
        3 => {
            // Release
            if payload.lease_present != 1 {
                return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
            }
            if payload.lease_id == [0u8; 16] {
                return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
            }
        }
        _ => {
            return Err(PbmuxError::logical(PbmuxErrorKind::UnsupportedMessage));
        }
    }

    // Encode to bytes
    let mut buf = Vec::with_capacity(COMMAND_PAYLOAD_LEN);
    buf.push(payload.command_type);
    buf.push(payload.lease_present);
    buf.extend_from_slice(&payload.lease_id);
    buf.extend_from_slice(&payload.command_seq.to_be_bytes());
    buf.extend_from_slice(&payload.trace_id);
    buf.push(payload.provider_present);
    buf.push(payload.provider_id);
    buf.extend_from_slice(&payload.payload_len.to_be_bytes());
    debug_assert_eq!(buf.len(), COMMAND_PAYLOAD_LEN);
    Ok(buf)
}

fn decode_command_payload(bytes: &[u8]) -> Result<CommandPayload, PbmuxError> {
    if bytes.len() != COMMAND_PAYLOAD_LEN {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    let command_type = bytes[0];
    let lease_present = bytes[1];
    let lease_id = {
        let mut arr = [0u8; 16];
        arr.copy_from_slice(&bytes[2..18]);
        arr
    };
    let command_seq = u64::from_be_bytes({
        let mut arr = [0u8; 8];
        arr.copy_from_slice(&bytes[18..26]);
        arr
    });
    let trace_id = {
        let mut arr = [0u8; 16];
        arr.copy_from_slice(&bytes[26..42]);
        arr
    };
    let provider_present = bytes[42];
    let provider_id = bytes[43];
    let payload_len = u16::from_be_bytes({
        let mut arr = [0u8; 2];
        arr.copy_from_slice(&bytes[44..46]);
        arr
    });

    // Validate presence fields
    if lease_present > 1 || provider_present > 1 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if lease_present == 0 && lease_id != [0u8; 16] {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if provider_present != 0 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if provider_id != 0 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if payload_len != 0 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }

    // Validate command_type specific rules
    match command_type {
        1 => {
            // Acquire
            if lease_present != 0 {
                return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
            }
            if command_seq != 0 {
                return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
            }
        }
        2 => {
            // Renew
            if lease_present != 1 {
                return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
            }
            if lease_id == [0u8; 16] {
                return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
            }
        }
        3 => {
            // Release
            if lease_present != 1 {
                return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
            }
            if lease_id == [0u8; 16] {
                return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
            }
        }
        _ => {
            return Err(PbmuxError::logical(PbmuxErrorKind::UnsupportedMessage));
        }
    }

    Ok(CommandPayload {
        command_type,
        lease_present,
        lease_id,
        command_seq,
        trace_id,
        provider_present,
        provider_id,
        payload_len,
    })
}

const ACK_PAYLOAD_LEN: usize = 98;

fn validate_ack_payload(ack: &AckPayload) -> Result<(), PbmuxError> {
    if ack.expected_present > 1 || ack.result_ref_present > 1 || ack.digest_present > 1 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if ack.expected_present == 0 && ack.expected != 0 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if ack.result_ref_present == 0 {
        let has_ref = ack.lease_id != [0u8; 16]
            || ack.worker_incarnation != [0u8; 16]
            || ack.ttl_remaining_ms != 0
            || ack.next_command_seq != 0;
        if has_ref {
            return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
        }
    }
    // V0.1 never carries a result digest: presence must be 0 and all digest bytes zero.
    if ack.digest_present != 0 || ack.digest != [0u8; 32] {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if !matches!(ack.ack_state, 1 | 2 | 3) {
        return Err(PbmuxError::logical(PbmuxErrorKind::UnsupportedMessage));
    }
    if ack.reason_code > 5 {
        return Err(PbmuxError::logical(PbmuxErrorKind::UnsupportedMessage));
    }
    if ack.ack_state == 1 {
        if ack.reason_code != 0 || ack.expected_present != 0 || ack.result_ref_present != 0 {
            return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
        }
    } else if ack.ack_state == 2 {
        if ack.reason_code != 0 || ack.expected_present != 0 {
            return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
        }
    } else {
        // FAILED
        if ack.reason_code == 0 || ack.result_ref_present != 0 {
            return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
        }
        if ack.reason_code == 3 {
            if ack.expected_present != 1 {
                return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
            }
        } else if ack.expected_present != 0 {
            return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
        }
    }
    Ok(())
}

fn encode_ack_payload(payload: &AckPayload) -> Result<Vec<u8>, PbmuxError> {
    validate_ack_payload(payload)?;
    let mut buf = Vec::with_capacity(ACK_PAYLOAD_LEN);
    buf.push(payload.ack_state);
    buf.extend_from_slice(&payload.reason_code.to_be_bytes());
    buf.extend_from_slice(&payload.command_seq.to_be_bytes());
    buf.push(payload.expected_present);
    buf.extend_from_slice(&payload.expected.to_be_bytes());
    buf.push(payload.result_ref_present);
    buf.extend_from_slice(&payload.lease_id);
    buf.extend_from_slice(&payload.worker_incarnation);
    buf.extend_from_slice(&payload.ttl_remaining_ms.to_be_bytes());
    buf.extend_from_slice(&payload.next_command_seq.to_be_bytes());
    buf.push(payload.digest_present);
    buf.extend_from_slice(&payload.digest);
    debug_assert_eq!(buf.len(), ACK_PAYLOAD_LEN);
    Ok(buf)
}

fn decode_ack_payload(bytes: &[u8]) -> Result<AckPayload, PbmuxError> {
    if bytes.len() != ACK_PAYLOAD_LEN {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    let mut lease_id = [0u8; 16];
    lease_id.copy_from_slice(&bytes[21..37]);
    let mut worker_incarnation = [0u8; 16];
    worker_incarnation.copy_from_slice(&bytes[37..53]);
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&bytes[66..98]);
    let ack = AckPayload {
        ack_state: bytes[0],
        reason_code: read_u16(bytes, 1),
        command_seq: read_u64(bytes, 3),
        expected_present: bytes[11],
        expected: read_u64(bytes, 12),
        result_ref_present: bytes[20],
        lease_id,
        worker_incarnation,
        ttl_remaining_ms: read_u32(bytes, 53),
        next_command_seq: read_u64(bytes, 57),
        digest_present: bytes[65],
        digest,
    };
    validate_ack_payload(&ack)?;
    Ok(ack)
}

const HEARTBEAT_PAYLOAD_LEN: usize = 110;

/// Validate a locked V0.1 HEARTBEAT payload. Every violation is a malformed
/// logical message (oracle verdict REJECT); HEARTBEAT has no unsupported-value
/// classification in the locked addendum (§7.2, §7.3 "invalid logical message",
/// §7.4 "rejected/noncanonical", §9 reject list).
fn validate_heartbeat_payload(h: &HeartbeatPayload) -> Result<(), PbmuxError> {
    if h.lease_present > 1 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if h.lease_present == 0 && h.lease_id != [0u8; 16] {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if h.thermal_status > 6 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if h.headroom_present > 1 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if h.headroom_present == 0 {
        if h.headroom_f32_bits != 0 {
            return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
        }
    } else {
        // IEEE-754 binary32 big-endian values accepted only when finite and
        // >= +0.0. Reject NaN (+/-inf) and the negative-zero/nonnegative sign
        // patterns exactly as the locked oracle does.
        let exponent = (h.headroom_f32_bits >> 23) & 0xFF;
        if h.headroom_f32_bits & 0x8000_0000 != 0 || exponent == 0xFF {
            return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
        }
    }
    if h.battery_pct > 100 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if h.charging > 1 || h.power_save > 1 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    if h.provider_count != 0 || h.transport_count != 0 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    Ok(())
}

fn encode_heartbeat_payload(payload: &HeartbeatPayload) -> Result<Vec<u8>, PbmuxError> {
    validate_heartbeat_payload(payload)?;
    let mut buf = Vec::with_capacity(HEARTBEAT_PAYLOAD_LEN);
    buf.extend_from_slice(&payload.peer_id);
    buf.extend_from_slice(&payload.worker_incarnation);
    buf.push(payload.lease_present);
    buf.extend_from_slice(&payload.lease_id);
    buf.extend_from_slice(&payload.monotonic_ms.to_be_bytes());
    buf.push(payload.thermal_status);
    buf.push(payload.headroom_present);
    buf.extend_from_slice(&payload.headroom_f32_bits.to_be_bytes());
    buf.push(payload.battery_pct);
    buf.push(payload.charging);
    buf.push(payload.power_save);
    buf.extend_from_slice(&payload.available_bytes.to_be_bytes());
    buf.extend_from_slice(&payload.safe_budget_bytes.to_be_bytes());
    buf.extend_from_slice(&payload.reserved_bytes.to_be_bytes());
    buf.extend_from_slice(&payload.queue_depth.to_be_bytes());
    buf.push(payload.provider_count);
    buf.push(payload.transport_count);
    debug_assert_eq!(buf.len(), HEARTBEAT_PAYLOAD_LEN);
    Ok(buf)
}

fn decode_heartbeat_payload(bytes: &[u8]) -> Result<HeartbeatPayload, PbmuxError> {
    if bytes.len() != HEARTBEAT_PAYLOAD_LEN {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    let mut peer_id = [0u8; 32];
    peer_id.copy_from_slice(&bytes[0..32]);
    let mut worker_incarnation = [0u8; 16];
    worker_incarnation.copy_from_slice(&bytes[32..48]);
    let mut lease_id = [0u8; 16];
    lease_id.copy_from_slice(&bytes[49..65]);
    let hb = HeartbeatPayload {
        peer_id,
        worker_incarnation,
        lease_present: bytes[48],
        lease_id,
        monotonic_ms: read_u64(bytes, 65),
        thermal_status: bytes[73],
        headroom_present: bytes[74],
        headroom_f32_bits: read_u32(bytes, 75),
        battery_pct: bytes[79],
        charging: bytes[80],
        power_save: bytes[81],
        available_bytes: read_u64(bytes, 82),
        safe_budget_bytes: read_u64(bytes, 90),
        reserved_bytes: read_u64(bytes, 98),
        queue_depth: read_u16(bytes, 106),
        provider_count: bytes[108],
        transport_count: bytes[109],
    };
    validate_heartbeat_payload(&hb)?;
    Ok(hb)
}

fn validate_fragment_fields(header: &Header) -> Result<(), PbmuxError> {
    let start = header.flags & FLAG_START != 0;
    let end = header.flags & FLAG_END != 0;
    let logical_len = header.logical_message_len as usize;
    let payload_len = header.payload_len as usize;

    if logical_len > MAX_LOGICAL_MESSAGE {
        return Err(PbmuxError::logical(PbmuxErrorKind::LogicalLengthInvalid));
    }
    if start {
        if header.fragment_index != 0 || logical_len < payload_len {
            return Err(PbmuxError::logical(PbmuxErrorKind::FragmentInvalid));
        }
        if end && logical_len != payload_len {
            return Err(PbmuxError::logical(PbmuxErrorKind::LogicalLengthInvalid));
        }
        if !end && logical_len <= payload_len {
            return Err(PbmuxError::logical(PbmuxErrorKind::LogicalLengthInvalid));
        }
    } else if header.fragment_index == 0 || header.logical_message_len != 0 {
        return Err(PbmuxError::logical(PbmuxErrorKind::FragmentInvalid));
    }
    Ok(())
}

pub fn encode(frame: &Frame) -> Result<Vec<u8>, PbmuxError> {
    if frame.payload.len() > MAX_PBMUX_PAYLOAD {
        return Err(PbmuxError::session(PbmuxErrorKind::FrameTooLarge));
    }
    if frame.header.payload_len as usize != frame.payload.len() {
        return Err(PbmuxError::session(PbmuxErrorKind::PayloadLengthMismatch));
    }
    if frame.header.flags & !KNOWN_FLAGS != 0 {
        return Err(PbmuxError::session(PbmuxErrorKind::ReservedFlags));
    }
    if !is_known_message_type(frame.header.channel, frame.header.message_type) {
        return Err(PbmuxError::logical(PbmuxErrorKind::UnsupportedMessage));
    }
    validate_fragment_fields(&frame.header)?;

    let mut output = Vec::with_capacity(PBMUX_HEADER_LEN + frame.payload.len());
    output.extend_from_slice(&PBMUX_MAGIC);
    output.push(PBMUX_VERSION);
    output.push(frame.header.channel as u8);
    output.extend_from_slice(&frame.header.flags.to_be_bytes());
    output.extend_from_slice(&frame.header.message_type.to_be_bytes());
    output.extend_from_slice(&(PBMUX_HEADER_LEN as u16).to_be_bytes());
    output.extend_from_slice(&frame.header.request_id.to_be_bytes());
    output.extend_from_slice(&frame.header.sequence.to_be_bytes());
    output.extend_from_slice(&frame.header.fragment_index.to_be_bytes());
    output.extend_from_slice(&frame.header.payload_len.to_be_bytes());
    output.extend_from_slice(&frame.header.logical_message_len.to_be_bytes());
    output.extend_from_slice(&frame.payload);
    debug_assert_eq!(output.len(), PBMUX_HEADER_LEN + frame.payload.len());
    Ok(output)
}

pub fn decode(bytes: &[u8]) -> Result<Frame, PbmuxError> {
    if bytes.len() < PBMUX_HEADER_LEN {
        return Err(PbmuxError::session(PbmuxErrorKind::HeaderTooShort));
    }
    if bytes.len() > MAX_PBMUX_PLAINTEXT {
        return Err(PbmuxError::session(PbmuxErrorKind::FrameTooLarge));
    }
    if bytes[0..4] != PBMUX_MAGIC {
        return Err(PbmuxError::session(PbmuxErrorKind::InvalidMagic));
    }
    if bytes[4] != PBMUX_VERSION {
        return Err(PbmuxError::session(PbmuxErrorKind::InvalidVersion));
    }
    let channel = Channel::try_from(bytes[5])
        .map_err(|()| PbmuxError::session(PbmuxErrorKind::UnknownChannel))?;
    let flags = read_u16(bytes, 6);
    if flags & !KNOWN_FLAGS != 0 {
        return Err(PbmuxError::session(PbmuxErrorKind::ReservedFlags));
    }
    let header_len = read_u16(bytes, 10);
    if header_len as usize != PBMUX_HEADER_LEN {
        return Err(PbmuxError::session(PbmuxErrorKind::InvalidHeaderLength));
    }
    let payload_len = read_u32(bytes, 32);
    if payload_len as usize > MAX_PBMUX_PAYLOAD
        || PBMUX_HEADER_LEN + payload_len as usize != bytes.len()
    {
        return Err(PbmuxError::session(PbmuxErrorKind::PayloadLengthMismatch));
    }
    let message_type = read_u16(bytes, 8);
    if !is_known_message_type(channel, message_type) {
        return Err(PbmuxError::logical(PbmuxErrorKind::UnsupportedMessage));
    }
    let header = Header {
        channel,
        flags,
        message_type,
        request_id: read_u64(bytes, 12),
        sequence: read_u64(bytes, 20),
        fragment_index: read_u32(bytes, 28),
        payload_len,
        logical_message_len: read_u32(bytes, 36),
    };
    validate_fragment_fields(&header)?;
    Ok(Frame {
        header,
        payload: bytes[PBMUX_HEADER_LEN..].to_vec(),
    })
}

pub fn pair_confirm_frame(request_id: u64, sequence: u64) -> Result<Frame, PbmuxError> {
    if request_id == 0 {
        return Err(PbmuxError::logical(PbmuxErrorKind::PairConfirmUnexpected));
    }
    Ok(Frame {
        header: Header {
            channel: Channel::Control,
            flags: FLAG_START | FLAG_END,
            message_type: ControlType::PairConfirm as u16,
            request_id,
            sequence,
            fragment_index: 0,
            payload_len: 0,
            logical_message_len: 0,
        },
        payload: Vec::new(),
    })
}

pub fn validate_pair_confirm(frame: &Frame) -> Result<(), PbmuxError> {
    let h = &frame.header;
    if h.channel != Channel::Control
        || h.message_type != ControlType::PairConfirm as u16
        || h.flags != FLAG_START | FLAG_END
        || h.request_id == 0
        || h.fragment_index != 0
        || h.payload_len != 0
        || h.logical_message_len != 0
        || !frame.payload.is_empty()
    {
        return Err(PbmuxError::logical(PbmuxErrorKind::PairConfirmUnexpected));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchMode {
    PairingControlOnly,
    Committed,
}

/// Authorize a decoded frame for a given dispatch mode. In committed mode any
/// non-PairConfirm frame is allowed; while pairing only a small CONTROL set is
/// permitted (Ping/Pong/Error/SessionClose).
pub fn authorize_dispatch(frame: &Frame, mode: DispatchMode) -> Result<(), PbmuxError> {
    if frame.header.channel == Channel::Control
        && frame.header.message_type == ControlType::PairConfirm as u16
    {
        if mode == DispatchMode::Committed {
            return Err(PbmuxError::logical(PbmuxErrorKind::PairConfirmUnexpected));
        }
        return validate_pair_confirm(frame);
    }
    if mode == DispatchMode::Committed {
        return Ok(());
    }
    let allowed = frame.header.channel == Channel::Control
        && matches!(
            ControlType::try_from(frame.header.message_type),
            Ok(ControlType::Ping
                | ControlType::Pong
                | ControlType::Error
                | ControlType::SessionClose)
        );
    if allowed {
        Ok(())
    } else {
        Err(PbmuxError::logical(PbmuxErrorKind::PairingNotCommitted))
    }
}

/// Build a locked CONTROL/5 COMMAND frame. The frame profile exactly matches
/// addendum §5.1: START|END|ACK_REQUIRED, nonzero request_id, single fragment,
/// payload and logical length both 46. The private COMMAND codec is the only
/// validator of the payload bytes.
pub fn build_command_frame(
    payload: &CommandPayload,
    request_id: u64,
    sequence: u64,
) -> Result<Frame, PbmuxError> {
    if request_id == 0 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    let payload = encode_command_payload(payload)?;
    debug_assert_eq!(payload.len(), COMMAND_PAYLOAD_LEN);
    Ok(Frame {
        header: Header {
            channel: Channel::Control,
            flags: FLAG_START | FLAG_END | FLAG_ACK_REQUIRED,
            message_type: ControlType::Command as u16,
            request_id,
            sequence,
            fragment_index: 0,
            payload_len: COMMAND_PAYLOAD_LEN as u32,
            logical_message_len: COMMAND_PAYLOAD_LEN as u32,
        },
        payload,
    })
}

/// Parse and strictly validate a C07 COMMAND frame. Every locked profile field
/// must match exactly before the private decoder runs; any mismatch is a
/// malformed logical message.
pub fn parse_command_frame(frame: &Frame) -> Result<CommandPayload, PbmuxError> {
    let h = &frame.header;
    if h.channel != Channel::Control
        || h.message_type != ControlType::Command as u16
        || h.flags != FLAG_START | FLAG_END | FLAG_ACK_REQUIRED
        || h.request_id == 0
        || h.fragment_index != 0
        || h.payload_len != COMMAND_PAYLOAD_LEN as u32
        || h.logical_message_len != COMMAND_PAYLOAD_LEN as u32
        || frame.payload.len() != COMMAND_PAYLOAD_LEN
    {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    decode_command_payload(&frame.payload)
}

/// Build a locked CONTROL/6 COMMAND_ACK frame (addendum §6.1): flags
/// START|END, nonzero request_id matching the COMMAND being acknowledged,
/// payload and logical length 98. No request correlation state is invented
/// here; only the nonzero request_id profile rule is enforced.
pub fn build_command_ack_frame(
    payload: &AckPayload,
    request_id: u64,
    sequence: u64,
) -> Result<Frame, PbmuxError> {
    if request_id == 0 {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    let payload = encode_ack_payload(payload)?;
    debug_assert_eq!(payload.len(), ACK_PAYLOAD_LEN);
    Ok(Frame {
        header: Header {
            channel: Channel::Control,
            flags: FLAG_START | FLAG_END,
            message_type: ControlType::CommandAck as u16,
            request_id,
            sequence,
            fragment_index: 0,
            payload_len: ACK_PAYLOAD_LEN as u32,
            logical_message_len: ACK_PAYLOAD_LEN as u32,
        },
        payload,
    })
}

/// Parse an exact locked CONTROL/6 COMMAND_ACK frame and return its payload.
pub fn parse_command_ack_frame(frame: &Frame) -> Result<AckPayload, PbmuxError> {
    let h = &frame.header;
    if h.channel != Channel::Control
        || h.message_type != ControlType::CommandAck as u16
        || h.flags != FLAG_START | FLAG_END
        || h.request_id == 0
        || h.fragment_index != 0
        || h.payload_len != ACK_PAYLOAD_LEN as u32
        || h.logical_message_len != ACK_PAYLOAD_LEN as u32
        || frame.payload.len() != ACK_PAYLOAD_LEN
    {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    decode_ack_payload(&frame.payload)
}

/// Build a locked METRICS/1 HEARTBEAT frame (addendum §7.1): unsolicited
/// one-way with `request_id = 0`, flags START|END, payload and logical
/// length 110. No ACK_REQUIRED is ever set.
pub fn build_heartbeat_frame(
    payload: &HeartbeatPayload,
    sequence: u64,
) -> Result<Frame, PbmuxError> {
    let payload = encode_heartbeat_payload(payload)?;
    debug_assert_eq!(payload.len(), HEARTBEAT_PAYLOAD_LEN);
    Ok(Frame {
        header: Header {
            channel: Channel::Metrics,
            flags: FLAG_START | FLAG_END,
            message_type: 1,
            request_id: 0,
            sequence,
            fragment_index: 0,
            payload_len: HEARTBEAT_PAYLOAD_LEN as u32,
            logical_message_len: HEARTBEAT_PAYLOAD_LEN as u32,
        },
        payload,
    })
}

/// Parse an exact locked METRICS/1 HEARTBEAT frame and return its payload.
/// The heartbeat never carries a request_id and must be a single frame.
pub fn parse_heartbeat_frame(frame: &Frame) -> Result<HeartbeatPayload, PbmuxError> {
    let h = &frame.header;
    if h.channel != Channel::Metrics
        || h.message_type != 1
        || h.flags != FLAG_START | FLAG_END
        || h.request_id != 0
        || h.fragment_index != 0
        || h.payload_len != HEARTBEAT_PAYLOAD_LEN as u32
        || h.logical_message_len != HEARTBEAT_PAYLOAD_LEN as u32
        || frame.payload.len() != HEARTBEAT_PAYLOAD_LEN
    {
        return Err(PbmuxError::logical(PbmuxErrorKind::InvalidCommandPayload));
    }
    decode_heartbeat_payload(&frame.payload)
}

#[derive(Debug, Default)]
pub struct SequenceTracker {
    expected: u64,
}

impl SequenceTracker {
    pub const fn expected(&self) -> u64 {
        self.expected
    }

    pub fn accept(&mut self, received: u64) -> Result<(), PbmuxError> {
        if received != self.expected {
            return Err(PbmuxError::session(PbmuxErrorKind::SequenceMismatch));
        }
        self.expected = self
            .expected
            .checked_add(1)
            .ok_or_else(|| PbmuxError::session(PbmuxErrorKind::SequenceMismatch))?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdmissionTicket {
    channel: Channel,
    bytes: usize,
}

#[derive(Clone, Debug, Default)]
pub struct ReassemblyAccounting {
    channel_bytes: [usize; 6],
    channel_count: [usize; 6],
    session_bytes: usize,
}

impl ReassemblyAccounting {
    pub fn try_start(
        &mut self,
        channel: Channel,
        logical_bytes: usize,
    ) -> Result<AdmissionTicket, PbmuxError> {
        if logical_bytes > MAX_LOGICAL_MESSAGE {
            return Err(PbmuxError::logical(PbmuxErrorKind::LogicalLengthInvalid));
        }
        let index = channel as usize;
        let count_limit = if channel == Channel::Control {
            MAX_INFLIGHT_CONTROL
        } else {
            MAX_INFLIGHT_PER_DATA_CHANNEL
        };
        let channel_after = self.channel_bytes[index]
            .checked_add(logical_bytes)
            .ok_or_else(|| PbmuxError::logical(PbmuxErrorKind::ReassemblyQuota))?;
        let session_after = self
            .session_bytes
            .checked_add(logical_bytes)
            .ok_or_else(|| PbmuxError::logical(PbmuxErrorKind::ReassemblyQuota))?;
        if self.channel_count[index] >= count_limit
            || channel_after > MAX_REASSEMBLY_PER_CHANNEL
            || session_after > MAX_REASSEMBLY_PER_SESSION
        {
            return Err(PbmuxError::logical(PbmuxErrorKind::ReassemblyQuota));
        }
        self.channel_bytes[index] = channel_after;
        self.channel_count[index] += 1;
        self.session_bytes = session_after;
        Ok(AdmissionTicket {
            channel,
            bytes: logical_bytes,
        })
    }

    pub fn finish(&mut self, ticket: AdmissionTicket) {
        let index = ticket.channel as usize;
        self.channel_bytes[index] -= ticket.bytes;
        self.channel_count[index] -= 1;
        self.session_bytes -= ticket.bytes;
    }

    pub const fn session_bytes(&self) -> usize {
        self.session_bytes
    }

    pub const fn channel_bytes(&self, channel: Channel) -> usize {
        self.channel_bytes[channel as usize]
    }

    pub const fn channel_count(&self, channel: Channel) -> usize {
        self.channel_count[channel as usize]
    }
}

#[derive(Debug)]
struct PartialMessage {
    ticket: AdmissionTicket,
    message_type: u16,
    ack_required: bool,
    expected_fragment: u32,
    expected_len: usize,
    payload: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct Reassembler {
    accounting: ReassemblyAccounting,
    partial: HashMap<(Channel, u64), PartialMessage>,
}

impl Reassembler {
    pub const fn accounting(&self) -> &ReassemblyAccounting {
        &self.accounting
    }

    fn abort_partial(&mut self, key: (Channel, u64), kind: PbmuxErrorKind) -> PbmuxError {
        if let Some(partial) = self.partial.remove(&key) {
            self.accounting.finish(partial.ticket);
        }
        PbmuxError::logical(kind)
    }

    pub fn accept(&mut self, frame: Frame) -> Result<Option<Vec<u8>>, PbmuxError> {
        let start = frame.header.flags & FLAG_START != 0;
        let end = frame.header.flags & FLAG_END != 0;
        let key = (frame.header.channel, frame.header.request_id);
        if start && end {
            if self.partial.contains_key(&key) {
                return Err(self.abort_partial(key, PbmuxErrorKind::FragmentInvalid));
            }
            return Ok(Some(frame.payload));
        }
        if start {
            if self.partial.contains_key(&key) {
                return Err(self.abort_partial(key, PbmuxErrorKind::FragmentInvalid));
            }
            let expected_len = frame.header.logical_message_len as usize;
            let ticket = self
                .accounting
                .try_start(frame.header.channel, expected_len)?;
            let mut payload = Vec::with_capacity(expected_len);
            payload.extend_from_slice(&frame.payload);
            self.partial.insert(
                key,
                PartialMessage {
                    ticket,
                    message_type: frame.header.message_type,
                    ack_required: frame.header.flags & FLAG_ACK_REQUIRED != 0,
                    expected_fragment: 1,
                    expected_len,
                    payload,
                },
            );
            return Ok(None);
        }

        let Some(partial) = self.partial.get(&key) else {
            return Err(PbmuxError::logical(PbmuxErrorKind::ReassemblyMissing));
        };
        if frame.header.message_type != partial.message_type
            || frame.header.flags & FLAG_ACK_REQUIRED
                != u16::from(partial.ack_required) * FLAG_ACK_REQUIRED
            || frame.header.fragment_index != partial.expected_fragment
            || partial.payload.len() + frame.payload.len() > partial.expected_len
        {
            return Err(self.abort_partial(key, PbmuxErrorKind::FragmentInvalid));
        }
        let partial = self.partial.get_mut(&key).expect("partial exists");
        partial.payload.extend_from_slice(&frame.payload);
        partial.expected_fragment += 1;
        if !end {
            return Ok(None);
        }
        if partial.payload.len() != partial.expected_len {
            return Err(self.abort_partial(key, PbmuxErrorKind::LogicalLengthInvalid));
        }
        let completed = self.partial.remove(&key).expect("partial exists");
        self.accounting.finish(completed.ticket);
        Ok(Some(completed.payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fragment(request_id: u64, index: u32, flags: u16, payload: &[u8], logical: u32) -> Frame {
        Frame {
            header: Header {
                channel: Channel::Resource,
                flags,
                message_type: 1,
                request_id,
                sequence: index as u64,
                fragment_index: index,
                payload_len: payload.len() as u32,
                logical_message_len: logical,
            },
            payload: payload.to_vec(),
        }
    }

    #[test]
    fn pair_confirm_matches_locked_golden_bytes() {
        let encoded = encode(&pair_confirm_frame(0x0102_0304_0506_0708, 0).unwrap()).unwrap();
        let expected = [
            0x50, 0x42, 0x4d, 0x31, 0x01, 0x00, 0x00, 0x03, 0x00, 0x08, 0x00, 0x28, 0x01, 0x02,
            0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        assert_eq!(encoded, expected);
        validate_pair_confirm(&decode(&encoded).unwrap()).unwrap();
    }

    #[test]
    fn sequence_gap_and_duplicate_fail_session_scope() {
        let mut tracker = SequenceTracker::default();
        tracker.accept(0).unwrap();
        let gap = tracker.accept(2).unwrap_err();
        assert_eq!(gap.kind, PbmuxErrorKind::SequenceMismatch);
        assert_eq!(gap.scope, ErrorScope::Session);
        let duplicate = tracker.accept(0).unwrap_err();
        assert_eq!(duplicate.scope, ErrorScope::Session);
    }

    #[test]
    fn quota_rejects_before_accounting_change() {
        let mut accounting = ReassemblyAccounting::default();
        let first = accounting
            .try_start(Channel::RemoteBuffer, MAX_LOGICAL_MESSAGE)
            .unwrap();
        let second = accounting
            .try_start(Channel::RemoteBuffer, MAX_LOGICAL_MESSAGE)
            .unwrap();
        let before = accounting.channel_bytes(Channel::RemoteBuffer);
        assert!(
            accounting
                .try_start(Channel::RemoteBuffer, MAX_LOGICAL_MESSAGE)
                .is_err()
        );
        assert_eq!(accounting.channel_bytes(Channel::RemoteBuffer), before);
        accounting.finish(first);
        accounting.finish(second);
    }

    #[test]
    fn wrong_fragment_index_aborts_only_its_partial_and_releases_once() {
        let mut reassembler = Reassembler::default();
        reassembler
            .accept(fragment(11, 0, FLAG_START, b"aaaa", 8))
            .unwrap();
        reassembler
            .accept(fragment(22, 0, FLAG_START, b"bbbb", 8))
            .unwrap();
        assert_eq!(reassembler.accounting().session_bytes(), 16);

        let error = reassembler
            .accept(fragment(11, 2, FLAG_END, b"cccc", 0))
            .unwrap_err();
        assert_eq!(error.kind, PbmuxErrorKind::FragmentInvalid);
        assert_eq!(reassembler.accounting().session_bytes(), 8);
        assert_eq!(reassembler.accounting().channel_count(Channel::Resource), 1);

        let completed = reassembler
            .accept(fragment(22, 1, FLAG_END, b"dddd", 0))
            .unwrap()
            .unwrap();
        assert_eq!(completed, b"bbbbdddd");
        assert_eq!(reassembler.accounting().session_bytes(), 0);

        reassembler
            .accept(fragment(11, 0, FLAG_START, b"eeee", 8))
            .unwrap();
        assert_eq!(reassembler.accounting().session_bytes(), 8);
    }

    #[test]
    fn payload_overflow_aborts_and_restores_accounting() {
        let mut reassembler = Reassembler::default();
        reassembler
            .accept(fragment(33, 0, FLAG_START, b"aaaa", 6))
            .unwrap();
        let error = reassembler
            .accept(fragment(33, 1, FLAG_END, b"bbb", 0))
            .unwrap_err();
        assert_eq!(error.kind, PbmuxErrorKind::FragmentInvalid);
        assert_eq!(reassembler.accounting().session_bytes(), 0);
        assert_eq!(reassembler.accounting().channel_count(Channel::Resource), 0);
    }

    #[test]
    fn incorrect_final_length_aborts_and_restores_accounting() {
        let mut reassembler = Reassembler::default();
        reassembler
            .accept(fragment(44, 0, FLAG_START, b"aaaa", 8))
            .unwrap();
        let error = reassembler
            .accept(fragment(44, 1, FLAG_END, b"bbb", 0))
            .unwrap_err();
        assert_eq!(error.kind, PbmuxErrorKind::LogicalLengthInvalid);
        assert_eq!(reassembler.accounting().session_bytes(), 0);
        assert_eq!(reassembler.accounting().channel_bytes(Channel::Resource), 0);
    }

    #[test]
    fn pairing_gate_is_central_and_strict() {
        let pair = pair_confirm_frame(1, 0).unwrap();
        authorize_dispatch(&pair, DispatchMode::PairingControlOnly).unwrap();
        assert!(authorize_dispatch(&pair, DispatchMode::Committed).is_err());

        let command = Frame {
            header: Header {
                channel: Channel::Control,
                flags: FLAG_START | FLAG_END,
                message_type: ControlType::Command as u16,
                request_id: 2,
                sequence: 0,
                fragment_index: 0,
                payload_len: 0,
                logical_message_len: 0,
            },
            payload: Vec::new(),
        };
        assert_eq!(
            authorize_dispatch(&command, DispatchMode::PairingControlOnly)
                .unwrap_err()
                .kind,
            PbmuxErrorKind::PairingNotCommitted
        );
    }

    #[test]
    fn gv_c07_01_acquire() {
        // Build a valid Acquire command payload
        let payload = CommandPayload {
            command_type: 1, // Acquire
            lease_present: 0,
            lease_id: [0u8; 16],
            command_seq: 0,
            trace_id: [0u8; 16],
            provider_present: 0,
            provider_id: 0,
            payload_len: 0,
        };
        let encoded = encode_command_payload(&payload).unwrap();
        assert_eq!(encoded.len(), COMMAND_PAYLOAD_LEN);
        // Decode it back
        let decoded = decode_command_payload(&encoded).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn gv_c07_02_renew() {
        let mut lease_id = [0u8; 16];
        lease_id[0] = 1; // non-zero
        let payload = CommandPayload {
            command_type: 2, // Renew
            lease_present: 1,
            lease_id,
            command_seq: 0xdead_beef_dead_beef, // any value
            trace_id: [0u8; 16],
            provider_present: 0,
            provider_id: 0,
            payload_len: 0,
        };
        let encoded = encode_command_payload(&payload).unwrap();
        assert_eq!(encoded.len(), COMMAND_PAYLOAD_LEN);
        let decoded = decode_command_payload(&encoded).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn gv_c07_03_release() {
        let mut lease_id = [0u8; 16];
        lease_id[15] = 1; // non-zero
        let payload = CommandPayload {
            command_type: 3, // Release
            lease_present: 1,
            lease_id,
            command_seq: 0xfeed_feed_feed_feed, // any value
            trace_id: [0u8; 16],
            provider_present: 0,
            provider_id: 0,
            payload_len: 0,
        };
        let encoded = encode_command_payload(&payload).unwrap();
        assert_eq!(encoded.len(), COMMAND_PAYLOAD_LEN);
        let decoded = decode_command_payload(&encoded).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn gv_c07_04_acquire_with_lease() {
        let mut lease_id = [0u8; 16];
        lease_id[0] = 1; // non-zero, which is invalid for Acquire
        let payload = CommandPayload {
            command_type: 1,  // Acquire
            lease_present: 0, // but lease_id non-zero -> invalid
            lease_id,
            command_seq: 0,
            trace_id: [0u8; 16],
            provider_present: 0,
            provider_id: 0,
            payload_len: 0,
        };
        let err = encode_command_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        assert_eq!(err.scope, ErrorScope::LogicalMessage);
    }

    #[test]
    fn gv_c07_05_renew_without_lease() {
        let payload = CommandPayload {
            command_type: 2,  // Renew
            lease_present: 0, // missing lease
            lease_id: [0u8; 16],
            command_seq: 0,
            trace_id: [0u8; 16],
            provider_present: 0,
            provider_id: 0,
            payload_len: 0,
        };
        let err = encode_command_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        assert_eq!(err.scope, ErrorScope::LogicalMessage);
    }

    #[test]
    fn renew_with_zero_lease_id() {
        let payload = CommandPayload {
            command_type: 2, // Renew
            lease_present: 1,
            lease_id: [0u8; 16], // all zero
            command_seq: 0,
            trace_id: [0u8; 16],
            provider_present: 0,
            provider_id: 0,
            payload_len: 0,
        };
        let err = encode_command_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        assert_eq!(err.scope, ErrorScope::LogicalMessage);
    }

    #[test]
    fn wrong_payload_size() {
        // Too short
        let mut buf = vec![0u8; COMMAND_PAYLOAD_LEN - 1];
        let err = decode_command_payload(&buf).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        // Too long
        let mut buf = vec![0u8; COMMAND_PAYLOAD_LEN + 1];
        let err = decode_command_payload(&buf).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
    }

    #[test]
    fn invalid_presence() {
        // lease_present = 2 (invalid)
        let mut payload = CommandPayload {
            command_type: 1,
            lease_present: 2,
            lease_id: [0u8; 16],
            command_seq: 0,
            trace_id: [0u8; 16],
            provider_present: 0,
            provider_id: 0,
            payload_len: 0,
        };
        let err = encode_command_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        // provider_present = 2
        payload.lease_present = 0;
        payload.provider_present = 2;
        let err = encode_command_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
    }

    #[test]
    fn unknown_command_type() {
        let payload = CommandPayload {
            command_type: 99, // unknown
            lease_present: 0,
            lease_id: [0u8; 16],
            command_seq: 0,
            trace_id: [0u8; 16],
            provider_present: 0,
            provider_id: 0,
            payload_len: 0,
        };
        let err = encode_command_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::UnsupportedMessage);
        assert_eq!(err.scope, ErrorScope::LogicalMessage);
        // Also test decoding unknown command_type from bytes
        let mut bytes = [0u8; COMMAND_PAYLOAD_LEN];
        bytes[0] = 99;
        let err = decode_command_payload(&bytes).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::UnsupportedMessage);
        assert_eq!(err.scope, ErrorScope::LogicalMessage);
    }
    const fn valid_completed_ack() -> AckPayload {
        AckPayload {
            ack_state: 2,
            reason_code: 0,
            command_seq: 0,
            expected_present: 0,
            expected: 0,
            result_ref_present: 0,
            lease_id: [0u8; 16],
            worker_incarnation: [0u8; 16],
            ttl_remaining_ms: 0,
            next_command_seq: 0,
            digest_present: 0,
            digest: [0u8; 32],
        }
    }

    #[test]
    fn gv_c07_06_ack_acquire_completed_fixture() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-C07-06-ack-acquire-completed.bin"
        );
        assert_eq!(fixture.len(), PBMUX_HEADER_LEN + ACK_PAYLOAD_LEN);
        let payload = &fixture[PBMUX_HEADER_LEN..];
        let ack = decode_ack_payload(payload).unwrap();
        assert_eq!(ack.ack_state, 2);
        assert_eq!(ack.reason_code, 0);
        assert_eq!(ack.command_seq, 0);
        assert_eq!(ack.expected_present, 0);
        assert_eq!(ack.expected, 0);
        assert_eq!(ack.result_ref_present, 1);
        assert_eq!(
            ack.lease_id,
            [
                0x00u8, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc,
                0xdd, 0xee, 0xff
            ]
        );
        assert_eq!(
            ack.worker_incarnation,
            [
                0x10u8, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98, 0xa9, 0xba, 0xcb, 0xdc,
                0xed, 0xfe, 0x0f
            ]
        );
        assert_eq!(ack.ttl_remaining_ms, 60_000);
        assert_eq!(ack.next_command_seq, 0);
        assert_eq!(ack.digest_present, 0);
        assert_eq!(ack.digest, [0u8; 32]);
        assert_eq!(encode_ack_payload(&ack).unwrap(), payload);
    }

    #[test]
    fn gv_c07_07_ack_renew_completed_fixture() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-C07-07-ack-renew-completed.bin"
        );
        let payload = &fixture[PBMUX_HEADER_LEN..];
        let ack = decode_ack_payload(payload).unwrap();
        assert_eq!(ack.ack_state, 2);
        assert_eq!(ack.reason_code, 0);
        assert_eq!(ack.result_ref_present, 1);
        assert_eq!(ack.ttl_remaining_ms, 60_000);
        assert_eq!(ack.next_command_seq, 1);
        assert_eq!(encode_ack_payload(&ack).unwrap(), payload);
    }
    #[test]
    fn gv_c07_08_ack_release_completed_fixture() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-C07-08-ack-release-completed.bin"
        );
        let payload = &fixture[PBMUX_HEADER_LEN..];
        let ack = decode_ack_payload(payload).unwrap();
        assert_eq!(ack.ack_state, 2);
        assert_eq!(ack.command_seq, 1);
        assert_eq!(ack.result_ref_present, 0);
        assert_eq!(ack.lease_id, [0u8; 16]);
        assert_eq!(ack.worker_incarnation, [0u8; 16]);
        assert_eq!(ack.ttl_remaining_ms, 0);
        assert_eq!(ack.next_command_seq, 0);
        assert_eq!(encode_ack_payload(&ack).unwrap(), payload);
    }

    #[test]
    fn gv_c07_09_ack_out_of_order_fixture() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-C07-09-ack-out-of-order.bin"
        );
        let payload = &fixture[PBMUX_HEADER_LEN..];
        let ack = decode_ack_payload(payload).unwrap();
        assert_eq!(ack.ack_state, 3);
        assert_eq!(ack.reason_code, 3);
        assert_eq!(ack.command_seq, 7);
        assert_eq!(ack.expected_present, 1);
        assert_eq!(ack.expected, 1);
        assert_eq!(ack.result_ref_present, 0);
        assert_eq!(encode_ack_payload(&ack).unwrap(), payload);
    }

    #[test]
    fn gv_c07_10_ack_invalid_missing_expected_rejected() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-C07-10-ack-invalid-missing-expected.bin"
        );
        let err = decode_ack_payload(&fixture[PBMUX_HEADER_LEN..]).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        assert_eq!(err.scope, ErrorScope::LogicalMessage);
        assert_eq!(err.reason_code(), ReasonCode::UnsupportedMessage);
    }

    #[test]
    fn ack_accepted_parser_compatible_form() {
        let mut ack = valid_completed_ack();
        ack.ack_state = 1;
        let encoded = encode_ack_payload(&ack).unwrap();
        let decoded = decode_ack_payload(&encoded).unwrap();
        assert_eq!(decoded.ack_state, 1);
        assert_eq!(decoded.reason_code, 0);
        assert_eq!(decoded.expected_present, 0);
        assert_eq!(decoded.result_ref_present, 0);
        assert_eq!(decoded.digest_present, 0);
    }

    #[test]
    fn ack_wrong_payload_length() {
        for len_ in [ACK_PAYLOAD_LEN - 1, ACK_PAYLOAD_LEN + 1] {
            let err = decode_ack_payload(&vec![0u8; len_]).unwrap_err();
            assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
            assert_eq!(err.scope, ErrorScope::LogicalMessage);
        }
    }

    #[test]
    fn ack_invalid_ack_state() {
        for state in [0u8, 4, 255u8] {
            let mut payload = valid_completed_ack();
            payload.ack_state = state;
            let err = encode_ack_payload(&payload).unwrap_err();
            assert_eq!(err.kind, PbmuxErrorKind::UnsupportedMessage);
            assert_eq!(err.scope, ErrorScope::LogicalMessage);
            let mut bytes = [0u8; ACK_PAYLOAD_LEN];
            bytes[0] = state;
            let err = decode_ack_payload(&bytes).unwrap_err();
            assert_eq!(err.kind, PbmuxErrorKind::UnsupportedMessage);
            assert_eq!(err.scope, ErrorScope::LogicalMessage);
        }
    }
    #[test]
    fn ack_invalid_presence_byte() {
        let mut payload = valid_completed_ack();
        payload.expected_present = 2;
        let err = encode_ack_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        payload = valid_completed_ack();
        payload.result_ref_present = 2;
        let err = encode_ack_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        payload = valid_completed_ack();
        payload.digest_present = 2;
        let err = encode_ack_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        let mut bytes = [0u8; ACK_PAYLOAD_LEN];
        bytes[0] = 2;
        bytes[11] = 2;
        let err = decode_ack_payload(&bytes).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
    }

    #[test]
    fn ack_nonzero_absent_expected() {
        let mut payload = valid_completed_ack();
        payload.expected = 7;
        let err = encode_ack_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        let mut bytes = [0u8; ACK_PAYLOAD_LEN];
        bytes[2] = 2;
        bytes[12] = 1;
        let err = decode_ack_payload(&bytes).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
    }

    #[test]
    fn ack_nonzero_absent_result_ref_fields() {
        let mut payload = valid_completed_ack();
        payload.lease_id[0] = 1;
        let err = encode_ack_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        payload = valid_completed_ack();
        payload.worker_incarnation[0] = 1;
        let err = encode_ack_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        payload = valid_completed_ack();
        payload.ttl_remaining_ms = 1;
        let err = encode_ack_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        payload = valid_completed_ack();
        payload.next_command_seq = 1;
        let err = encode_ack_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
    }

    #[test]
    fn ack_nonzero_absent_digest() {
        let mut payload = valid_completed_ack();
        payload.digest[0] = 1;
        let err = encode_ack_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        payload = valid_completed_ack();
        payload.digest_present = 1;
        let err = encode_ack_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
    }

    #[test]
    fn ack_reason_unsupported() {
        for reason in [6u16, 65535u16] {
            let mut payload = valid_completed_ack();
            payload.reason_code = reason;
            let err = encode_ack_payload(&payload).unwrap_err();
            assert_eq!(err.kind, PbmuxErrorKind::UnsupportedMessage);
            let mut bytes = [0u8; ACK_PAYLOAD_LEN];
            bytes[0] = 3;
            bytes[1..3].copy_from_slice(&reason.to_be_bytes());
            let err = decode_ack_payload(&bytes).unwrap_err();
            assert_eq!(err.kind, PbmuxErrorKind::UnsupportedMessage);
            assert_eq!(err.scope, ErrorScope::LogicalMessage);
        }
    }

    #[test]
    fn ack_state_reason_combo_rules() {
        let mut payload = valid_completed_ack();
        payload.ack_state = 3;
        let err = encode_ack_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        payload = valid_completed_ack();
        payload.reason_code = 1;
        let err = encode_ack_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        payload = valid_completed_ack();
        payload.ack_state = 3;
        payload.reason_code = 3;
        let err = encode_ack_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        payload = valid_completed_ack();
        payload.ack_state = 1;
        payload.reason_code = 1;
        let err = encode_ack_payload(&payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
    }
    const fn valid_heartbeat() -> HeartbeatPayload {
        HeartbeatPayload {
            peer_id: [
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
                0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
                0x1c, 0x1d, 0x1e, 0x1f,
            ],
            worker_incarnation: [
                0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98, 0xa9, 0xba, 0xcb, 0xdc, 0xed,
                0xfe, 0x0f,
            ],
            lease_present: 0,
            lease_id: [0u8; 16],
            monotonic_ms: 1_000_000,
            thermal_status: 2,
            headroom_present: 1,
            headroom_f32_bits: 0x3F00_0000,
            battery_pct: 80,
            charging: 1,
            power_save: 0,
            available_bytes: 0x4000_0000,
            safe_budget_bytes: 0x0800_0000,
            reserved_bytes: 0,
            queue_depth: 3,
            provider_count: 0,
            transport_count: 0,
        }
    }

    fn assert_hb_reject(payload: &[u8]) {
        let err = decode_heartbeat_payload(payload).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        assert_eq!(err.scope, ErrorScope::LogicalMessage);
        assert_eq!(err.reason_code(), ReasonCode::UnsupportedMessage);
    }

    #[test]
    fn gv_hb_01_no_lease_fixture() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-HB-01-no-lease.bin"
        );
        assert_eq!(fixture.len(), PBMUX_HEADER_LEN + HEARTBEAT_PAYLOAD_LEN);
        let payload = &fixture[PBMUX_HEADER_LEN..];
        let hb = decode_heartbeat_payload(payload).unwrap();
        assert_eq!(hb.lease_present, 0);
        assert_eq!(hb.lease_id, [0u8; 16]);
        assert_eq!(hb.peer_id[0], 0x00);
        assert_eq!(hb.monotonic_ms, 1_000_000);
        assert_eq!(hb.thermal_status, 2);
        assert_eq!(hb.headroom_present, 1);
        assert_eq!(hb.headroom_f32_bits, 0x3F00_0000);
        assert_eq!(hb.battery_pct, 80);
        assert_eq!(hb.charging, 1);
        assert_eq!(hb.power_save, 0);
        assert_eq!(hb.available_bytes, 0x4000_0000);
        assert_eq!(hb.safe_budget_bytes, 0x0800_0000);
        assert_eq!(hb.reserved_bytes, 0);
        assert_eq!(hb.queue_depth, 3);
        assert_eq!(hb.provider_count, 0);
        assert_eq!(hb.transport_count, 0);
        assert_eq!(encode_heartbeat_payload(&hb).unwrap(), payload);
    }

    #[test]
    fn gv_hb_02_active_lease_fixture() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-HB-02-active-lease.bin"
        );
        let payload = &fixture[PBMUX_HEADER_LEN..];
        let hb = decode_heartbeat_payload(payload).unwrap();
        assert_eq!(hb.lease_present, 1);
        assert_eq!(
            hb.lease_id,
            [
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff
            ]
        );
        assert_eq!(encode_heartbeat_payload(&hb).unwrap(), payload);
    }

    #[test]
    fn gv_hb_03_headroom_absent_fixture() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-HB-03-headroom-absent.bin"
        );
        let payload = &fixture[PBMUX_HEADER_LEN..];
        let hb = decode_heartbeat_payload(payload).unwrap();
        assert_eq!(hb.headroom_present, 0);
        assert_eq!(hb.headroom_f32_bits, 0);
        assert_eq!(encode_heartbeat_payload(&hb).unwrap(), payload);
    }

    #[test]
    fn gv_hb_04_headroom_present_fixture() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-HB-04-headroom-present.bin"
        );
        let payload = &fixture[PBMUX_HEADER_LEN..];
        let hb = decode_heartbeat_payload(payload).unwrap();
        assert_eq!(hb.lease_present, 1);
        assert_eq!(hb.headroom_present, 1);
        assert_eq!(hb.headroom_f32_bits, 0x3E80_0000);
        assert_eq!(encode_heartbeat_payload(&hb).unwrap(), payload);
    }

    #[test]
    fn gv_hb_05_invalid_provider_count_rejected() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-HB-05-invalid-provider-count.bin"
        );
        assert_hb_reject(&fixture[PBMUX_HEADER_LEN..]);
    }

    #[test]
    fn gv_hb_06_invalid_transport_count_rejected() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-HB-06-invalid-transport-count.bin"
        );
        assert_hb_reject(&fixture[PBMUX_HEADER_LEN..]);
    }

    #[test]
    fn gv_hb_07_invalid_thermal_status_rejected() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-HB-07-invalid-thermal-status.bin"
        );
        assert_hb_reject(&fixture[PBMUX_HEADER_LEN..]);
    }

    #[test]
    fn gv_hb_08_invalid_negative_zero_headroom_rejected() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-HB-08-invalid-negative-zero-headroom.bin"
        );
        assert_hb_reject(&fixture[PBMUX_HEADER_LEN..]);
    }
    #[test]
    fn hb_wrong_payload_length() {
        for len_ in [HEARTBEAT_PAYLOAD_LEN - 1, HEARTBEAT_PAYLOAD_LEN + 1] {
            let err = decode_heartbeat_payload(&vec![0u8; len_]).unwrap_err();
            assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
            assert_eq!(err.scope, ErrorScope::LogicalMessage);
        }
    }

    #[test]
    fn hb_invalid_presence_byte() {
        let mut hb = valid_heartbeat();
        hb.lease_present = 2;
        let err = encode_heartbeat_payload(&hb).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        hb = valid_heartbeat();
        hb.headroom_present = 2;
        let err = encode_heartbeat_payload(&hb).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        let mut bytes = [0u8; HEARTBEAT_PAYLOAD_LEN];
        bytes[48] = 2;
        assert_hb_reject(&bytes);
        let mut bytes = [0u8; HEARTBEAT_PAYLOAD_LEN];
        bytes[74] = 2;
        assert_hb_reject(&bytes);
    }

    #[test]
    fn hb_nonzero_absent_lease() {
        let mut hb = valid_heartbeat();
        hb.lease_id[0] = 1;
        let err = encode_heartbeat_payload(&hb).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        let mut bytes = [0u8; HEARTBEAT_PAYLOAD_LEN];
        bytes[49] = 1;
        assert_hb_reject(&bytes);
    }

    #[test]
    fn hb_invalid_thermal_status() {
        for status in [7u8, 255u8] {
            let mut hb = valid_heartbeat();
            hb.thermal_status = status;
            let err = encode_heartbeat_payload(&hb).unwrap_err();
            assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
            let mut bytes = [0u8; HEARTBEAT_PAYLOAD_LEN];
            bytes[73] = status;
            assert_hb_reject(&bytes);
        }
    }

    #[test]
    fn hb_headroom_absent_nonzero_bits() {
        let mut hb = valid_heartbeat();
        hb.headroom_present = 0;
        hb.headroom_f32_bits = 1;
        let err = encode_heartbeat_payload(&hb).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        let mut bytes = [0u8; HEARTBEAT_PAYLOAD_LEN];
        bytes[75] = 1;
        assert_hb_reject(&bytes);
    }

    #[test]
    fn hb_headroom_rejects_nan_and_infinities() {
        for bits in [0x7FC0_0000u32, 0x7F80_0000u32, 0xFF80_0000u32] {
            let mut hb = valid_heartbeat();
            hb.headroom_f32_bits = bits;
            let err = encode_heartbeat_payload(&hb).unwrap_err();
            assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
            let mut bytes = [0u8; HEARTBEAT_PAYLOAD_LEN];
            bytes[75..79].copy_from_slice(&bits.to_be_bytes());
            assert_hb_reject(&bytes);
        }
    }

    #[test]
    fn hb_headroom_rejects_negative_and_negative_zero() {
        for bits in [0x8000_0000u32, 0xBF80_0000u32] {
            let mut hb = valid_heartbeat();
            hb.headroom_f32_bits = bits;
            let err = encode_heartbeat_payload(&hb).unwrap_err();
            assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
            let mut bytes = [0u8; HEARTBEAT_PAYLOAD_LEN];
            bytes[75..79].copy_from_slice(&bits.to_be_bytes());
            assert_hb_reject(&bytes);
        }
    }

    #[test]
    fn hb_headroom_accepts_positive_values() {
        for bits in [0x0000_0000u32, 0x3F80_0000u32, 0x3E80_0000u32] {
            let mut hb = valid_heartbeat();
            hb.headroom_present = 1;
            hb.headroom_f32_bits = bits;
            encode_heartbeat_payload(&hb).unwrap();
            let mut bytes = [0u8; HEARTBEAT_PAYLOAD_LEN];
            bytes[74] = 1;
            bytes[75..79].copy_from_slice(&bits.to_be_bytes());
            decode_heartbeat_payload(&bytes).unwrap();
        }
    }

    #[test]
    fn hb_invalid_charging_power_save() {
        let mut hb = valid_heartbeat();
        hb.charging = 2;
        let err = encode_heartbeat_payload(&hb).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        hb = valid_heartbeat();
        hb.power_save = 2;
        let err = encode_heartbeat_payload(&hb).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        let mut bytes = [0u8; HEARTBEAT_PAYLOAD_LEN];
        bytes[80] = 2;
        assert_hb_reject(&bytes);
        let mut bytes = [0u8; HEARTBEAT_PAYLOAD_LEN];
        bytes[81] = 2;
        assert_hb_reject(&bytes);
    }

    #[test]
    fn hb_nonzero_counts_rejected() {
        for (off, val) in [(108u32, 1u8), (109, 1)] {
            let mut hb = valid_heartbeat();
            if off == 108 {
                hb.provider_count = val;
            } else {
                hb.transport_count = val;
            }
            let err = encode_heartbeat_payload(&hb).unwrap_err();
            assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
            let mut bytes = [0u8; HEARTBEAT_PAYLOAD_LEN];
            bytes[off as usize] = val;
            assert_hb_reject(&bytes);
        }
    }

    #[test]
    fn hb_battery_out_of_range() {
        for pct in [101u8, 255u8] {
            let mut hb = valid_heartbeat();
            hb.battery_pct = pct;
            let err = encode_heartbeat_payload(&hb).unwrap_err();
            assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
            let mut bytes = [0u8; HEARTBEAT_PAYLOAD_LEN];
            bytes[79] = pct;
            assert_hb_reject(&bytes);
        }
    }
    #[test]
    fn command_frame_encode_decode_round_trip() {
        let payload = CommandPayload {
            command_type: 1, // ACQUIRE
            lease_present: 0,
            lease_id: [0u8; 16],
            command_seq: 0,
            trace_id: [0x11; 16],
            provider_present: 0,
            provider_id: 0,
            payload_len: 0,
        };
        let frame = build_command_frame(&payload, 0x0102_0304_0506_0708, 7).unwrap();
        assert_eq!(frame.header.channel, Channel::Control);
        assert_eq!(frame.header.message_type, ControlType::Command as u16);
        assert_eq!(
            frame.header.flags,
            FLAG_START | FLAG_END | FLAG_ACK_REQUIRED
        );
        assert_eq!(frame.header.fragment_index, 0);
        let bytes = encode(&frame).unwrap();
        let parsed = parse_command_frame(&decode(&bytes).unwrap()).unwrap();
        assert_eq!(parsed, payload);
        let rebuilt =
            build_command_frame(&parsed, frame.header.request_id, frame.header.sequence).unwrap();
        assert_eq!(encode(&rebuilt).unwrap(), bytes);
    }

    #[test]
    fn command_ack_frame_encode_decode_round_trip() {
        let frame =
            build_command_ack_frame(&valid_completed_ack(), 0x0102_0304_0506_0708, 3).unwrap();
        assert_eq!(frame.header.channel, Channel::Control);
        assert_eq!(frame.header.message_type, ControlType::CommandAck as u16);
        assert_eq!(frame.header.flags, FLAG_START | FLAG_END);
        let bytes = encode(&frame).unwrap();
        let parsed = parse_command_ack_frame(&decode(&bytes).unwrap()).unwrap();
        assert_eq!(parsed, valid_completed_ack());
        let rebuilt =
            build_command_ack_frame(&parsed, frame.header.request_id, frame.header.sequence)
                .unwrap();
        assert_eq!(encode(&rebuilt).unwrap(), bytes);
    }

    #[test]
    fn heartbeat_frame_encode_decode_round_trip() {
        let frame = build_heartbeat_frame(&valid_heartbeat(), 4).unwrap();
        assert_eq!(frame.header.channel, Channel::Metrics);
        assert_eq!(frame.header.message_type, 1);
        assert_eq!(frame.header.request_id, 0);
        assert_eq!(frame.header.flags, FLAG_START | FLAG_END);
        let bytes = encode(&frame).unwrap();
        let parsed = parse_heartbeat_frame(&decode(&bytes).unwrap()).unwrap();
        assert_eq!(parsed, valid_heartbeat());
        let rebuilt = build_heartbeat_frame(&parsed, frame.header.sequence).unwrap();
        assert_eq!(encode(&rebuilt).unwrap(), bytes);
    }

    #[test]
    fn command_frame_matches_locked_fixture_bytes() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-C07-01-acquire-valid.bin"
        );
        let frame = decode(fixture).unwrap();
        let payload = parse_command_frame(&frame).unwrap();
        assert_eq!(payload.command_type, 1);
        let rebuilt =
            build_command_frame(&payload, frame.header.request_id, frame.header.sequence).unwrap();
        assert_eq!(encode(&rebuilt).unwrap(), fixture);
    }

    #[test]
    fn command_ack_frame_matches_locked_fixture_bytes() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-C07-06-ack-acquire-completed.bin"
        );
        let frame = decode(fixture).unwrap();
        let payload = parse_command_ack_frame(&frame).unwrap();
        assert_eq!(payload.ack_state, 2);
        let rebuilt =
            build_command_ack_frame(&payload, frame.header.request_id, frame.header.sequence)
                .unwrap();
        assert_eq!(encode(&rebuilt).unwrap(), fixture);
    }

    #[test]
    fn heartbeat_frame_matches_locked_fixture_bytes() {
        let fixture = include_bytes!(
            "../../../docs/protocol/c07_wire_v0_1_vectors_002/GV-HB-01-no-lease.bin"
        );
        let frame = decode(fixture).unwrap();
        let payload = parse_heartbeat_frame(&frame).unwrap();
        assert_eq!(payload.thermal_status, 2);
        let rebuilt = build_heartbeat_frame(&payload, frame.header.sequence).unwrap();
        assert_eq!(encode(&rebuilt).unwrap(), fixture);
    }
    #[test]
    fn command_frame_negative_profile() {
        let payload = CommandPayload {
            command_type: 1,
            lease_present: 0,
            lease_id: [0u8; 16],
            command_seq: 0,
            trace_id: [0x11; 16],
            provider_present: 0,
            provider_id: 0,
            payload_len: 0,
        };
        let base = build_command_frame(&payload, 0x0102_0304_0506_0708, 1).unwrap();
        let reject = |frame: &Frame| {
            let err = parse_command_frame(frame).unwrap_err();
            assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
            assert_eq!(err.scope, ErrorScope::LogicalMessage);
        };

        let mut frame = base.clone();
        frame.header.channel = Channel::Resource;
        reject(&frame);
        let mut frame = base.clone();
        frame.header.message_type = ControlType::Pong as u16;
        reject(&frame);
        let mut frame = base.clone();
        frame.header.flags = FLAG_START | FLAG_END; // missing ACK_REQUIRED
        reject(&frame);
        let mut frame = base.clone();
        frame.header.request_id = 0;
        reject(&frame);
        let mut frame = base.clone();
        frame.header.payload_len = COMMAND_PAYLOAD_LEN as u32 + 1;
        reject(&frame);
        let mut frame = base.clone();
        frame.header.logical_message_len = COMMAND_PAYLOAD_LEN as u32 + 1;
        reject(&frame);
        let mut frame = base.clone();
        frame.header.fragment_index = 1;
        reject(&frame);

        // Wrong payload body length must also fail the profile pre-decoder.
        let mut frame = base;
        frame.payload.pop();
        reject(&frame);

        let err = build_command_frame(&payload, 0, 1).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        assert_eq!(err.scope, ErrorScope::LogicalMessage);
    }

    #[test]
    fn command_ack_frame_negative_profile() {
        let base =
            build_command_ack_frame(&valid_completed_ack(), 0x0102_0304_0506_0708, 1).unwrap();
        let reject = |frame: &Frame| {
            let err = parse_command_ack_frame(frame).unwrap_err();
            assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
            assert_eq!(err.scope, ErrorScope::LogicalMessage);
        };

        let mut frame = base.clone();
        frame.header.channel = Channel::Metrics;
        reject(&frame);
        let mut frame = base.clone();
        frame.header.message_type = ControlType::Ping as u16;
        reject(&frame);
        let mut frame = base.clone();
        frame.header.flags = FLAG_START | FLAG_END | FLAG_ACK_REQUIRED;
        reject(&frame);
        let mut frame = base.clone();
        frame.header.request_id = 0;
        reject(&frame);
        let mut frame = base.clone();
        frame.header.payload_len = ACK_PAYLOAD_LEN as u32 - 1;
        reject(&frame);
        let mut frame = base.clone();
        frame.header.logical_message_len = ACK_PAYLOAD_LEN as u32 - 1;
        reject(&frame);
        let mut frame = base;
        frame.header.fragment_index = 1;
        reject(&frame);

        let err = build_command_ack_frame(&valid_completed_ack(), 0, 1).unwrap_err();
        assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
        assert_eq!(err.scope, ErrorScope::LogicalMessage);
    }

    #[test]
    fn heartbeat_frame_negative_profile() {
        let base = build_heartbeat_frame(&valid_heartbeat(), 1).unwrap();
        let reject = |frame: &Frame| {
            let err = parse_heartbeat_frame(frame).unwrap_err();
            assert_eq!(err.kind, PbmuxErrorKind::InvalidCommandPayload);
            assert_eq!(err.scope, ErrorScope::LogicalMessage);
        };

        let mut frame = base.clone();
        frame.header.channel = Channel::Control;
        reject(&frame);
        let mut frame = base.clone();
        frame.header.message_type = 2;
        reject(&frame);
        let mut frame = base.clone();
        frame.header.flags = FLAG_START | FLAG_END | FLAG_ACK_REQUIRED;
        reject(&frame);
        let mut frame = base.clone();
        frame.header.request_id = 1; // heartbeat must carry request_id = 0
        reject(&frame);
        let mut frame = base.clone();
        frame.header.payload_len = HEARTBEAT_PAYLOAD_LEN as u32 - 1;
        reject(&frame);
        let mut frame = base.clone();
        frame.header.logical_message_len = HEARTBEAT_PAYLOAD_LEN as u32 - 1;
        reject(&frame);
        let mut frame = base;
        frame.header.fragment_index = 2;
        reject(&frame);
    }
}
