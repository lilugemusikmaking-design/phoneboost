//! Locked C12 v1 private import storage and lifecycle authority.

use std::collections::{HashMap, VecDeque};
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, RawDir, SealFlags, fchmod, fcntl_get_seals, fstat, fsync,
    mkdirat, open, openat, renameat, statat, unlinkat,
};
use rustix::io::Errno;
use rustix::process::getuid;

pub const MAXIMUM_IMPORT_BYTES: u64 = 128 * 1024 * 1024;
pub const MAXIMUM_IMPORTS_PER_REQUESTER: usize = 4;
pub const MAXIMUM_IMPORTS_PER_UID: usize = 8;
pub const MAXIMUM_AGGREGATE_BYTES_PER_UID: u64 = 256 * 1024 * 1024;
pub const MAXIMUM_RECEIVING_PER_REQUESTER: usize = 1;
pub const MAXIMUM_RECEIVING_PER_UID: usize = 2;
pub const IMPORT_COPY_CHUNK_BYTES: usize = 65_536;
pub const IMPORT_RECEIVING_TIMEOUT: Duration = Duration::from_secs(60);
pub const IMPORT_READY_TTL: Duration = Duration::from_secs(300);
pub const IMPORT_CANCELLATION_DEADLINE: Duration = Duration::from_secs(5);
pub const IMPORT_TOMBSTONE_TTL: Duration = Duration::from_secs(300);
pub const MAXIMUM_TOMBSTONES_PER_UID: usize = 256;

const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const PHONEBOOST_DIRECTORY: &str = "phoneboost";
const IMPORT_DIRECTORY: &str = "import";
const VERSION_DIRECTORY: &str = "v1";

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct ImportRequester {
    peer_uid: u32,
    connection_id: u64,
}

impl fmt::Debug for ImportRequester {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ImportRequester([REDACTED])")
    }
}

impl ImportRequester {
    pub const fn new(peer_uid: u32, connection_id: u64) -> Self {
        Self {
            peer_uid,
            connection_id,
        }
    }

    pub const fn peer_uid(self) -> u32 {
        self.peer_uid
    }
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub struct ImportHandle(String);

impl fmt::Debug for ImportHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ImportHandle([REDACTED])")
    }
}

impl ImportHandle {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn parse_exact(value: &str) -> Option<Self> {
        (value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        .then(|| Self(value.to_owned()))
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ReadyImport {
    handle: ImportHandle,
    bytes: u64,
}

impl fmt::Debug for ReadyImport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReadyImport")
            .field("state", &"READY")
            .field("size_class", &import_size_class(self.bytes))
            .finish()
    }
}

impl ReadyImport {
    pub fn handle(&self) -> &ImportHandle {
        &self.handle
    }

    pub const fn bytes(&self) -> u64 {
        self.bytes
    }
}

pub struct ConsumedImport {
    descriptor: OwnedFd,
    bytes: u64,
}

impl fmt::Debug for ConsumedImport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConsumedImport")
            .field("state", &"CONSUMED")
            .field("size_class", &import_size_class(self.bytes))
            .finish_non_exhaustive()
    }
}

impl ConsumedImport {
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn into_descriptor(self) -> OwnedFd {
        self.descriptor
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportTerminalState {
    Cancelled,
    Consumed,
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportStatus {
    Receiving,
    Ready { bytes: u64 },
    Terminal(ImportTerminalState),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImportCancellation {
    state: ImportTerminalState,
    state_changed: bool,
}

impl ImportCancellation {
    pub const fn state(self) -> ImportTerminalState {
        self.state
    }

    pub const fn state_changed(self) -> bool {
        self.state_changed
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportStoreError {
    CacheRootUnavailable,
    UnsafeCacheRoot,
    UnsafeDirectory,
    UnsafeSource,
    UnsealedMemfd,
    RequesterObjectLimit,
    UidObjectLimit,
    RequesterReceiverLimit,
    UidReceiverLimit,
    ObjectTooLarge,
    UidByteLimit,
    ReceivingTimedOut,
    EntropyUnavailable,
    NotFound,
    NotReady,
    CleanupUnconfirmed,
    Io,
}

impl fmt::Display for ImportStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CacheRootUnavailable => "import cache root unavailable",
            Self::UnsafeCacheRoot => "import cache root is unsafe",
            Self::UnsafeDirectory => "import directory is unsafe",
            Self::UnsafeSource => "import source is unsafe",
            Self::UnsealedMemfd => "import memfd is not fully sealed",
            Self::RequesterObjectLimit => "requester import count limit reached",
            Self::UidObjectLimit => "UID import count limit reached",
            Self::RequesterReceiverLimit => "requester receiver limit reached",
            Self::UidReceiverLimit => "UID receiver limit reached",
            Self::ObjectTooLarge => "import object exceeds its byte limit",
            Self::UidByteLimit => "UID import byte limit reached",
            Self::ReceivingTimedOut => "import receiving deadline reached",
            Self::EntropyUnavailable => "import handle entropy unavailable",
            Self::NotFound => "import not found",
            Self::NotReady => "import is not ready",
            Self::CleanupUnconfirmed => "import cleanup could not be confirmed",
            Self::Io => "import I/O failed",
        })
    }
}

impl std::error::Error for ImportStoreError {}

#[derive(Clone, Copy)]
struct ImportPolicy {
    maximum_object_bytes: u64,
    maximum_objects_per_requester: usize,
    maximum_objects_per_uid: usize,
    maximum_aggregate_bytes_per_uid: u64,
    maximum_receiving_per_requester: usize,
    maximum_receiving_per_uid: usize,
    receiving_timeout: Duration,
    ready_ttl: Duration,
    tombstone_ttl: Duration,
    maximum_tombstones_per_uid: usize,
    cancellation_deadline: Duration,
    receiver_start_delay: Duration,
    receiver_chunk_pause: Duration,
}

impl ImportPolicy {
    const LOCKED: Self = Self {
        maximum_object_bytes: MAXIMUM_IMPORT_BYTES,
        maximum_objects_per_requester: MAXIMUM_IMPORTS_PER_REQUESTER,
        maximum_objects_per_uid: MAXIMUM_IMPORTS_PER_UID,
        maximum_aggregate_bytes_per_uid: MAXIMUM_AGGREGATE_BYTES_PER_UID,
        maximum_receiving_per_requester: MAXIMUM_RECEIVING_PER_REQUESTER,
        maximum_receiving_per_uid: MAXIMUM_RECEIVING_PER_UID,
        receiving_timeout: IMPORT_RECEIVING_TIMEOUT,
        ready_ttl: IMPORT_READY_TTL,
        tombstone_ttl: IMPORT_TOMBSTONE_TTL,
        maximum_tombstones_per_uid: MAXIMUM_TOMBSTONES_PER_UID,
        cancellation_deadline: IMPORT_CANCELLATION_DEADLINE,
        receiver_start_delay: Duration::ZERO,
        receiver_chunk_pause: Duration::ZERO,
    };
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ImportPhase {
    Receiving,
    Ready { since: Instant },
}

enum AsyncReceiveError {
    Cancelled,
    Store(ImportStoreError),
}

impl From<ImportStoreError> for AsyncReceiveError {
    fn from(error: ImportStoreError) -> Self {
        Self::Store(error)
    }
}

struct LiveImport {
    requester: ImportRequester,
    bytes: u64,
    phase: ImportPhase,
    cancel_requested: bool,
    worker_running: bool,
    failure: Option<ImportStoreError>,
}

#[derive(Clone, Copy)]
enum TombstoneOutcome {
    Terminal(ImportTerminalState),
    Failed(ImportStoreError),
}

struct Tombstone {
    handle: String,
    requester: ImportRequester,
    outcome: TombstoneOutcome,
    created: Instant,
}

#[derive(Default)]
struct ImportState {
    live: HashMap<String, LiveImport>,
    tombstones: VecDeque<Tombstone>,
}

pub struct ImportStore {
    directory: OwnedFd,
    state: Mutex<ImportState>,
    worker_changed: Condvar,
    policy: ImportPolicy,
}

impl fmt::Debug for ImportStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImportStore")
            .finish_non_exhaustive()
    }
}

impl ImportStore {
    pub fn from_environment() -> Result<Self, ImportStoreError> {
        let cache_home =
            cache_home_from_values(env::var_os("XDG_CACHE_HOME"), env::var_os("HOME"))?;
        Self::from_cache_home(&cache_home)
    }

