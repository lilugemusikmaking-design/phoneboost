use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use pb_pbmux::{
    AckPayload, BufferResult, CommandPayload, ComputeRequest, ComputeResponse, RemoteBufferRequest,
    ResourceRequest, ResourceResult,
};
use pb_types::PeerId;

const INITIATOR_QUEUE_CAPACITY: usize = 16;
const INITIATOR_REQUEST_TIMEOUT: Duration = Duration::from_secs(35);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitiatorClientError {
    NotAuthenticated,
    Busy,
    InvalidRequest,
    SessionLost,
    UnknownAfterDisconnect,
    ResponseMismatch,
    Timeout,
}

impl std::fmt::Display for InitiatorClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NotAuthenticated => "initiator session is not authenticated",
            Self::Busy => "initiator request queue is full",
            Self::InvalidRequest => "initiator request is invalid",
            Self::SessionLost => "initiator session was lost",
            Self::UnknownAfterDisconnect => "request outcome is unknown after disconnect",
            Self::ResponseMismatch => "authenticated response does not match the request",
            Self::Timeout => "authenticated request timed out",
        })
    }
}

impl std::error::Error for InitiatorClientError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InitiatorSessionSnapshot {
    pub authenticated: bool,
    pub peer_id: Option<PeerId>,
    pub generation: u64,
    pub liveness_age: Option<Duration>,
}

#[derive(Clone)]
pub struct InitiatorSessionClient {
    sender: mpsc::SyncSender<PendingInitiatorRequest>,
    shared: Arc<InitiatorShared>,
}

pub struct InitiatorSessionDriver {
    receiver: mpsc::Receiver<PendingInitiatorRequest>,
    shared: Arc<InitiatorShared>,
    generation: Option<u64>,
}

struct InitiatorShared {
    admission: Mutex<()>,
    state: Mutex<InitiatorState>,
    changed: Condvar,
    cancel: AtomicBool,
}

struct InitiatorState {
    authenticated: bool,
    peer_id: Option<PeerId>,
    generation: u64,
    last_liveness: Option<Instant>,
    driver_closed: bool,
}

pub(crate) enum InitiatorRequest {
    Command(CommandPayload),
    Resource(ResourceRequest),
    RemoteBuffer(RemoteBufferRequest),
    Compute(ComputeRequest),
}

pub(crate) enum InitiatorResponse {
    Command(AckPayload),
    Resource(ResourceResult),
    RemoteBuffer(BufferResult),
    Compute(ComputeResponse),
}

pub(crate) struct PendingInitiatorRequest {
    pub(crate) generation: u64,
    pub(crate) request_id: Option<u64>,
    pub(crate) request: InitiatorRequest,
    pub(crate) response: mpsc::SyncSender<Result<InitiatorResponse, InitiatorClientError>>,
}

pub fn initiator_session_channel() -> (InitiatorSessionClient, InitiatorSessionDriver) {
    let (sender, receiver) = mpsc::sync_channel(INITIATOR_QUEUE_CAPACITY);
    let shared = Arc::new(InitiatorShared {
        admission: Mutex::new(()),
        state: Mutex::new(InitiatorState {
            authenticated: false,
            peer_id: None,
            generation: 0,
            last_liveness: None,
            driver_closed: false,
        }),
        changed: Condvar::new(),
        cancel: AtomicBool::new(false),
    });
    (
        InitiatorSessionClient {
            sender,
            shared: Arc::clone(&shared),
        },
        InitiatorSessionDriver {
            receiver,
            shared,
            generation: None,
        },
    )
}

impl InitiatorSessionClient {
    pub fn snapshot(&self) -> InitiatorSessionSnapshot {
        let state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        InitiatorSessionSnapshot {
            authenticated: state.authenticated,
            peer_id: state.peer_id,
            generation: state.generation,
            liveness_age: state.last_liveness.map(|at| at.elapsed()),
        }
    }

