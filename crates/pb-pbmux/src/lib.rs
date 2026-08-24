#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fmt;

use pb_types::{
    Channel, ControlType, ErrorScope, FLAG_END, FLAG_START, KNOWN_FLAGS, MAX_INFLIGHT_CONTROL,
    MAX_INFLIGHT_PER_DATA_CHANNEL, MAX_LOGICAL_MESSAGE, MAX_PBMUX_PAYLOAD, MAX_PBMUX_PLAINTEXT,
    MAX_REASSEMBLY_PER_CHANNEL, MAX_REASSEMBLY_PER_SESSION, PBMUX_HEADER_LEN, PBMUX_MAGIC,
    PBMUX_VERSION, ReasonCode, is_known_message_type,
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
        if frame.header.fragment_index != partial.expected_fragment
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
}