    pub fn from_cache_home(cache_home: &Path) -> Result<Self, ImportStoreError> {
        Self::from_cache_home_with_policy(cache_home, ImportPolicy::LOCKED)
    }

    fn from_cache_home_with_policy(
        cache_home: &Path,
        policy: ImportPolicy,
    ) -> Result<Self, ImportStoreError> {
        if !cache_home.is_absolute() {
            return Err(ImportStoreError::UnsafeCacheRoot);
        }
        fs::create_dir_all(cache_home).map_err(|_| ImportStoreError::CacheRootUnavailable)?;
        reject_symlink_components(cache_home)?;
        let cache_root = open(
            cache_home,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| ImportStoreError::UnsafeCacheRoot)?;
        validate_owned_directory(&cache_root, None)
            .map_err(|_| ImportStoreError::UnsafeCacheRoot)?;
        let phoneboost = ensure_private_directory(&cache_root, PHONEBOOST_DIRECTORY)?;
        let import = ensure_private_directory(&phoneboost, IMPORT_DIRECTORY)?;
        let directory = ensure_private_directory(&import, VERSION_DIRECTORY)?;
        clear_version_directory(&directory)?;
        Ok(Self {
            directory,
            state: Mutex::new(ImportState::default()),
            worker_changed: Condvar::new(),
            policy,
        })
    }

    /// Validate and schedule a bounded receiver, returning only after the
    /// receiver thread owns both descriptors.
    pub fn start_import(
        self: &Arc<Self>,
        requester: ImportRequester,
        source: OwnedFd,
    ) -> Result<ImportHandle, ImportStoreError> {
        validate_source(&source, requester.peer_uid)?;
        let mut source = File::from(source);
        source
            .seek(SeekFrom::Start(0))
            .map_err(|_| ImportStoreError::UnsafeSource)?;
        let handle = self.reserve_import_with_worker(requester, true)?;
        let receiving_name = receiving_name(&handle);
        let destination = match openat(
            &self.directory,
            receiving_name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(descriptor) => descriptor,
            Err(_) => return self.fail_import(&handle, ImportStoreError::Io),
        };
        if validate_private_regular_file(&destination).is_err() {
            return self.fail_import(&handle, ImportStoreError::UnsafeDirectory);
        }
        let destination = File::from(destination);
        let worker_store = Arc::clone(self);
        let worker_handle = handle.clone();
        if thread::Builder::new()
            .name("phoneboost-import-receiver".to_owned())
            .spawn(move || {
                worker_store.receive_async(worker_handle, requester, source, destination)
            })
            .is_err()
        {
            self.mark_worker_stopped(&handle, requester, Some(ImportStoreError::Io));
            return self.fail_import(&handle, ImportStoreError::Io);
        }
        Ok(ImportHandle(handle))
    }

    pub fn status(
        &self,
        requester: ImportRequester,
        handle: &ImportHandle,
    ) -> Result<ImportStatus, ImportStoreError> {
        self.expire_ready_at(Instant::now())?;
        let mut state = self.state.lock().map_err(|_| ImportStoreError::Io)?;
        prune_tombstones(&mut state, Instant::now(), self.policy.tombstone_ttl);
        if let Some(tombstone) = state
            .tombstones
            .iter()
            .find(|item| item.handle == handle.0 && item.requester == requester)
        {
            return match tombstone.outcome {
                TombstoneOutcome::Terminal(terminal) => Ok(ImportStatus::Terminal(terminal)),
                TombstoneOutcome::Failed(error) => Err(error),
            };
        }
        let Some(entry) = state.live.get(&handle.0) else {
            return Err(ImportStoreError::NotFound);
        };
        if entry.requester != requester {
            return Err(ImportStoreError::NotFound);
        }
        if let Some(error) = entry.failure {
            return Err(error);
        }
        Ok(match entry.phase {
            ImportPhase::Receiving => ImportStatus::Receiving,
            ImportPhase::Ready { .. } => ImportStatus::Ready { bytes: entry.bytes },
        })
    }

    fn receive_async(
        &self,
        handle: String,
        requester: ImportRequester,
        mut source: File,
        mut destination: File,
    ) {
        if !self.policy.receiver_start_delay.is_zero() {
            thread::sleep(self.policy.receiver_start_delay);
        }
        let outcome = self.copy_async(&handle, requester, &mut source, &mut destination);
        drop(source);
        drop(destination);
        match outcome {
            Ok(()) => self.finish_async_ready(&handle, requester),
            Err(AsyncReceiveError::Cancelled) => self.finish_async_cancelled(&handle, requester),
            Err(AsyncReceiveError::Store(error)) => {
                self.finish_async_failed(&handle, requester, error)
            }
        }
    }

    fn copy_async(
        &self,
        handle: &str,
        requester: ImportRequester,
        source: &mut File,
        destination: &mut File,
    ) -> Result<(), AsyncReceiveError> {
        let started = Instant::now();
        let mut chunk = [0_u8; IMPORT_COPY_CHUNK_BYTES];
        loop {
            if self.receiver_cancelled(handle, requester)? {
                return Err(AsyncReceiveError::Cancelled);
            }
            if started.elapsed() >= self.policy.receiving_timeout {
                return Err(ImportStoreError::ReceivingTimedOut.into());
            }
            let read = source.read(&mut chunk).map_err(|_| ImportStoreError::Io)?;
            if !self.policy.receiver_chunk_pause.is_zero() {
                thread::sleep(self.policy.receiver_chunk_pause);
            }
            if started.elapsed() >= self.policy.receiving_timeout {
                return Err(ImportStoreError::ReceivingTimedOut.into());
            }
            if self.receiver_cancelled(handle, requester)? {
                return Err(AsyncReceiveError::Cancelled);
            }
            if read == 0 {
                break;
            }
            if let Err(error) = self.reserve_bytes(handle, requester, read as u64) {
                if self.receiver_cancelled(handle, requester).unwrap_or(false) {
                    return Err(AsyncReceiveError::Cancelled);
                }
                return Err(error.into());
            }
            destination
                .write_all(&chunk[..read])
                .map_err(|_| ImportStoreError::Io)?;
        }
        destination.flush().map_err(|_| ImportStoreError::Io)?;
        fsync(&*destination).map_err(|_| ImportStoreError::Io)?;
        if self.receiver_cancelled(handle, requester)? {
            return Err(AsyncReceiveError::Cancelled);
        }
        Ok(())
    }