    pub fn wait_authenticated(
        &self,
        timeout: Duration,
    ) -> Result<InitiatorSessionSnapshot, InitiatorClientError> {
        let deadline = Instant::now() + timeout;
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if state.authenticated {
                return Ok(InitiatorSessionSnapshot {
                    authenticated: true,
                    peer_id: state.peer_id,
                    generation: state.generation,
                    liveness_age: state.last_liveness.map(|at| at.elapsed()),
                });
            }
            if state.driver_closed {
                return Err(InitiatorClientError::SessionLost);
            }
            if self.shared.cancel.load(Ordering::Acquire) {
                return Err(InitiatorClientError::NotAuthenticated);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(InitiatorClientError::Timeout);
            }
            let remaining = deadline.saturating_duration_since(now);
            let waited = self
                .shared
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = waited.0;
        }
    }

    pub fn cancel_session(&self) {
        let _admission = self
            .shared
            .admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.shared.cancel.store(true, Ordering::Release);
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.authenticated = false;
        state.last_liveness = None;
        self.shared.changed.notify_all();
    }

    pub fn command(&self, request: CommandPayload) -> Result<AckPayload, InitiatorClientError> {
        match self.request(InitiatorRequest::Command(request))? {
            InitiatorResponse::Command(response) => Ok(response),
            _ => Err(InitiatorClientError::ResponseMismatch),
        }
    }

    pub fn resource(
        &self,
        request: ResourceRequest,
    ) -> Result<ResourceResult, InitiatorClientError> {
        self.resource_with_optional_request_id(None, request)
    }

    pub fn allocate_resource_request_id(&self) -> Result<u64, InitiatorClientError> {
        crate::runtime::random_nonzero_u64().map_err(|_| InitiatorClientError::InvalidRequest)
    }

    pub fn resource_with_request_id(
        &self,
        request_id: u64,
        request: ResourceRequest,
    ) -> Result<ResourceResult, InitiatorClientError> {
        if request_id == 0 {
            return Err(InitiatorClientError::InvalidRequest);
        }
        self.resource_with_optional_request_id(Some(request_id), request)
    }

    fn resource_with_optional_request_id(
        &self,
        request_id: Option<u64>,
        request: ResourceRequest,
    ) -> Result<ResourceResult, InitiatorClientError> {
        match self.request_with_id(request_id, InitiatorRequest::Resource(request))? {
            InitiatorResponse::Resource(response) => Ok(response),
            _ => Err(InitiatorClientError::ResponseMismatch),
        }
    }

    pub fn remote_buffer(
        &self,
        request: RemoteBufferRequest,
    ) -> Result<BufferResult, InitiatorClientError> {
        match self.request(InitiatorRequest::RemoteBuffer(request))? {
            InitiatorResponse::RemoteBuffer(response) => Ok(response),
            _ => Err(InitiatorClientError::ResponseMismatch),
        }
    }

    pub fn compute(
        &self,
        request: ComputeRequest,
    ) -> Result<ComputeResponse, InitiatorClientError> {
        match self.request(InitiatorRequest::Compute(request))? {
            InitiatorResponse::Compute(response) => Ok(response),
            _ => Err(InitiatorClientError::ResponseMismatch),
        }
    }

    fn request(
        &self,
        request: InitiatorRequest,
    ) -> Result<InitiatorResponse, InitiatorClientError> {
        self.request_with_id(None, request)
    }

    fn request_with_id(
        &self,
        request_id: Option<u64>,
        request: InitiatorRequest,
    ) -> Result<InitiatorResponse, InitiatorClientError> {
        let admission = self
            .shared
            .admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let generation = {
            let state = self
                .shared
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !state.authenticated || self.shared.cancel.load(Ordering::Acquire) {
                return Err(InitiatorClientError::NotAuthenticated);
            }
            state.generation
        };
        let (response, receiver) = mpsc::sync_channel(1);
        let pending = PendingInitiatorRequest {
            generation,
            request_id,
            request,
            response,
        };
        self.sender.try_send(pending).map_err(|error| match error {
            mpsc::TrySendError::Full(_) => InitiatorClientError::Busy,
            mpsc::TrySendError::Disconnected(_) => InitiatorClientError::SessionLost,
        })?;
        drop(admission);
        receiver
            .recv_timeout(INITIATOR_REQUEST_TIMEOUT)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => InitiatorClientError::Timeout,
                mpsc::RecvTimeoutError::Disconnected => InitiatorClientError::SessionLost,
            })?
    }
}

impl InitiatorSessionDriver {
    pub(crate) fn begin_session(&mut self, peer_id: PeerId) -> u64 {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.generation = state.generation.saturating_add(1).max(1);
        state.peer_id = Some(peer_id);
        state.authenticated = !self.shared.cancel.load(Ordering::Acquire);
        state.last_liveness = Some(Instant::now());
        state.driver_closed = false;
        self.generation = Some(state.generation);
        self.shared.changed.notify_all();
        state.generation
    }

    pub(crate) fn mark_liveness(&self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.authenticated {
            state.last_liveness = Some(Instant::now());
            self.shared.changed.notify_all();
        }
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.shared.cancel.load(Ordering::Acquire)
    }

    pub(crate) fn next_request(&mut self, generation: u64) -> Option<PendingInitiatorRequest> {
        loop {
            let pending = match self.receiver.try_recv() {
                Ok(pending) => pending,
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => return None,
            };
            if pending.generation == generation {
                return Some(pending);
            }
            let _ = pending
                .response
                .send(Err(InitiatorClientError::SessionLost));
        }
    }

    pub(crate) fn end_session(&mut self) {
        let generation = self.generation.take();
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if generation == Some(state.generation) {
            state.authenticated = false;
            state.last_liveness = None;
        }
        state.driver_closed = true;
        self.shared.changed.notify_all();
        drop(state);
        while let Ok(pending) = self.receiver.try_recv() {
            let _ = pending
                .response
                .send(Err(InitiatorClientError::SessionLost));
        }
    }
}

impl Drop for InitiatorSessionDriver {
    fn drop(&mut self) {
        self.end_session();
    }
}
