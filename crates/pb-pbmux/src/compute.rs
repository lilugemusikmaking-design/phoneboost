use pb_types::{Channel, FLAG_ACK_REQUIRED, FLAG_END, FLAG_START};

use crate::{Frame, Header, PbmuxError, PbmuxErrorKind, read_u16, read_u64};

pub const COMPUTE_SUBMIT_LEN: usize = 84;
pub const COMPUTE_JOB_REQUEST_LEN: usize = 48;
pub const COMPUTE_STATUS_LEN: usize = 56;
pub const COMPUTE_RESULT_LEN: usize = 88;
pub const MAX_COMPUTE_INPUT_BYTES: u64 = 128 * 1024 * 1024;
pub const BLAKE3_PROVIDER_ID: u8 = 1;
pub const BLAKE3_PROVIDER_VERSION: u8 = 1;
pub const REMOTE_BUFFER_INPUT_KIND: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComputeSubmit {
    pub lease_id: [u8; 16],
    pub worker_incarnation_id: [u8; 16],
    pub reservation_id: [u8; 16],
    pub provider_id: u8,
    pub provider_version: u8,
    pub input_kind: u8,
    pub buffer_id: [u8; 16],
    pub input_offset: u64,
    pub input_length: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComputeJobRequest {
    pub lease_id: [u8; 16],
    pub worker_incarnation_id: [u8; 16],
    pub job_id: [u8; 16],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputeRequest {
    Submit(ComputeSubmit),
    Status(ComputeJobRequest),
    Cancel(ComputeJobRequest),
}

impl ComputeRequest {
    pub const fn lease_id(&self) -> &[u8; 16] {
        match self {
            Self::Submit(request) => &request.lease_id,
            Self::Status(request) | Self::Cancel(request) => &request.lease_id,
        }
    }

    pub const fn worker_incarnation_id(&self) -> &[u8; 16] {
        match self {
            Self::Submit(request) => &request.worker_incarnation_id,
            Self::Status(request) | Self::Cancel(request) => &request.worker_incarnation_id,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ComputeJobState {
    Invalid = 0,
    Accepted = 1,
    Running = 2,
    Completed = 3,
    Failed = 4,
    Cancelled = 5,
}

impl ComputeJobState {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

impl TryFrom<u8> for ComputeJobState {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::Invalid,
            1 => Self::Accepted,
            2 => Self::Running,
            3 => Self::Completed,
            4 => Self::Failed,
            5 => Self::Cancelled,
            _ => return Err(()),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ComputeReason {
    None = 0,
    StaleControllerLease = 1,
    WrongWorkerIncarnation = 2,
    UnsupportedProvider = 3,
    InvalidInput = 4,
    BufferNotFound = 5,
    BufferNotOwned = 6,
    BufferWrongIncarnation = 7,
    BufferInvalidState = 8,
    BufferLost = 9,
    BufferFreed = 10,
    BufferEvicted = 11,
    InputTooLarge = 12,
    ReservationInvalid = 13,
    ResourceExhausted = 14,
    RequestIdConflict = 15,
    IdempotenceTableFull = 16,
    JobNotFound = 17,
    JobNotOwned = 18,
    JobNotCancellable = 19,
    ProviderTimeout = 20,
    ProviderFailed = 21,
    SessionLost = 22,
    UnsupportedMessage = 23,
    InternalError = 24,
}

impl TryFrom<u16> for ComputeReason {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::None,
            1 => Self::StaleControllerLease,
            2 => Self::WrongWorkerIncarnation,
            3 => Self::UnsupportedProvider,
            4 => Self::InvalidInput,
            5 => Self::BufferNotFound,
            6 => Self::BufferNotOwned,
            7 => Self::BufferWrongIncarnation,
            8 => Self::BufferInvalidState,
            9 => Self::BufferLost,
            10 => Self::BufferFreed,
            11 => Self::BufferEvicted,
            12 => Self::InputTooLarge,
            13 => Self::ReservationInvalid,
            14 => Self::ResourceExhausted,
            15 => Self::RequestIdConflict,
            16 => Self::IdempotenceTableFull,
            17 => Self::JobNotFound,
            18 => Self::JobNotOwned,
            19 => Self::JobNotCancellable,
            20 => Self::ProviderTimeout,
            21 => Self::ProviderFailed,
            22 => Self::SessionLost,
            23 => Self::UnsupportedMessage,
            24 => Self::InternalError,
            _ => return Err(()),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComputeJobRef {
    pub job_id: [u8; 16],
    pub provider_id: u8,
    pub provider_version: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComputeStatus {
    pub state: ComputeJobState,
    pub reason: ComputeReason,
    pub lease_id: [u8; 16],
    pub worker_incarnation_id: [u8; 16],
    pub job: Option<ComputeJobRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComputeResult {
    pub state: ComputeJobState,
    pub reason: ComputeReason,
    pub lease_id: [u8; 16],
    pub worker_incarnation_id: [u8; 16],
    pub job: Option<ComputeJobRef>,
    pub digest: Option<[u8; 32]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputeResponse {
    Status(ComputeStatus),
    Result(ComputeResult),
    Cancel(ComputeStatus),
}

fn invalid() -> PbmuxError {
    PbmuxError::logical(PbmuxErrorKind::InvalidComputePayload)
}

fn nonzero(id: &[u8; 16]) -> bool {
    id.iter().any(|byte| *byte != 0)
}

fn request_profile(frame: &Frame, expected_len: usize) -> bool {
    frame.header.channel == Channel::Compute
        && frame.header.flags == FLAG_START | FLAG_END | FLAG_ACK_REQUIRED
        && frame.header.request_id != 0
        && frame.header.fragment_index == 0
        && frame.header.payload_len == expected_len as u32
        && frame.header.logical_message_len == expected_len as u32
        && frame.payload.len() == expected_len
}

fn response_profile(frame: &Frame, expected_len: usize) -> bool {
    frame.header.channel == Channel::Compute
        && frame.header.flags == FLAG_START | FLAG_END
        && frame.header.request_id != 0
        && frame.header.fragment_index == 0
        && frame.header.payload_len == expected_len as u32
        && frame.header.logical_message_len == expected_len as u32
        && frame.payload.len() == expected_len
}

pub fn build_compute_request_frame(
    request: &ComputeRequest,
    request_id: u64,
    sequence: u64,
) -> Result<Frame, PbmuxError> {
    if request_id == 0 {
        return Err(invalid());
    }
    let (message_type, payload) = match request {
        ComputeRequest::Submit(request) => {
            if !nonzero(&request.lease_id)
                || !nonzero(&request.worker_incarnation_id)
                || !nonzero(&request.reservation_id)
                || !nonzero(&request.buffer_id)
            {
                return Err(invalid());
            }
            let mut payload = Vec::with_capacity(COMPUTE_SUBMIT_LEN);
            payload.extend_from_slice(&request.lease_id);
            payload.extend_from_slice(&request.worker_incarnation_id);
            payload.extend_from_slice(&request.reservation_id);
            payload.extend_from_slice(&[
                request.provider_id,
                request.provider_version,
                request.input_kind,
                0,
            ]);
            payload.extend_from_slice(&request.buffer_id);
            payload.extend_from_slice(&request.input_offset.to_be_bytes());
            payload.extend_from_slice(&request.input_length.to_be_bytes());
            (1, payload)
        }
        ComputeRequest::Status(request) => {
            if !nonzero(&request.lease_id)
                || !nonzero(&request.worker_incarnation_id)
                || !nonzero(&request.job_id)
            {
                return Err(invalid());
            }
            let mut payload = Vec::with_capacity(COMPUTE_JOB_REQUEST_LEN);
            payload.extend_from_slice(&request.lease_id);
            payload.extend_from_slice(&request.worker_incarnation_id);
            payload.extend_from_slice(&request.job_id);
            (2, payload)
        }
        ComputeRequest::Cancel(request) => {
            if !nonzero(&request.lease_id)
                || !nonzero(&request.worker_incarnation_id)
                || !nonzero(&request.job_id)
            {
                return Err(invalid());
            }
            let mut payload = Vec::with_capacity(COMPUTE_JOB_REQUEST_LEN);
            payload.extend_from_slice(&request.lease_id);
            payload.extend_from_slice(&request.worker_incarnation_id);
            payload.extend_from_slice(&request.job_id);
            (4, payload)
        }
    };
    Ok(Frame {
        header: Header {
            channel: Channel::Compute,
            flags: FLAG_START | FLAG_END | FLAG_ACK_REQUIRED,
            message_type,
            request_id,
            sequence,
            fragment_index: 0,
            payload_len: payload.len() as u32,
            logical_message_len: payload.len() as u32,
        },
        payload,
    })
}

pub fn parse_compute_request_frame(frame: &Frame) -> Result<ComputeRequest, PbmuxError> {
    match frame.header.message_type {
        1 => {
            if !request_profile(frame, COMPUTE_SUBMIT_LEN) || frame.payload[51] != 0 {
                return Err(invalid());
            }
            let request = ComputeSubmit {
                lease_id: frame.payload[0..16].try_into().expect("fixed lease"),
                worker_incarnation_id: frame.payload[16..32].try_into().expect("fixed incarnation"),
                reservation_id: frame.payload[32..48].try_into().expect("fixed reservation"),
                provider_id: frame.payload[48],
                provider_version: frame.payload[49],
                input_kind: frame.payload[50],
                buffer_id: frame.payload[52..68].try_into().expect("fixed buffer"),
                input_offset: read_u64(&frame.payload, 68),
                input_length: read_u64(&frame.payload, 76),
            };
            if !nonzero(&request.lease_id)
                || !nonzero(&request.worker_incarnation_id)
                || !nonzero(&request.reservation_id)
                || !nonzero(&request.buffer_id)
            {
                return Err(invalid());
            }
            Ok(ComputeRequest::Submit(request))
        }
        2 | 4 => {
            if !request_profile(frame, COMPUTE_JOB_REQUEST_LEN) {
                return Err(invalid());
            }
            let request = ComputeJobRequest {
                lease_id: frame.payload[0..16].try_into().expect("fixed lease"),
                worker_incarnation_id: frame.payload[16..32].try_into().expect("fixed incarnation"),
                job_id: frame.payload[32..48].try_into().expect("fixed job"),
            };
            if !nonzero(&request.lease_id)
                || !nonzero(&request.worker_incarnation_id)
                || !nonzero(&request.job_id)
            {
                return Err(invalid());
            }
            if frame.header.message_type == 2 {
                Ok(ComputeRequest::Status(request))
            } else {
                Ok(ComputeRequest::Cancel(request))
            }
        }
        _ => Err(invalid()),
    }
}

fn valid_absent_lookup_reason(reason: ComputeReason) -> bool {
    matches!(
        reason,
        ComputeReason::StaleControllerLease
            | ComputeReason::WrongWorkerIncarnation
            | ComputeReason::JobNotFound
            | ComputeReason::JobNotOwned
            | ComputeReason::UnsupportedMessage
            | ComputeReason::InternalError
    )
}

fn validate_status(status: &ComputeStatus, cancel: bool) -> Result<(), PbmuxError> {
    if !nonzero(&status.lease_id) || !nonzero(&status.worker_incarnation_id) {
        return Err(invalid());
    }
    match status.job {
        None => {
            if status.state != ComputeJobState::Invalid
                || !valid_absent_lookup_reason(status.reason)
            {
                return Err(invalid());
            }
        }
        Some(job) => {
            if !nonzero(&job.job_id)
                || (job.provider_id, job.provider_version)
                    != (BLAKE3_PROVIDER_ID, BLAKE3_PROVIDER_VERSION)
            {
                return Err(invalid());
            }
            let valid = if cancel {
                (status.state == ComputeJobState::Cancelled && status.reason == ComputeReason::None)
                    || (status.state.is_terminal()
                        && status.reason == ComputeReason::JobNotCancellable)
            } else {
                matches!(
                    status.state,
                    ComputeJobState::Accepted | ComputeJobState::Running
                ) && status.reason == ComputeReason::None
            };
            if !valid {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

fn valid_admission_reason(reason: ComputeReason) -> bool {
    matches!(
        reason,
        ComputeReason::StaleControllerLease
            | ComputeReason::WrongWorkerIncarnation
            | ComputeReason::UnsupportedProvider
            | ComputeReason::InvalidInput
            | ComputeReason::BufferNotFound
            | ComputeReason::BufferNotOwned
            | ComputeReason::BufferWrongIncarnation
            | ComputeReason::BufferInvalidState
            | ComputeReason::BufferLost
            | ComputeReason::BufferFreed
            | ComputeReason::BufferEvicted
            | ComputeReason::InputTooLarge
            | ComputeReason::ReservationInvalid
            | ComputeReason::ResourceExhausted
            | ComputeReason::RequestIdConflict
            | ComputeReason::IdempotenceTableFull
            | ComputeReason::UnsupportedMessage
            | ComputeReason::InternalError
    )
}

fn validate_result(result: &ComputeResult) -> Result<(), PbmuxError> {
    if !nonzero(&result.lease_id) || !nonzero(&result.worker_incarnation_id) {
        return Err(invalid());
    }
    match result.job {
        None => {
            if result.state != ComputeJobState::Invalid
                || !valid_admission_reason(result.reason)
                || result.digest.is_some()
            {
                return Err(invalid());
            }
        }
        Some(job) => {
            if !nonzero(&job.job_id)
                || (job.provider_id, job.provider_version)
                    != (BLAKE3_PROVIDER_ID, BLAKE3_PROVIDER_VERSION)
            {
                return Err(invalid());
            }
            let valid = match result.state {
                ComputeJobState::Completed => {
                    result.reason == ComputeReason::None && result.digest.is_some()
                }
                ComputeJobState::Failed => {
                    matches!(
                        result.reason,
                        ComputeReason::BufferInvalidState
                            | ComputeReason::BufferLost
                            | ComputeReason::BufferFreed
                            | ComputeReason::BufferEvicted
                            | ComputeReason::ResourceExhausted
                            | ComputeReason::ProviderTimeout
                            | ComputeReason::ProviderFailed
                            | ComputeReason::SessionLost
                            | ComputeReason::InternalError
                    ) && result.digest.is_none()
                }
                ComputeJobState::Cancelled => {
                    matches!(
                        result.reason,
                        ComputeReason::None | ComputeReason::SessionLost
                    ) && result.digest.is_none()
                }
                _ => false,
            };
            if !valid {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

fn encode_status(status: &ComputeStatus, cancel: bool) -> Result<Vec<u8>, PbmuxError> {
    validate_status(status, cancel)?;
    let mut payload = Vec::with_capacity(COMPUTE_STATUS_LEN);
    payload.push(status.state as u8);
    payload.push(u8::from(status.job.is_some()));
    payload.extend_from_slice(&(status.reason as u16).to_be_bytes());
    payload.extend_from_slice(&status.lease_id);
    payload.extend_from_slice(&status.worker_incarnation_id);
    if let Some(job) = status.job {
        payload.extend_from_slice(&job.job_id);
        payload.push(job.provider_id);
        payload.push(job.provider_version);
    } else {
        payload.extend_from_slice(&[0; 18]);
    }
    payload.extend_from_slice(&[0; 2]);
    Ok(payload)
}

fn encode_result(result: &ComputeResult) -> Result<Vec<u8>, PbmuxError> {
    validate_result(result)?;
    let mut payload = Vec::with_capacity(COMPUTE_RESULT_LEN);
    payload.push(result.state as u8);
    payload.push(u8::from(result.job.is_some()));
    payload.extend_from_slice(&(result.reason as u16).to_be_bytes());
    payload.push(u8::from(result.digest.is_some()));
    if let Some(job) = result.job {
        payload.push(job.provider_id);
        payload.push(job.provider_version);
    } else {
        payload.extend_from_slice(&[0; 2]);
    }
    payload.push(0);
    payload.extend_from_slice(&result.lease_id);
    payload.extend_from_slice(&result.worker_incarnation_id);
    payload.extend_from_slice(&result.job.map_or([0; 16], |job| job.job_id));
    payload.extend_from_slice(&result.digest.unwrap_or([0; 32]));
    Ok(payload)
}

pub fn build_compute_response_frame(
    response: &ComputeResponse,
    request_id: u64,
    sequence: u64,
) -> Result<Frame, PbmuxError> {
    if request_id == 0 {
        return Err(invalid());
    }
    let (message_type, payload) = match response {
        ComputeResponse::Status(status) => (2, encode_status(status, false)?),
        ComputeResponse::Result(result) => (3, encode_result(result)?),
        ComputeResponse::Cancel(status) => (4, encode_status(status, true)?),
    };
    Ok(Frame {
        header: Header {
            channel: Channel::Compute,
            flags: FLAG_START | FLAG_END,
            message_type,
            request_id,
            sequence,
            fragment_index: 0,
            payload_len: payload.len() as u32,
            logical_message_len: payload.len() as u32,
        },
        payload,
    })
}

fn decode_job_ref(
    payload: &[u8],
    present: u8,
    offset: usize,
) -> Result<Option<ComputeJobRef>, PbmuxError> {
    if present > 1 {
        return Err(invalid());
    }
    let job_id = payload[offset..offset + 16].try_into().expect("fixed job");
    let provider_id = payload[offset + 16];
    let provider_version = payload[offset + 17];
    if present == 0 {
        if job_id != [0; 16] || provider_id != 0 || provider_version != 0 {
            return Err(invalid());
        }
        Ok(None)
    } else {
        Ok(Some(ComputeJobRef {
            job_id,
            provider_id,
            provider_version,
        }))
    }
}

pub fn parse_compute_response_frame(frame: &Frame) -> Result<ComputeResponse, PbmuxError> {
    match frame.header.message_type {
        2 | 4 => {
            if !response_profile(frame, COMPUTE_STATUS_LEN) || frame.payload[54..56] != [0; 2] {
                return Err(invalid());
            }
            let status = ComputeStatus {
                state: ComputeJobState::try_from(frame.payload[0]).map_err(|()| invalid())?,
                reason: ComputeReason::try_from(read_u16(&frame.payload, 2))
                    .map_err(|()| invalid())?,
                lease_id: frame.payload[4..20].try_into().expect("fixed lease"),
                worker_incarnation_id: frame.payload[20..36].try_into().expect("fixed incarnation"),
                job: decode_job_ref(&frame.payload, frame.payload[1], 36)?,
            };
            let cancel = frame.header.message_type == 4;
            validate_status(&status, cancel)?;
            Ok(if cancel {
                ComputeResponse::Cancel(status)
            } else {
                ComputeResponse::Status(status)
            })
        }
        3 => {
            if !response_profile(frame, COMPUTE_RESULT_LEN)
                || frame.payload[7] != 0
                || frame.payload[4] > 1
            {
                return Err(invalid());
            }
            let job_id: [u8; 16] = frame.payload[40..56].try_into().expect("fixed job");
            let job = match frame.payload[1] {
                0 => {
                    if job_id != [0; 16] || frame.payload[5] != 0 || frame.payload[6] != 0 {
                        return Err(invalid());
                    }
                    None
                }
                1 => Some(ComputeJobRef {
                    job_id,
                    provider_id: frame.payload[5],
                    provider_version: frame.payload[6],
                }),
                _ => return Err(invalid()),
            };
            let digest_bytes: [u8; 32] = frame.payload[56..88].try_into().expect("fixed digest");
            let digest = if frame.payload[4] == 0 {
                if digest_bytes != [0; 32] {
                    return Err(invalid());
                }
                None
            } else {
                Some(digest_bytes)
            };
            let result = ComputeResult {
                state: ComputeJobState::try_from(frame.payload[0]).map_err(|()| invalid())?,
                reason: ComputeReason::try_from(read_u16(&frame.payload, 2))
                    .map_err(|()| invalid())?,
                lease_id: frame.payload[8..24].try_into().expect("fixed lease"),
                worker_incarnation_id: frame.payload[24..40].try_into().expect("fixed incarnation"),
                job,
                digest,
            };
            validate_result(&result)?;
            Ok(ComputeResponse::Result(result))
        }
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode, encode};

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/../../docs/protocol/c10_wire_v0_1_vectors_001/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn all_locked_vectors_obey_the_typed_direction_and_payload_profiles() {
        for name in [
            "GV-C10-01-submit-blake3-remote-buffer.bin",
            "GV-C10-02-status-request.bin",
            "GV-C10-06-cancel-request.bin",
            "GV-C10-08-unsupported-provider.bin",
            "GV-C10-14-request-id-conflict.bin",
            "GV-C10-15-duplicate-submit-replay.bin",
            "GV-C10-16-consumed-reservation-reuse.bin",
            "GV-C10-17-session-loss-non-resurrection.bin",
        ] {
            let bytes = fixture(name);
            let frame = decode(&bytes).unwrap();
            let request = parse_compute_request_frame(&frame).unwrap();
            assert_eq!(
                encode(
                    &build_compute_request_frame(
                        &request,
                        frame.header.request_id,
                        frame.header.sequence,
                    )
                    .unwrap()
                )
                .unwrap(),
                bytes,
                "{name}"
            );
        }
        for name in [
            "GV-C10-03-status-running-response.bin",
            "GV-C10-04-result-completed.bin",
            "GV-C10-05-result-failed.bin",
            "GV-C10-07-cancel-response.bin",
            "GV-C10-09-stale-lease.bin",
            "GV-C10-10-buffer-lost.bin",
        ] {
            let bytes = fixture(name);
            let frame = decode(&bytes).unwrap();
            let response = parse_compute_response_frame(&frame)
                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            assert_eq!(
                encode(
                    &build_compute_response_frame(
                        &response,
                        frame.header.request_id,
                        frame.header.sequence,
                    )
                    .unwrap()
                )
                .unwrap(),
                bytes,
                "{name}"
            );
        }
        assert!(
            parse_compute_response_frame(
                &decode(&fixture("GV-C10-11-invalid-digest-presence.bin")).unwrap()
            )
            .is_err()
        );
        assert!(
            parse_compute_request_frame(
                &decode(&fixture("GV-C10-12-malformed-reserved-zero.bin")).unwrap()
            )
            .is_err()
        );
        let wrong_direction = decode(&fixture("GV-C10-13-wrong-direction.bin")).unwrap();
        assert!(parse_compute_response_frame(&wrong_direction).is_err());
    }

    #[test]
    fn malformed_lengths_flags_ids_and_state_matrix_fail_closed() {
        let frame = decode(&fixture("GV-C10-01-submit-blake3-remote-buffer.bin")).unwrap();
        for mutate in [0_u8, 1, 2, 3] {
            let mut malformed = frame.clone();
            match mutate {
                0 => malformed.header.flags = FLAG_START | FLAG_END,
                1 => malformed.header.request_id = 0,
                2 => malformed.header.logical_message_len -= 1,
                _ => malformed.payload[0..16].fill(0),
            }
            assert!(parse_compute_request_frame(&malformed).is_err());
        }
        let mut malformed = decode(&fixture("GV-C10-04-result-completed.bin")).unwrap();
        malformed.payload[2..4]
            .copy_from_slice(&(ComputeReason::ProviderFailed as u16).to_be_bytes());
        assert!(parse_compute_response_frame(&malformed).is_err());
    }
}