    fn receiver_cancelled(
        &self,
        handle: &str,
        requester: ImportRequester,
    ) -> Result<bool, ImportStoreError> {
        let state = self.state.lock().map_err(|_| ImportStoreError::Io)?;
        let Some(entry) = state.live.get(handle) else {
            return Err(ImportStoreError::NotFound);
        };
        if entry.requester != requester || entry.phase != ImportPhase::Receiving {
            return Err(ImportStoreError::NotFound);
        }
        Ok(entry.cancel_requested)
    }

    fn finish_async_ready(&self, handle: &str, requester: ImportRequester) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        let Some(entry) = state.live.get(handle) else {
            return;
        };
        if entry.requester != requester || entry.phase != ImportPhase::Receiving {
            return;
        }
        if entry.cancel_requested {
            drop(state);
            self.finish_async_cancelled(handle, requester);
            return;
        }
        if renameat(
            &self.directory,
            receiving_name(handle).as_str(),
            &self.directory,
            ready_name(handle).as_str(),
        )
        .is_err()
        {
            drop(state);
            self.finish_async_failed(handle, requester, ImportStoreError::Io);
            return;
        }
        if let Some(entry) = state.live.get_mut(handle) {
            entry.phase = ImportPhase::Ready {
                since: Instant::now(),
            };
            entry.worker_running = false;
        }
        self.worker_changed.notify_all();
    }

    fn finish_async_cancelled(&self, handle: &str, requester: ImportRequester) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        let Some(entry) = state.live.get(handle) else {
            self.worker_changed.notify_all();
            return;
        };
        if entry.requester != requester {
            return;
        }
        if cleanup_live_path(&self.directory, handle, entry.phase).is_err() {
            if let Some(entry) = state.live.get_mut(handle) {
                entry.worker_running = false;
                entry.failure = Some(ImportStoreError::CleanupUnconfirmed);
            }
            self.worker_changed.notify_all();
            return;
        }
        state.live.remove(handle);
        push_tombstone(
            &mut state,
            handle.to_owned(),
            requester,
            ImportTerminalState::Cancelled,
            Instant::now(),
            self.policy,
        );
        self.worker_changed.notify_all();
    }

    fn finish_async_failed(
        &self,
        handle: &str,
        requester: ImportRequester,
        error: ImportStoreError,
    ) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        let Some(entry) = state.live.get(handle) else {
            return;
        };
        if entry.requester != requester {
            return;
        }
        if cleanup_live_path(&self.directory, handle, entry.phase).is_err() {
            if let Some(entry) = state.live.get_mut(handle) {
                entry.worker_running = false;
                entry.failure = Some(ImportStoreError::CleanupUnconfirmed);
            }
            self.worker_changed.notify_all();
            return;
        }
        state.live.remove(handle);
        push_failure_tombstone(
            &mut state,
            handle.to_owned(),
            requester,
            error,
            Instant::now(),
            self.policy,
        );
        self.worker_changed.notify_all();
    }

    fn mark_worker_stopped(
        &self,
        handle: &str,
        requester: ImportRequester,
        failure: Option<ImportStoreError>,
    ) {
        if let Ok(mut state) = self.state.lock()
            && let Some(entry) = state.live.get_mut(handle)
            && entry.requester == requester
        {
            entry.worker_running = false;
            entry.failure = failure;
            self.worker_changed.notify_all();
        }
    }

    pub fn import_descriptor(
        &self,
        requester: ImportRequester,
        source: OwnedFd,
    ) -> Result<ReadyImport, ImportStoreError> {
        validate_source(&source, requester.peer_uid)?;
        let mut source = File::from(source);
        source
            .seek(SeekFrom::Start(0))
            .map_err(|_| ImportStoreError::UnsafeSource)?;
        let handle = self.reserve_import_with_worker(requester, false)?;
        let receiving_name = receiving_name(&handle);
        let ready_name = ready_name(&handle);
        let destination = match openat(
            &self.directory,
            receiving_name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(descriptor) => descriptor,
            Err(_) => return self.fail_import(&handle, ImportStoreError::Io),
        };
        if validate_private_regular_file(&destination).is_err() {
            return self.fail_import(&handle, ImportStoreError::UnsafeDirectory);
        }
        let mut destination = File::from(destination);
        let started = Instant::now();
        let mut chunk = [0_u8; IMPORT_COPY_CHUNK_BYTES];
        loop {
            if started.elapsed() >= self.policy.receiving_timeout {
                return self.fail_import(&handle, ImportStoreError::ReceivingTimedOut);
            }
            if !self.is_receiving(&handle, requester)? {
                return Err(ImportStoreError::NotFound);
            }
            let read = source
                .read(&mut chunk)
                .map_err(|_| ImportStoreError::Io)
                .and_then(|read| {
                    if started.elapsed() >= self.policy.receiving_timeout {
                        Err(ImportStoreError::ReceivingTimedOut)
                    } else {
                        Ok(read)
                    }
                });
            let read = match read {
                Ok(read) => read,
                Err(error) => return self.fail_import(&handle, error),
            };
            if read == 0 {
                break;
            }
            if let Err(error) = self.reserve_bytes(&handle, requester, read as u64) {
                return self.fail_import(&handle, error);
            }
            if destination.write_all(&chunk[..read]).is_err() {
                return self.fail_import(&handle, ImportStoreError::Io);
            }
        }
        if destination.flush().is_err() || fsync(&destination).is_err() {
            return self.fail_import(&handle, ImportStoreError::Io);
        }

        let mut state = self.state.lock().map_err(|_| ImportStoreError::Io)?;
        let Some(entry) = state.live.get_mut(&handle) else {
            return Err(ImportStoreError::NotFound);
        };
        if entry.requester != requester || entry.phase != ImportPhase::Receiving {
            return Err(ImportStoreError::NotFound);
        }
        if renameat(
            &self.directory,
            receiving_name.as_str(),
            &self.directory,
            ready_name.as_str(),
        )
        .is_err()
        {
            drop(state);
            return self.fail_import(&handle, ImportStoreError::Io);
        }
        entry.phase = ImportPhase::Ready {
            since: Instant::now(),
        };
        let bytes = entry.bytes;
        Ok(ReadyImport {
            handle: ImportHandle(handle),
            bytes,
        })
    }

    pub fn cancel(
        &self,
        requester: ImportRequester,
        handle: &ImportHandle,
    ) -> Result<ImportCancellation, ImportStoreError> {
        let started = Instant::now();
        let mut requested_cancellation = false;
        let mut state = self.state.lock().map_err(|_| ImportStoreError::Io)?;
        loop {
            let now = Instant::now();
            prune_tombstones(&mut state, now, self.policy.tombstone_ttl);
            if let Some(tombstone) = state
                .tombstones
                .iter()
                .find(|item| item.handle == handle.0 && item.requester == requester)
            {
                return match tombstone.outcome {
                    TombstoneOutcome::Terminal(terminal) => Ok(ImportCancellation {
                        state: terminal,
                        state_changed: requested_cancellation
                            && terminal == ImportTerminalState::Cancelled,
                    }),
                    TombstoneOutcome::Failed(error) => Err(error),
                };
            }
            let Some(entry) = state.live.get_mut(&handle.0) else {
                return Err(ImportStoreError::NotFound);
            };
            if entry.requester != requester {
                return Err(ImportStoreError::NotFound);
            }
            if entry.phase == ImportPhase::Receiving && entry.worker_running {
                entry.cancel_requested = true;
                requested_cancellation = true;
                let elapsed = started.elapsed();
                let Some(remaining) = self.policy.cancellation_deadline.checked_sub(elapsed) else {
                    return Err(ImportStoreError::CleanupUnconfirmed);
                };
                let (next, wait) = self
                    .worker_changed
                    .wait_timeout(state, remaining)
                    .map_err(|_| ImportStoreError::Io)?;
                state = next;
                if wait.timed_out() {
                    return Err(ImportStoreError::CleanupUnconfirmed);
                }
                continue;
            }
            let phase = entry.phase;
            cleanup_live_path(&self.directory, &handle.0, phase)?;
            state.live.remove(&handle.0);
            push_tombstone(
                &mut state,
                handle.0.clone(),
                requester,
                ImportTerminalState::Cancelled,
                now,
                self.policy,
            );
            return Ok(ImportCancellation {
                state: ImportTerminalState::Cancelled,
                state_changed: true,
            });
        }
    }

    pub fn consume(
        &self,
        requester: ImportRequester,
        handle: &ImportHandle,
    ) -> Result<ConsumedImport, ImportStoreError> {
        self.expire_ready_at(Instant::now())?;
        let mut state = self.state.lock().map_err(|_| ImportStoreError::Io)?;
        let Some(entry) = state.live.get(&handle.0) else {
            return Err(ImportStoreError::NotFound);
        };
        if entry.requester != requester {
            return Err(ImportStoreError::NotFound);
        }
        if !matches!(entry.phase, ImportPhase::Ready { .. }) {
            return Err(ImportStoreError::NotReady);
        }
        let descriptor = openat(
            &self.directory,
            ready_name(&handle.0).as_str(),
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| ImportStoreError::CleanupUnconfirmed)?;
        validate_private_regular_file(&descriptor)
            .map_err(|_| ImportStoreError::CleanupUnconfirmed)?;
        unlink_confirmed(&self.directory, ready_name(&handle.0).as_str())?;
        let bytes = entry.bytes;
        state.live.remove(&handle.0);
        push_tombstone(
            &mut state,
            handle.0.clone(),
            requester,
            ImportTerminalState::Consumed,
            Instant::now(),
            self.policy,
        );
        Ok(ConsumedImport { descriptor, bytes })
    }

    pub fn disconnect(&self, requester: ImportRequester) -> Result<(), ImportStoreError> {
        let handles = {
            let state = self.state.lock().map_err(|_| ImportStoreError::Io)?;
            state
                .live
                .iter()
                .filter(|(_, entry)| entry.requester == requester)
                .map(|(handle, _)| ImportHandle(handle.clone()))
                .collect::<Vec<_>>()
        };
        let mut failure = None;
        for handle in handles {
            if let Err(error) = self.cancel(requester, &handle) {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub fn expire_ready(&self) -> Result<usize, ImportStoreError> {
        self.expire_ready_at(Instant::now())
    }

    fn expire_ready_at(&self, now: Instant) -> Result<usize, ImportStoreError> {
        let mut state = self.state.lock().map_err(|_| ImportStoreError::Io)?;
        prune_tombstones(&mut state, now, self.policy.tombstone_ttl);
        let expired = state
            .live
            .iter()
            .filter_map(|(handle, entry)| match entry.phase {
                ImportPhase::Ready { since }
                    if now.saturating_duration_since(since) >= self.policy.ready_ttl =>
                {
                    Some((handle.clone(), entry.requester))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for (handle, requester) in &expired {
            unlink_confirmed(&self.directory, ready_name(handle).as_str())?;
            state.live.remove(handle);
            push_tombstone(
                &mut state,
                handle.clone(),
                *requester,
                ImportTerminalState::Expired,
                now,
                self.policy,
            );
        }
        Ok(expired.len())
    }

    #[cfg(test)]
    fn reserve_import(&self, requester: ImportRequester) -> Result<String, ImportStoreError> {
        self.reserve_import_with_worker(requester, false)
    }

    fn reserve_import_with_worker(
        &self,
        requester: ImportRequester,
        worker_running: bool,
    ) -> Result<String, ImportStoreError> {
        let mut state = self.state.lock().map_err(|_| ImportStoreError::Io)?;
        prune_tombstones(&mut state, Instant::now(), self.policy.tombstone_ttl);
        let requester_live = state
            .live
            .values()
            .filter(|entry| entry.requester == requester)
            .count();
        let uid_live = state
            .live
            .values()
            .filter(|entry| entry.requester.peer_uid == requester.peer_uid)
            .count();
        let requester_receiving = state
            .live
            .values()
            .filter(|entry| entry.requester == requester && entry.phase == ImportPhase::Receiving)
            .count();
        let uid_receiving = state
            .live
            .values()
            .filter(|entry| {
                entry.requester.peer_uid == requester.peer_uid
                    && entry.phase == ImportPhase::Receiving
            })
            .count();
        if requester_live >= self.policy.maximum_objects_per_requester {
            return Err(ImportStoreError::RequesterObjectLimit);
        }
        if uid_live >= self.policy.maximum_objects_per_uid {
            return Err(ImportStoreError::UidObjectLimit);
        }
        if requester_receiving >= self.policy.maximum_receiving_per_requester {
            return Err(ImportStoreError::RequesterReceiverLimit);
        }
        if uid_receiving >= self.policy.maximum_receiving_per_uid {
            return Err(ImportStoreError::UidReceiverLimit);
        }
        let handle = random_handle(&state)?;
        state.live.insert(
            handle.clone(),
            LiveImport {
                requester,
                bytes: 0,
                phase: ImportPhase::Receiving,
                cancel_requested: false,
                worker_running,
                failure: None,
            },
        );
        Ok(handle)
    }

    fn reserve_bytes(
        &self,
        handle: &str,
        requester: ImportRequester,
        bytes: u64,
    ) -> Result<(), ImportStoreError> {
        let mut state = self.state.lock().map_err(|_| ImportStoreError::Io)?;
        let uid_bytes = state
            .live
            .values()
            .filter(|entry| entry.requester.peer_uid == requester.peer_uid)
            .map(|entry| entry.bytes)
            .sum::<u64>();
        let Some(entry) = state.live.get_mut(handle) else {
            return Err(ImportStoreError::NotFound);
        };
        if entry.requester != requester || entry.phase != ImportPhase::Receiving {
            return Err(ImportStoreError::NotFound);
        }
        if entry.cancel_requested {
            return Err(ImportStoreError::NotFound);
        }
        let object_total = entry
            .bytes
            .checked_add(bytes)
            .ok_or(ImportStoreError::ObjectTooLarge)?;
        if object_total > self.policy.maximum_object_bytes {
            return Err(ImportStoreError::ObjectTooLarge);
        }
        let uid_total = uid_bytes
            .checked_add(bytes)
            .ok_or(ImportStoreError::UidByteLimit)?;
        if uid_total > self.policy.maximum_aggregate_bytes_per_uid {
            return Err(ImportStoreError::UidByteLimit);
        }
        entry.bytes = object_total;
        Ok(())
    }

    fn is_receiving(
        &self,
        handle: &str,
        requester: ImportRequester,
    ) -> Result<bool, ImportStoreError> {
        let state = self.state.lock().map_err(|_| ImportStoreError::Io)?;
        Ok(state.live.get(handle).is_some_and(|entry| {
            entry.requester == requester && entry.phase == ImportPhase::Receiving
        }))
    }

    fn fail_import<T>(
        &self,
        handle: &str,
        original: ImportStoreError,
    ) -> Result<T, ImportStoreError> {
        let mut state = self.state.lock().map_err(|_| ImportStoreError::Io)?;
        let Some(entry) = state.live.get(handle) else {
            return Err(original);
        };
        if cleanup_live_path(&self.directory, handle, entry.phase).is_err() {
            return Err(ImportStoreError::CleanupUnconfirmed);
        }
        state.live.remove(handle);
        Err(original)
    }

    #[cfg(test)]
    fn live_count(&self) -> usize {
        self.state.lock().unwrap().live.len()
    }
}

fn validate_source<Fd: AsFd>(source: Fd, peer_uid: u32) -> Result<(), ImportStoreError> {
    let stat = fstat(&source).map_err(|_| ImportStoreError::UnsafeSource)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile || stat.st_uid != peer_uid {
        return Err(ImportStoreError::UnsafeSource);
    }
    if stat.st_nlink == 0 {
        match fcntl_get_seals(&source) {
            Ok(seals) => {
                let required =
                    SealFlags::SEAL | SealFlags::SHRINK | SealFlags::GROW | SealFlags::WRITE;
                if !seals.contains(required) {
                    return Err(ImportStoreError::UnsealedMemfd);
                }
            }
            Err(_) => return Err(ImportStoreError::UnsafeSource),
        }
    }
    Ok(())
}

fn random_handle(state: &ImportState) -> Result<String, ImportStoreError> {
    for _ in 0..8 {
        let mut bytes = [0_u8; 32];
        File::open("/dev/urandom")
            .and_then(|mut random| random.read_exact(&mut bytes))
            .map_err(|_| ImportStoreError::EntropyUnavailable)?;
        let handle = lowercase_hex(&bytes);
        if !state.live.contains_key(&handle)
            && !state.tombstones.iter().any(|item| item.handle == handle)
        {
            return Ok(handle);
        }
    }
    Err(ImportStoreError::EntropyUnavailable)
}

fn cleanup_live_path<Fd: AsFd>(
    directory: Fd,
    handle: &str,
    phase: ImportPhase,
) -> Result<(), ImportStoreError> {
    let name = match phase {
        ImportPhase::Receiving => receiving_name(handle),
        ImportPhase::Ready { .. } => ready_name(handle),
    };
    unlink_confirmed(directory, name.as_str())
}

fn unlink_confirmed<Fd: AsFd>(directory: Fd, name: &str) -> Result<(), ImportStoreError> {
    match unlinkat(directory, name, AtFlags::empty()) {
        Ok(()) | Err(Errno::NOENT) => Ok(()),
        Err(_) => Err(ImportStoreError::CleanupUnconfirmed),
    }
}

fn push_tombstone(
    state: &mut ImportState,
    handle: String,
    requester: ImportRequester,
    terminal: ImportTerminalState,
    now: Instant,
    policy: ImportPolicy,
) {
    push_tombstone_outcome(
        state,
        handle,
        requester,
        TombstoneOutcome::Terminal(terminal),
        now,
        policy,
    );
}

fn push_failure_tombstone(
    state: &mut ImportState,
    handle: String,
    requester: ImportRequester,
    error: ImportStoreError,
    now: Instant,
    policy: ImportPolicy,
) {
    push_tombstone_outcome(
        state,
        handle,
        requester,
        TombstoneOutcome::Failed(error),
        now,
        policy,
    );
}

fn push_tombstone_outcome(
    state: &mut ImportState,
    handle: String,
    requester: ImportRequester,
    outcome: TombstoneOutcome,
    now: Instant,
    policy: ImportPolicy,
) {
    prune_tombstones(state, now, policy.tombstone_ttl);
    state.tombstones.push_back(Tombstone {
        handle,
        requester,
        outcome,
        created: now,
    });
    while state
        .tombstones
        .iter()
        .filter(|item| item.requester.peer_uid == requester.peer_uid)
        .count()
        > policy.maximum_tombstones_per_uid
    {
        if let Some(position) = state
            .tombstones
            .iter()
            .position(|item| item.requester.peer_uid == requester.peer_uid)
        {
            state.tombstones.remove(position);
        }
    }
}

fn prune_tombstones(state: &mut ImportState, now: Instant, ttl: Duration) {
    state
        .tombstones
        .retain(|item| now.saturating_duration_since(item.created) < ttl);
}

fn receiving_name(handle: &str) -> String {
    format!(".{handle}.receiving")
}

fn ready_name(handle: &str) -> String {
    format!("{handle}.ready")
}

fn cache_home_from_values(
    xdg_cache_home: Option<OsString>,
    home: Option<OsString>,
) -> Result<PathBuf, ImportStoreError> {
    if let Some(value) = xdg_cache_home {
        if value.is_empty() {
            return Err(ImportStoreError::UnsafeCacheRoot);
        }
        let path = PathBuf::from(value);
        return path
            .is_absolute()
            .then_some(path)
            .ok_or(ImportStoreError::UnsafeCacheRoot);
    }
    let home = home.ok_or(ImportStoreError::CacheRootUnavailable)?;
    if home.is_empty() {
        return Err(ImportStoreError::CacheRootUnavailable);
    }
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return Err(ImportStoreError::UnsafeCacheRoot);
    }
    Ok(home.join(".cache"))
}

fn reject_symlink_components(path: &Path) -> Result<(), ImportStoreError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push(Path::new("/")),
            Component::Normal(name) => current.push(name),
            _ => return Err(ImportStoreError::UnsafeCacheRoot),
        }
        let metadata =
            fs::symlink_metadata(&current).map_err(|_| ImportStoreError::UnsafeCacheRoot)?;
        if metadata.file_type().is_symlink() {
            return Err(ImportStoreError::UnsafeCacheRoot);
        }
    }
    Ok(())
}

fn ensure_private_directory<Fd: AsFd>(parent: Fd, name: &str) -> Result<OwnedFd, ImportStoreError> {
    let created = match mkdirat(&parent, name, Mode::RWXU) {
        Ok(()) => true,
        Err(Errno::EXIST) => false,
        Err(_) => return Err(ImportStoreError::UnsafeDirectory),
    };
    let directory = openat(
        &parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| ImportStoreError::UnsafeDirectory)?;
    if created {
        fchmod(&directory, Mode::RWXU).map_err(|_| ImportStoreError::UnsafeDirectory)?;
    }
    validate_owned_directory(&directory, Some(PRIVATE_DIRECTORY_MODE))
        .map_err(|_| ImportStoreError::UnsafeDirectory)?;
    Ok(directory)
}

fn validate_owned_directory<Fd: AsFd>(fd: Fd, mode: Option<u32>) -> Result<(), ()> {
    let stat = fstat(fd).map_err(|_| ())?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != getuid().as_raw()
        || mode.is_some_and(|expected| permission_bits(&stat) != expected)
    {
        return Err(());
    }
    Ok(())
}

fn validate_private_regular_file<Fd: AsFd>(fd: Fd) -> Result<(), ()> {
    let stat = fstat(fd).map_err(|_| ())?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_uid != getuid().as_raw()
        || permission_bits(&stat) != PRIVATE_FILE_MODE
    {
        return Err(());
    }
    Ok(())
}

fn clear_version_directory(directory: &OwnedFd) -> Result<(), ImportStoreError> {
    let mut buffer = [MaybeUninit::uninit(); 8192];
    let mut entries = RawDir::new(directory, &mut buffer);
    while let Some(entry) = entries.next() {
        let entry = entry.map_err(|_| ImportStoreError::UnsafeDirectory)?;
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        let stat = statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| ImportStoreError::UnsafeDirectory)?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
            || stat.st_uid != getuid().as_raw()
            || permission_bits(&stat) != PRIVATE_FILE_MODE
        {
            return Err(ImportStoreError::UnsafeDirectory);
        }
        unlinkat(directory, name, AtFlags::empty())
            .map_err(|_| ImportStoreError::CleanupUnconfirmed)?;
    }
    Ok(())
}

const fn permission_bits(stat: &rustix::fs::Stat) -> u32 {
    stat.st_mode & 0o7777
}

fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn import_size_class(bytes: u64) -> &'static str {
    match bytes {
        0 => "EMPTY",
        1..=65_536 => "LE_64_KIB",
        65_537..=1_048_576 => "LE_1_MIB",
        1_048_577..=16_777_216 => "LE_16_MIB",
        _ => "LE_128_MIB",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::fs::{MemfdFlags, fcntl_add_seals, memfd_create};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_SANDBOX: AtomicU64 = AtomicU64::new(0);

    struct Sandbox {
        root: PathBuf,
    }

    impl Sandbox {
        fn new() -> Self {
            let id = NEXT_SANDBOX.fetch_add(1, Ordering::Relaxed);
            let root =
                env::temp_dir().join(format!("phoneboost-c12-import-{}-{id}", std::process::id()));
            fs::create_dir(&root).expect("create import sandbox");
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("sandbox mode");
            Self { root }
        }

        fn version(&self) -> PathBuf {
            self.root.join("phoneboost/import/v1")
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn requester(connection_id: u64) -> ImportRequester {
        ImportRequester::new(getuid().as_raw(), connection_id)
    }

    fn source(sandbox: &Sandbox, name: &str, bytes: &[u8]) -> OwnedFd {
        let path = sandbox.root.join(name);
        fs::write(&path, bytes).expect("write source fixture");
        File::open(path).expect("open source fixture").into()
    }

    fn consumed_bytes(import: ConsumedImport) -> Vec<u8> {
        let mut file = File::from(import.into_descriptor());
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).expect("read consumed import");
        bytes
    }

    #[test]
    fn locked_constants_match_the_approved_machine_contract() {
        assert_eq!(MAXIMUM_IMPORT_BYTES, 134_217_728);
        assert_eq!(MAXIMUM_IMPORTS_PER_REQUESTER, 4);
        assert_eq!(MAXIMUM_IMPORTS_PER_UID, 8);
        assert_eq!(MAXIMUM_AGGREGATE_BYTES_PER_UID, 268_435_456);
        assert_eq!(MAXIMUM_RECEIVING_PER_REQUESTER, 1);
        assert_eq!(MAXIMUM_RECEIVING_PER_UID, 2);
        assert_eq!(IMPORT_COPY_CHUNK_BYTES, 65_536);
        assert_eq!(IMPORT_RECEIVING_TIMEOUT, Duration::from_millis(60_000));
        assert_eq!(IMPORT_READY_TTL, Duration::from_millis(300_000));
        assert_eq!(IMPORT_CANCELLATION_DEADLINE, Duration::from_millis(5_000));
        assert_eq!(IMPORT_TOMBSTONE_TTL, Duration::from_millis(300_000));
        assert_eq!(MAXIMUM_TOMBSTONES_PER_UID, 256);
    }

    #[test]
    fn regular_file_is_copied_once_privately_and_consumed_once() {
        let sandbox = Sandbox::new();
        let source_path = sandbox.root.join("mutable.bin");
        fs::write(&source_path, b"original").unwrap();
        let store = ImportStore::from_cache_home(&sandbox.root).expect("import store");
        let ready = store
            .import_descriptor(requester(1), File::open(&source_path).unwrap().into())
            .expect("import regular file");
        assert_eq!(ready.bytes(), 8);
        assert_eq!(ready.handle().as_str().len(), 64);
        assert!(
            ready
                .handle()
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        fs::write(&source_path, b"changed!").unwrap();
        let stored = sandbox.version().join(ready_name(ready.handle().as_str()));
        assert_eq!(
            fs::symlink_metadata(&stored).unwrap().mode() & 0o7777,
            0o600
        );
        assert_eq!(
            consumed_bytes(store.consume(requester(1), ready.handle()).unwrap()),
            b"original"
        );
        assert_eq!(
            store.consume(requester(1), ready.handle()).unwrap_err(),
            ImportStoreError::NotFound
        );
    }

    #[test]
    fn cancellation_is_idempotent_and_connection_bound() {
        let sandbox = Sandbox::new();
        let store = ImportStore::from_cache_home(&sandbox.root).unwrap();
        let ready = store
            .import_descriptor(requester(7), source(&sandbox, "cancel.bin", b"payload"))
            .unwrap();
        assert_eq!(
            store.cancel(requester(8), ready.handle()).unwrap_err(),
            ImportStoreError::NotFound
        );
        let first = store.cancel(requester(7), ready.handle()).unwrap();
        assert_eq!(first.state(), ImportTerminalState::Cancelled);
        assert!(first.state_changed());
        let repeat = store.cancel(requester(7), ready.handle()).unwrap();
        assert_eq!(repeat.state(), ImportTerminalState::Cancelled);
        assert!(!repeat.state_changed());
        assert_eq!(store.live_count(), 0);
    }

    #[test]
    fn exact_byte_and_uid_aggregate_boundaries_fail_without_retained_partial() {
        let sandbox = Sandbox::new();
        let policy = ImportPolicy {
            maximum_object_bytes: 8,
            maximum_objects_per_requester: 4,
            maximum_objects_per_uid: 8,
            maximum_aggregate_bytes_per_uid: 12,
            ..ImportPolicy::LOCKED
        };
        let store = ImportStore::from_cache_home_with_policy(&sandbox.root, policy).unwrap();
        let equal = store
            .import_descriptor(requester(1), source(&sandbox, "equal.bin", b"12345678"))
            .unwrap();
        assert_eq!(equal.bytes(), 8);
        assert_eq!(
            store
                .import_descriptor(
                    requester(1),
                    source(&sandbox, "over-object.bin", b"123456789")
                )
                .unwrap_err(),
            ImportStoreError::ObjectTooLarge
        );
        assert_eq!(
            store
                .import_descriptor(requester(2), source(&sandbox, "over-uid.bin", b"12345"))
                .unwrap_err(),
            ImportStoreError::UidByteLimit
        );
        assert_eq!(store.live_count(), 1);
        assert_eq!(fs::read_dir(sandbox.version()).unwrap().count(), 1);
    }

    #[test]
    fn special_objects_and_unsealed_memfd_fail_before_lifecycle_state() {
        let sandbox = Sandbox::new();
        let store = ImportStore::from_cache_home(&sandbox.root).unwrap();
        let directory = open(
            &sandbox.root,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .unwrap();
        assert_eq!(
            store
                .import_descriptor(requester(1), directory)
                .unwrap_err(),
            ImportStoreError::UnsafeSource
        );

        let memfd = memfd_create(
            "c12-unsealed",
            MemfdFlags::ALLOW_SEALING | MemfdFlags::CLOEXEC,
        )
        .unwrap();
        File::from(memfd.try_clone().unwrap())
            .write_all(b"payload")
            .unwrap();
        assert_eq!(
            store.import_descriptor(requester(1), memfd).unwrap_err(),
            ImportStoreError::UnsealedMemfd
        );
        assert_eq!(store.live_count(), 0);
    }

    #[test]
    fn source_owner_must_equal_the_authenticated_peer_uid() {
        let sandbox = Sandbox::new();
        let store = ImportStore::from_cache_home(&sandbox.root).unwrap();
        let wrong_uid = getuid().as_raw().checked_add(1).unwrap();
        assert_eq!(
            store
                .import_descriptor(
                    ImportRequester::new(wrong_uid, 1),
                    source(&sandbox, "wrong-owner.bin", b"payload"),
                )
                .unwrap_err(),
            ImportStoreError::UnsafeSource
        );
        assert_eq!(store.live_count(), 0);
    }

    #[test]
    fn requester_uid_and_receiver_count_boundaries_are_exact() {
        let sandbox = Sandbox::new();
        let store = ImportStore::from_cache_home(&sandbox.root).unwrap();
        let first = store.reserve_import(requester(1)).unwrap();
        assert_eq!(
            store.reserve_import(requester(1)).unwrap_err(),
            ImportStoreError::RequesterReceiverLimit
        );
        let second = store.reserve_import(requester(2)).unwrap();
        assert_eq!(
            store.reserve_import(requester(3)).unwrap_err(),
            ImportStoreError::UidReceiverLimit
        );
        assert_ne!(first, second);
        assert_eq!(store.live_count(), 2);
    }

    #[test]
    fn receiving_deadline_failure_removes_state_and_partial_object() {
        let sandbox = Sandbox::new();
        let policy = ImportPolicy {
            receiving_timeout: Duration::ZERO,
            ..ImportPolicy::LOCKED
        };
        let store = ImportStore::from_cache_home_with_policy(&sandbox.root, policy).unwrap();
        assert_eq!(
            store
                .import_descriptor(requester(1), source(&sandbox, "timeout.bin", b"payload"))
                .unwrap_err(),
            ImportStoreError::ReceivingTimedOut
        );
        assert_eq!(store.live_count(), 0);
        assert_eq!(fs::read_dir(sandbox.version()).unwrap().count(), 0);
    }

    #[test]
    fn handles_payloads_and_descriptors_are_redacted_from_debug_and_errors() {
        let sandbox = Sandbox::new();
        let store = ImportStore::from_cache_home(&sandbox.root).unwrap();
        let ready = store
            .import_descriptor(
                requester(1),
                source(&sandbox, "privacy.bin", b"secret-payload-marker"),
            )
            .unwrap();
        let rendered = format!("{ready:?} {:?}", ready.handle());
        assert!(!rendered.contains(ready.handle().as_str()));
        assert!(!rendered.contains("secret-payload-marker"));
        assert!(!rendered.contains(&ready.bytes().to_string()));
        assert_eq!(
            format!("{:?}", requester(77)),
            "ImportRequester([REDACTED])"
        );
        assert!(
            !ImportStoreError::NotFound
                .to_string()
                .contains(ready.handle().as_str())
        );
    }

    #[test]
    fn fully_sealed_memfd_is_accepted() {
        let sandbox = Sandbox::new();
        let store = ImportStore::from_cache_home(&sandbox.root).unwrap();
        let memfd = memfd_create(
            "c12-sealed",
            MemfdFlags::ALLOW_SEALING | MemfdFlags::CLOEXEC,
        )
        .unwrap();
        File::from(memfd.try_clone().unwrap())
            .write_all(b"sealed")
            .unwrap();
        fcntl_add_seals(
            &memfd,
            SealFlags::SEAL | SealFlags::SHRINK | SealFlags::GROW | SealFlags::WRITE,
        )
        .unwrap();
        let ready = store.import_descriptor(requester(1), memfd).unwrap();
        assert_eq!(
            consumed_bytes(store.consume(requester(1), ready.handle()).unwrap()),
            b"sealed"
        );
    }

    #[test]
    fn disconnect_removes_every_object_for_only_that_connection() {
        let sandbox = Sandbox::new();
        let store = ImportStore::from_cache_home(&sandbox.root).unwrap();
        let first = store
            .import_descriptor(requester(1), source(&sandbox, "first.bin", b"one"))
            .unwrap();
        let second = store
            .import_descriptor(requester(2), source(&sandbox, "second.bin", b"two"))
            .unwrap();
        store.disconnect(requester(1)).unwrap();
        assert_eq!(
            store.consume(requester(1), first.handle()).unwrap_err(),
            ImportStoreError::NotFound
        );
        assert_eq!(
            consumed_bytes(store.consume(requester(2), second.handle()).unwrap()),
            b"two"
        );
    }

    #[test]
    fn ready_expiry_is_exact_and_startup_clears_ready_objects() {
        let sandbox = Sandbox::new();
        let policy = ImportPolicy {
            ready_ttl: Duration::from_millis(10),
            ..ImportPolicy::LOCKED
        };
        let store = ImportStore::from_cache_home_with_policy(&sandbox.root, policy).unwrap();
        let ready = store
            .import_descriptor(requester(1), source(&sandbox, "expiry.bin", b"x"))
            .unwrap();
        let since = match store
            .state
            .lock()
            .unwrap()
            .live
            .get(ready.handle().as_str())
            .unwrap()
            .phase
        {
            ImportPhase::Ready { since } => since,
            ImportPhase::Receiving => panic!("import not ready"),
        };
        assert_eq!(
            store
                .expire_ready_at(since + Duration::from_millis(9))
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .expire_ready_at(since + Duration::from_millis(10))
                .unwrap(),
            1
        );
        assert_eq!(
            store.cancel(requester(1), ready.handle()).unwrap().state(),
            ImportTerminalState::Expired
        );

        let survivor = store
            .import_descriptor(requester(2), source(&sandbox, "restart.bin", b"ready"))
            .unwrap();
        assert!(
            sandbox
                .version()
                .join(ready_name(survivor.handle().as_str()))
                .exists()
        );
        drop(store);
        let restarted = ImportStore::from_cache_home(&sandbox.root).unwrap();
        assert_eq!(restarted.live_count(), 0);
        assert_eq!(fs::read_dir(sandbox.version()).unwrap().count(), 0);
    }

    #[test]
    fn asynchronous_receiver_exposes_only_receiving_then_ready_and_terminal_state() {
        let sandbox = Sandbox::new();
        let store = Arc::new(ImportStore::from_cache_home(&sandbox.root).unwrap());
        let requester = requester(41);
        let handle = store
            .start_import(
                requester,
                source(&sandbox, "async-ready.bin", b"async-payload"),
            )
            .expect("schedule receiver");
        assert_eq!(handle.as_str().len(), 64);

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match store.status(requester, &handle).expect("observable import") {
                ImportStatus::Receiving => assert!(Instant::now() < deadline),
                ImportStatus::Ready { bytes } => {
                    assert_eq!(bytes, 13);
                    break;
                }
                ImportStatus::Terminal(state) => panic!("unexpected terminal state: {state:?}"),
            }
            thread::sleep(Duration::from_millis(5));
        }

        let cancelled = store.cancel(requester, &handle).unwrap();
        assert_eq!(cancelled.state(), ImportTerminalState::Cancelled);
        assert!(cancelled.state_changed());
        assert_eq!(
            store.status(requester, &handle).unwrap(),
            ImportStatus::Terminal(ImportTerminalState::Cancelled)
        );
        let repeated = store.cancel(requester, &handle).unwrap();
        assert!(!repeated.state_changed());
    }

    #[test]
    fn cancellation_waits_for_receiver_descriptor_shutdown_before_success() {
        let sandbox = Sandbox::new();
        let policy = ImportPolicy {
            receiver_chunk_pause: Duration::from_millis(50),
            ..ImportPolicy::LOCKED
        };
        let store =
            Arc::new(ImportStore::from_cache_home_with_policy(&sandbox.root, policy).unwrap());
        let requester = requester(42);
        let payload = vec![7_u8; IMPORT_COPY_CHUNK_BYTES * 2];
        let handle = store
            .start_import(requester, source(&sandbox, "async-cancel.bin", &payload))
            .unwrap();

        let cancelled = store.cancel(requester, &handle).unwrap();
        assert_eq!(cancelled.state(), ImportTerminalState::Cancelled);
        assert!(cancelled.state_changed());
        assert_eq!(store.live_count(), 0);
        assert_eq!(fs::read_dir(sandbox.version()).unwrap().count(), 0);
    }

    #[test]
    fn cancellation_timeout_keeps_quota_until_receiver_later_confirms_cleanup() {
        let sandbox = Sandbox::new();
        let policy = ImportPolicy {
            cancellation_deadline: Duration::ZERO,
            receiver_start_delay: Duration::from_millis(200),
            ..ImportPolicy::LOCKED
        };
        let store =
            Arc::new(ImportStore::from_cache_home_with_policy(&sandbox.root, policy).unwrap());
        let requester = requester(43);
        let handle = store
            .start_import(
                requester,
                source(&sandbox, "async-timeout-cancel.bin", b"payload"),
            )
            .unwrap();

        assert_eq!(
            store.cancel(requester, &handle).unwrap_err(),
            ImportStoreError::CleanupUnconfirmed
        );
        assert_eq!(store.live_count(), 1, "quota remains charged at timeout");

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if store.status(requester, &handle)
                == Ok(ImportStatus::Terminal(ImportTerminalState::Cancelled))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "receiver cleanup acknowledgement"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(store.live_count(), 0);
    }

    #[test]
    fn connection_close_waits_for_live_receiver_cleanup() {
        let sandbox = Sandbox::new();
        let policy = ImportPolicy {
            receiver_start_delay: Duration::from_millis(50),
            ..ImportPolicy::LOCKED
        };
        let store =
            Arc::new(ImportStore::from_cache_home_with_policy(&sandbox.root, policy).unwrap());
        let requester = requester(46);
        let handle = store
            .start_import(
                requester,
                source(&sandbox, "disconnect-receiving.bin", b"payload"),
            )
            .unwrap();
        store.disconnect(requester).expect("disconnect cleanup");
        assert_eq!(
            store.status(requester, &handle).unwrap(),
            ImportStatus::Terminal(ImportTerminalState::Cancelled)
        );
        assert_eq!(store.live_count(), 0);
        assert_eq!(fs::read_dir(sandbox.version()).unwrap().count(), 0);
    }

    #[test]
    fn asynchronous_failure_is_retained_without_handle_or_authority_disclosure() {
        let sandbox = Sandbox::new();
        let policy = ImportPolicy {
            receiving_timeout: Duration::ZERO,
            ..ImportPolicy::LOCKED
        };
        let store =
            Arc::new(ImportStore::from_cache_home_with_policy(&sandbox.root, policy).unwrap());
        let owner = requester(44);
        let handle = store
            .start_import(
                owner,
                source(&sandbox, "async-receive-timeout.bin", b"payload"),
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match store.status(owner, &handle) {
                Err(ImportStoreError::ReceivingTimedOut) => break,
                Ok(ImportStatus::Receiving) => {}
                other => panic!("unexpected async failure state: {other:?}"),
            }
            assert!(Instant::now() < deadline, "timeout tombstone retained");
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            store.status(requester(45), &handle).unwrap_err(),
            ImportStoreError::NotFound
        );
        assert_eq!(store.live_count(), 0);
        assert_eq!(fs::read_dir(sandbox.version()).unwrap().count(), 0);
    }

    #[test]
    fn wire_handle_parser_accepts_only_exact_lowercase_hex() {
        let valid = "0123456789abcdef".repeat(4);
        assert_eq!(ImportHandle::parse_exact(&valid).unwrap().as_str(), valid);
        for invalid in [
            "0123456789abcdef".repeat(3),
            format!("{}g", &valid[..63]),
            valid.to_uppercase(),
            format!("{valid}0"),
        ] {
            assert!(ImportHandle::parse_exact(&invalid).is_none(), "{invalid}");
        }
    }
}
