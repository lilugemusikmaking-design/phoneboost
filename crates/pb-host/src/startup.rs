use std::env;
use std::ffi::OsString;
use std::fmt;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, Stat, chmod, fstat, mkdirat, open, openat, statat, unlinkat,
};
use rustix::io::Errno;
use rustix::net::{
    AddressFamily, SocketAddrUnix, SocketFlags, SocketType, bind, connect, listen, socket_with,
};
use rustix::process::getuid;

use crate::admission::{
    AuthenticatedLocalClient, LocalAdmissionError, LocalAdmissionGate, admit_local_client,
};

const RUNTIME_SUBDIRECTORY: &str = "phoneboost";
const CONTROL_SOCKET: &str = "control.sock";
const PARENT_MODE: u32 = 0o700;
const SOCKET_MODE: u32 = 0o600;
const LISTEN_BACKLOG: i32 = 128;

/// Canonical, structured startup event names emitted by this pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupEventKind {
    LocalRuntimeValidated,
    LocalRuntimeFailed,
    ControlSocketCreated,
}

impl StartupEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalRuntimeValidated => "LOCAL_RUNTIME_VALIDATED",
            Self::LocalRuntimeFailed => "LOCAL_RUNTIME_FAILED",
            Self::ControlSocketCreated => "CONTROL_SOCKET_CREATED",
        }
    }
}

/// A minimal in-memory startup event. No environment value is captured.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupEvent {
    kind: StartupEventKind,
    path: Option<PathBuf>,
}

impl StartupEvent {
    pub const fn kind(&self) -> StartupEventKind {
        self.kind
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

/// In-memory events associated with a non-mutating startup result.
#[derive(Debug)]
pub struct StartupReport {
    events: Vec<StartupEvent>,
}

impl StartupReport {
    pub fn events(&self) -> &[StartupEvent] {
        &self.events
    }
}

/// Failure state, kept separate from canonical General reason codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupErrorKind {
    LocalRuntimeUnsafe,
    StartupRefused,
}

impl StartupErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalRuntimeUnsafe => "LOCAL_RUNTIME_UNSAFE",
            Self::StartupRefused => "STARTUP_REFUSED",
        }
    }
}

/// Redacted diagnostic category; it never contains an environment value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupIssue {
    MissingRuntimeDirectory,
    EmptyRuntimeDirectory,
    RelativeRuntimeDirectory,
    UnsafeRuntimeDirectory,
    UnsafePhoneboostDirectory,
    UnsafeControlObject,
    ControlObjectChanged,
    BindFailed,
    ModeFailed,
    ListenFailed,
}

/// Typed startup failure. Every failure has `state_changed=false`.
#[derive(Debug)]
pub struct StartupError {
    kind: StartupErrorKind,
    issue: StartupIssue,
    event: StartupEvent,
}

impl StartupError {
    pub const fn kind(&self) -> StartupErrorKind {
        self.kind
    }

    pub const fn issue(&self) -> StartupIssue {
        self.issue
    }

    pub const fn state_changed(&self) -> bool {
        false
    }

    pub const fn event(&self) -> &StartupEvent {
        &self.event
    }
}

impl fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {:?}", self.kind.as_str(), self.issue)
    }
}

impl std::error::Error for StartupError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ObjectIdentity {
    file_type: FileType,
    uid: u32,
    device: u64,
    inode: u64,
}

impl ObjectIdentity {
    fn from_stat(stat: &Stat) -> Self {
        Self {
            file_type: FileType::from_raw_mode(stat.st_mode),
            uid: stat.st_uid,
            device: stat.st_dev,
            inode: stat.st_ino,
        }
    }
}

/// A listening runtime authority created by this startup attempt.
///
/// The listening descriptor and validated parent descriptor remain alive for
/// the lifetime of this value. Drop removes the pathname only if its identity
/// is still exactly the one created by this attempt.
pub struct ReadyRuntime {
    listener: OwnedFd,
    admission_gate: LocalAdmissionGate,
    parent: OwnedFd,
    socket_path: PathBuf,
    socket_identity: ObjectIdentity,
    events: Vec<StartupEvent>,
}

impl fmt::Debug for ReadyRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReadyRuntime")
            .field("socket_path", &self.socket_path)
            .field("socket_identity", &self.socket_identity)
            .field("events", &self.events)
            .finish_non_exhaustive()
    }
}

impl ReadyRuntime {
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn events(&self) -> &[StartupEvent] {
        &self.events
    }

    pub const fn state_changed(&self) -> bool {
        true
    }

    /// Accept and authenticate exactly one local connection.
    ///
    /// No application byte is read by this operation. A returned client has
    /// reached `AUTHENTICATED_LOCAL`, never `ACTIVE`.
    pub fn accept_local_client(&self) -> Result<AuthenticatedLocalClient, LocalAdmissionError> {
        admit_local_client(self.listener.as_fd(), &self.admission_gate)
    }

    pub fn authenticated_active_clients(&self) -> usize {
        self.admission_gate.active_clients()
    }
}

impl Drop for ReadyRuntime {
    fn drop(&mut self) {
        let _listener_is_kept_alive_until_cleanup = &self.listener;
        safe_unlink_if_same(&self.parent, self.socket_identity);
    }
}

/// The only successful states of `HOST_STARTUP`.
#[derive(Debug)]
pub enum StartupOutcome {
    Ready(ReadyRuntime),
    AlreadyRunning(StartupReport),
}

impl StartupOutcome {
    pub const fn state_changed(&self) -> bool {
        matches!(self, Self::Ready(_))
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Ready(_) => "READY",
            Self::AlreadyRunning(_) => "ALREADY_RUNNING",
        }
    }

    pub fn events(&self) -> &[StartupEvent] {
        match self {
            Self::Ready(ready) => ready.events(),
            Self::AlreadyRunning(report) => report.events(),
        }
    }
}

/// Validate local authority and create or recover the canonical control socket.
pub fn host_startup() -> Result<StartupOutcome, StartupError> {
    #[cfg(test)]
    {
        let mut hooks = TestHooks::default();
        startup_from_runtime_value(env::var_os("XDG_RUNTIME_DIR"), &mut hooks)
    }

    #[cfg(not(test))]
    {
        startup_from_runtime_value(env::var_os("XDG_RUNTIME_DIR"))
    }
}

fn startup_from_runtime_value(
    runtime_value: Option<OsString>,
    #[cfg(test)] hooks: &mut TestHooks<'_>,
) -> Result<StartupOutcome, StartupError> {
    let runtime_value =
        runtime_value.ok_or_else(|| local_runtime_error(StartupIssue::MissingRuntimeDirectory))?;
    if runtime_value.is_empty() {
        return Err(local_runtime_error(StartupIssue::EmptyRuntimeDirectory));
    }

    let runtime_path = PathBuf::from(runtime_value);
    if !runtime_path.is_absolute() {
        return Err(local_runtime_error(StartupIssue::RelativeRuntimeDirectory));
    }

    let current_uid = getuid().as_raw();
    let runtime_fd = open(
        &runtime_path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| local_runtime_error(StartupIssue::UnsafeRuntimeDirectory))?;
    let runtime_stat = fstat(&runtime_fd)
        .map_err(|_| local_runtime_error(StartupIssue::UnsafeRuntimeDirectory))?;
    if FileType::from_raw_mode(runtime_stat.st_mode) != FileType::Directory
        || runtime_stat.st_uid != current_uid
        || permission_bits(&runtime_stat) & 0o077 != 0
    {
        return Err(local_runtime_error(StartupIssue::UnsafeRuntimeDirectory));
    }

    let created_parent = match mkdirat(&runtime_fd, RUNTIME_SUBDIRECTORY, Mode::RWXU) {
        Ok(()) => true,
        Err(Errno::EXIST) => false,
        Err(_) => return Err(local_runtime_error(StartupIssue::UnsafePhoneboostDirectory)),
    };
    let parent = openat(
        &runtime_fd,
        RUNTIME_SUBDIRECTORY,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| local_runtime_error(StartupIssue::UnsafePhoneboostDirectory))?;
    let parent_stat =
        fstat(&parent).map_err(|_| local_runtime_error(StartupIssue::UnsafePhoneboostDirectory))?;
    if FileType::from_raw_mode(parent_stat.st_mode) != FileType::Directory
        || parent_stat.st_uid != current_uid
        || permission_bits(&parent_stat) != PARENT_MODE
    {
        return Err(local_runtime_error(StartupIssue::UnsafePhoneboostDirectory));
    }
    let _created_parent_is_validated_without_repair = created_parent;

    let socket_path = runtime_path.join(RUNTIME_SUBDIRECTORY).join(CONTROL_SOCKET);
    let validated_event = StartupEvent {
        kind: StartupEventKind::LocalRuntimeValidated,
        path: Some(runtime_path),
    };

    let first = match statat(&parent, CONTROL_SOCKET, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => Some(ObjectIdentity::from_stat(&stat)),
        Err(Errno::NOENT) => None,
        Err(_) => return Err(startup_refused(StartupIssue::UnsafeControlObject)),
    };

    if let Some(first_identity) = first {
        if first_identity.file_type != FileType::Socket || first_identity.uid != current_uid {
            return Err(startup_refused(StartupIssue::UnsafeControlObject));
        }

        if connect_existing(&socket_path).is_ok() {
            return Ok(StartupOutcome::AlreadyRunning(StartupReport {
                events: vec![validated_event],
            }));
        }

        #[cfg(test)]
        if let Some(hook) = hooks.before_second_stat.as_mut() {
            hook(&parent);
        }

        let second = statat(&parent, CONTROL_SOCKET, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| startup_refused(StartupIssue::ControlObjectChanged))?;
        let second_identity = ObjectIdentity::from_stat(&second);
        if second_identity != first_identity {
            return Err(startup_refused(StartupIssue::ControlObjectChanged));
        }

        unlinkat(&parent, CONTROL_SOCKET, AtFlags::empty())
            .map_err(|_| startup_refused(StartupIssue::ControlObjectChanged))?;
    }

    bind_fresh(
        parent,
        socket_path,
        validated_event,
        #[cfg(test)]
        hooks,
    )
}

fn connect_existing(socket_path: &Path) -> Result<(), Errno> {
    let socket = socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC,
        None,
    )?;
    let address = SocketAddrUnix::new(socket_path)?;
    connect(&socket, &address)
}

fn bind_fresh(
    parent: OwnedFd,
    socket_path: PathBuf,
    validated_event: StartupEvent,
    #[cfg(test)] hooks: &mut TestHooks<'_>,
) -> Result<StartupOutcome, StartupError> {
    let listener = socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC,
        None,
    )
    .map_err(|_| startup_refused(StartupIssue::BindFailed))?;
    let address =
        SocketAddrUnix::new(&socket_path).map_err(|_| startup_refused(StartupIssue::BindFailed))?;
    bind(&listener, &address).map_err(|_| startup_refused(StartupIssue::BindFailed))?;

    let pinned_socket = match openat(
        &parent,
        CONTROL_SOCKET,
        OFlags::PATH | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(_) => return Err(startup_refused(StartupIssue::ControlObjectChanged)),
    };
    let bound_stat = match fstat(&pinned_socket) {
        Ok(stat) => stat,
        Err(_) => return Err(startup_refused(StartupIssue::ControlObjectChanged)),
    };
    let bound_identity = ObjectIdentity::from_stat(&bound_stat);
    if bound_identity.file_type != FileType::Socket || bound_identity.uid != getuid().as_raw() {
        return Err(startup_refused(StartupIssue::ControlObjectChanged));
    }

    #[cfg(test)]
    if let Some(hook) = hooks.after_bind.as_mut() {
        hook(&parent);
    }

    #[cfg(test)]
    if hooks.force_mode_failure {
        safe_unlink_if_same(&parent, bound_identity);
        return Err(startup_refused(StartupIssue::ModeFailed));
    }

    if chmod_pinned_socket(&pinned_socket).is_err() {
        safe_unlink_if_same(&parent, bound_identity);
        return Err(startup_refused(StartupIssue::ModeFailed));
    }

    let mode_stat = match statat(&parent, CONTROL_SOCKET, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(_) => {
            safe_unlink_if_same(&parent, bound_identity);
            return Err(startup_refused(StartupIssue::ControlObjectChanged));
        }
    };
    let mode_identity = ObjectIdentity::from_stat(&mode_stat);
    if mode_identity != bound_identity || permission_bits(&mode_stat) != SOCKET_MODE {
        safe_unlink_if_same(&parent, bound_identity);
        return Err(startup_refused(if mode_identity == bound_identity {
            StartupIssue::ModeFailed
        } else {
            StartupIssue::ControlObjectChanged
        }));
    }

    if listen(&listener, LISTEN_BACKLOG).is_err() {
        safe_unlink_if_same(&parent, bound_identity);
        return Err(startup_refused(StartupIssue::ListenFailed));
    }

    let final_stat = match statat(&parent, CONTROL_SOCKET, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(_) => {
            safe_unlink_if_same(&parent, bound_identity);
            return Err(startup_refused(StartupIssue::ControlObjectChanged));
        }
    };
    if ObjectIdentity::from_stat(&final_stat) != bound_identity
        || permission_bits(&final_stat) != SOCKET_MODE
    {
        safe_unlink_if_same(&parent, bound_identity);
        return Err(startup_refused(StartupIssue::ControlObjectChanged));
    }

    Ok(StartupOutcome::Ready(ReadyRuntime {
        listener,
        admission_gate: LocalAdmissionGate::new(),
        parent,
        socket_path: socket_path.clone(),
        socket_identity: bound_identity,
        events: vec![
            validated_event,
            StartupEvent {
                kind: StartupEventKind::ControlSocketCreated,
                path: Some(socket_path),
            },
        ],
    }))
}

fn chmod_pinned_socket(socket: &OwnedFd) -> Result<(), Errno> {
    let proc_fd_path = PathBuf::from(format!("/proc/self/fd/{}", socket.as_raw_fd()));
    chmod(proc_fd_path, Mode::RUSR | Mode::WUSR)
}

fn safe_unlink_if_same(parent: &OwnedFd, expected: ObjectIdentity) {
    let current = statat(parent, CONTROL_SOCKET, AtFlags::SYMLINK_NOFOLLOW)
        .map(|stat| ObjectIdentity::from_stat(&stat));
    if current == Ok(expected) {
        let _result = unlinkat(parent, CONTROL_SOCKET, AtFlags::empty());
    }
}

fn permission_bits(stat: &Stat) -> u32 {
    stat.st_mode & 0o7777
}

fn local_runtime_error(issue: StartupIssue) -> StartupError {
    StartupError {
        kind: StartupErrorKind::LocalRuntimeUnsafe,
        issue,
        event: StartupEvent {
            kind: StartupEventKind::LocalRuntimeFailed,
            path: None,
        },
    }
}

fn startup_refused(issue: StartupIssue) -> StartupError {
    StartupError {
        kind: StartupErrorKind::StartupRefused,
        issue,
        event: StartupEvent {
            kind: StartupEventKind::LocalRuntimeFailed,
            path: None,
        },
    }
}

#[cfg(test)]
#[derive(Default)]
struct TestHooks<'a> {
    before_second_stat: Option<&'a mut dyn FnMut(&OwnedFd)>,
    after_bind: Option<&'a mut dyn FnMut(&OwnedFd)>,
    force_mode_failure: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt, symlink};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct Sandbox {
        root: PathBuf,
    }

    impl Sandbox {
        fn new(label: &str) -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let root = env::temp_dir().join(format!(
                "phoneboost-host-startup-{}-{label}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("create isolated test runtime directory");
            fs::set_permissions(&root, fs::Permissions::from_mode(PARENT_MODE))
                .expect("set isolated test runtime permissions");
            Self { root }
        }

        fn prepare_parent(&self) -> PathBuf {
            let parent = self.root.join(RUNTIME_SUBDIRECTORY);
            fs::create_dir(&parent).expect("create phoneboost parent");
            fs::set_permissions(&parent, fs::Permissions::from_mode(PARENT_MODE))
                .expect("set phoneboost parent permissions");
            parent
        }

        fn start(&self) -> Result<StartupOutcome, StartupError> {
            let mut hooks = TestHooks::default();
            startup_from_runtime_value(Some(self.root.clone().into_os_string()), &mut hooks)
        }

        fn start_with(&self, hooks: &mut TestHooks<'_>) -> Result<StartupOutcome, StartupError> {
            startup_from_runtime_value(Some(self.root.clone().into_os_string()), hooks)
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _cleanup = fs::remove_dir_all(&self.root);
        }
    }

    fn error_kind(result: Result<StartupOutcome, StartupError>) -> StartupErrorKind {
        match result {
            Err(error) => {
                assert!(!error.state_changed());
                error.kind()
            }
            Ok(outcome) => panic!("startup unexpectedly returned {}", outcome.as_str()),
        }
    }

    fn ready(result: Result<StartupOutcome, StartupError>) -> ReadyRuntime {
        match result.expect("startup should succeed") {
            StartupOutcome::Ready(ready) => ready,
            StartupOutcome::AlreadyRunning(_) => {
                panic!("startup unexpectedly found an active daemon")
            }
        }
    }

    fn control_path(parent: &Path) -> PathBuf {
        parent.join(CONTROL_SOCKET)
    }

    fn stale_socket(path: &Path) {
        let listener = UnixListener::bind(path).expect("bind stale socket fixture");
        drop(listener);
    }

    #[test]
    fn hs_t01_missing_xdg_is_local_runtime_unsafe() {
        let mut hooks = TestHooks::default();
        let result = startup_from_runtime_value(None, &mut hooks);
        assert_eq!(error_kind(result), StartupErrorKind::LocalRuntimeUnsafe);
    }

    #[test]
    fn hs_t02_empty_xdg_fails_closed() {
        let mut hooks = TestHooks::default();
        let result = startup_from_runtime_value(Some(OsString::new()), &mut hooks);
        assert_eq!(error_kind(result), StartupErrorKind::LocalRuntimeUnsafe);
    }

    #[test]
    fn hs_t03_relative_xdg_fails_closed() {
        let mut hooks = TestHooks::default();
        let result = startup_from_runtime_value(Some(OsString::from("relative")), &mut hooks);
        assert_eq!(error_kind(result), StartupErrorKind::LocalRuntimeUnsafe);
    }

    #[test]
    fn hs_t04_nonexistent_xdg_fails_closed() {
        let sandbox = Sandbox::new("nonexistent");
        let missing = sandbox.root.join("missing");
        let mut hooks = TestHooks::default();
        let result = startup_from_runtime_value(Some(missing.into_os_string()), &mut hooks);
        assert_eq!(error_kind(result), StartupErrorKind::LocalRuntimeUnsafe);
    }

    #[test]
    fn hs_t05_regular_file_xdg_fails_closed() {
        let sandbox = Sandbox::new("xdg-file");
        let file = sandbox.root.join("runtime-file");
        File::create(&file).expect("create regular file");
        let mut hooks = TestHooks::default();
        let result = startup_from_runtime_value(Some(file.into_os_string()), &mut hooks);
        assert_eq!(error_kind(result), StartupErrorKind::LocalRuntimeUnsafe);
    }

    #[test]
    fn hs_t06_symlink_xdg_fails_closed() {
        let sandbox = Sandbox::new("xdg-symlink");
        let link = sandbox.root.join("runtime-link");
        symlink(&sandbox.root, &link).expect("create XDG symlink");
        let mut hooks = TestHooks::default();
        let result = startup_from_runtime_value(Some(link.into_os_string()), &mut hooks);
        assert_eq!(error_kind(result), StartupErrorKind::LocalRuntimeUnsafe);
    }

    #[test]
    fn hs_t08_xdg_group_bit_fails_closed() {
        let sandbox = Sandbox::new("xdg-group");
        fs::set_permissions(&sandbox.root, fs::Permissions::from_mode(0o710))
            .expect("set group bit");
        assert_eq!(
            error_kind(sandbox.start()),
            StartupErrorKind::LocalRuntimeUnsafe
        );
    }

    #[test]
    fn hs_t09_xdg_other_bit_fails_closed() {
        let sandbox = Sandbox::new("xdg-other");
        fs::set_permissions(&sandbox.root, fs::Permissions::from_mode(0o701))
            .expect("set other bit");
        assert_eq!(
            error_kind(sandbox.start()),
            StartupErrorKind::LocalRuntimeUnsafe
        );
    }

    #[test]
    fn hs_t10_absent_parent_is_created_securely() {
        let sandbox = Sandbox::new("parent-create");
        let ready = ready(sandbox.start());
        let metadata = fs::symlink_metadata(sandbox.root.join(RUNTIME_SUBDIRECTORY))
            .expect("stat created parent");
        assert_eq!(metadata.mode() & 0o7777, PARENT_MODE);
        assert_eq!(metadata.uid(), getuid().as_raw());
        drop(ready);
    }

    #[test]
    fn hs_t11_existing_correct_parent_is_accepted() {
        let sandbox = Sandbox::new("parent-existing");
        sandbox.prepare_parent();
        let ready = ready(sandbox.start());
        assert!(ready.state_changed());
    }

    #[test]
    fn hs_t12_parent_symlink_fails_closed() {
        let sandbox = Sandbox::new("parent-symlink");
        let target = sandbox.root.join("target");
        fs::create_dir(&target).expect("create symlink target");
        fs::set_permissions(&target, fs::Permissions::from_mode(PARENT_MODE))
            .expect("secure target mode");
        symlink(&target, sandbox.root.join(RUNTIME_SUBDIRECTORY)).expect("create parent symlink");
        assert_eq!(
            error_kind(sandbox.start()),
            StartupErrorKind::LocalRuntimeUnsafe
        );
    }

    #[test]
    fn hs_t13_parent_regular_file_fails_closed() {
        let sandbox = Sandbox::new("parent-file");
        File::create(sandbox.root.join(RUNTIME_SUBDIRECTORY)).expect("create parent file");
        assert_eq!(
            error_kind(sandbox.start()),
            StartupErrorKind::LocalRuntimeUnsafe
        );
    }

    #[test]
    fn hs_t14_parent_wrong_mode_fails_without_repair() {
        let sandbox = Sandbox::new("parent-mode");
        let parent = sandbox.prepare_parent();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).expect("set unsafe mode");
        assert_eq!(
            error_kind(sandbox.start()),
            StartupErrorKind::LocalRuntimeUnsafe
        );
        let mode = fs::symlink_metadata(parent)
            .expect("stat unsafe parent")
            .mode()
            & 0o7777;
        assert_eq!(mode, 0o755);
    }

    #[test]
    fn hs_t16_missing_control_binds_and_listens() {
        let sandbox = Sandbox::new("control-missing");
        let ready = ready(sandbox.start());
        assert_eq!(
            ready.socket_path(),
            control_path(&sandbox.root.join(RUNTIME_SUBDIRECTORY))
        );
        let second = sandbox
            .start()
            .expect("second startup should detect listener");
        assert!(matches!(second, StartupOutcome::AlreadyRunning(_)));
        assert!(!second.state_changed());
    }

    #[test]
    fn typed_outcomes_emit_only_structured_startup_events() {
        let sandbox = Sandbox::new("typed-events");
        let first = sandbox.start().expect("fresh startup should succeed");
        assert!(first.state_changed());
        assert_eq!(first.as_str(), "READY");
        assert_eq!(
            first
                .events()
                .iter()
                .map(|event| event.kind().as_str())
                .collect::<Vec<_>>(),
            vec!["LOCAL_RUNTIME_VALIDATED", "CONTROL_SOCKET_CREATED"]
        );

        let second = sandbox.start().expect("active startup should succeed");
        assert!(!second.state_changed());
        assert_eq!(second.as_str(), "ALREADY_RUNNING");
        assert_eq!(
            second
                .events()
                .iter()
                .map(|event| event.kind().as_str())
                .collect::<Vec<_>>(),
            vec!["LOCAL_RUNTIME_VALIDATED"]
        );
    }

    #[test]
    fn pass3a_ready_runtime_listener_admits_same_uid_without_becoming_active() {
        let sandbox = Sandbox::new("ready-admission");
        let ready = ready(sandbox.start());
        let _peer = UnixStream::connect(ready.socket_path()).expect("connect to ReadyRuntime");
        let authenticated = ready
            .accept_local_client()
            .expect("ReadyRuntime should admit same UID");
        assert_eq!(authenticated.peer_uid(), getuid().as_raw());
        assert_eq!(
            authenticated.state(),
            crate::LocalConnectionState::AuthenticatedLocal
        );
        assert_eq!(ready.authenticated_active_clients(), 1);
        drop(authenticated);
        assert_eq!(ready.authenticated_active_clients(), 0);
    }

    #[test]
    fn hs_t17_active_socket_is_already_running() {
        let sandbox = Sandbox::new("control-active");
        let first = ready(sandbox.start());
        let identity = fs::symlink_metadata(first.socket_path())
            .expect("stat active socket")
            .ino();
        let second = sandbox.start().expect("active socket should be accepted");
        assert!(matches!(second, StartupOutcome::AlreadyRunning(_)));
        assert!(!second.state_changed());
        assert_eq!(
            fs::symlink_metadata(first.socket_path())
                .expect("socket survives")
                .ino(),
            identity
        );
    }

    #[test]
    fn hs_t18_regular_control_is_refused_without_deletion() {
        let sandbox = Sandbox::new("control-file");
        let parent = sandbox.prepare_parent();
        let control = control_path(&parent);
        fs::write(&control, b"do-not-delete").expect("create unsafe control file");
        assert_eq!(
            error_kind(sandbox.start()),
            StartupErrorKind::StartupRefused
        );
        assert_eq!(
            fs::read(control).expect("unsafe file survives"),
            b"do-not-delete"
        );
    }

    #[test]
    fn hs_t19_control_symlink_is_refused_without_target_deletion() {
        let sandbox = Sandbox::new("control-symlink");
        let parent = sandbox.prepare_parent();
        let target = sandbox.root.join("target-data");
        fs::write(&target, b"target-survives").expect("create symlink target");
        let control = control_path(&parent);
        symlink(&target, &control).expect("create control symlink");
        assert_eq!(
            error_kind(sandbox.start()),
            StartupErrorKind::StartupRefused
        );
        assert!(
            fs::symlink_metadata(control)
                .expect("symlink survives")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read(target).expect("target survives"),
            b"target-survives"
        );
    }

    #[test]
    fn hs_t20_unchanged_stale_socket_is_recovered() {
        let sandbox = Sandbox::new("control-stale");
        let parent = sandbox.prepare_parent();
        let control = control_path(&parent);
        stale_socket(&control);
        let stale_inode = fs::symlink_metadata(&control)
            .expect("stat stale socket")
            .ino();
        let ready = ready(sandbox.start());
        let fresh = fs::symlink_metadata(ready.socket_path()).expect("stat fresh listener");
        assert!(fresh.file_type().is_socket());
        assert_ne!(fresh.ino(), stale_inode);
    }

    #[test]
    fn hs_t21_replaced_stale_socket_is_refused_and_survives() {
        let sandbox = Sandbox::new("control-replaced");
        let parent = sandbox.prepare_parent();
        let control = control_path(&parent);
        stale_socket(&control);
        let replacement_path = control.clone();
        let mut replace = move |parent_fd: &OwnedFd| {
            unlinkat(parent_fd, CONTROL_SOCKET, AtFlags::empty()).expect("remove stale fixture");
            stale_socket(&replacement_path);
        };
        let mut hooks = TestHooks {
            before_second_stat: Some(&mut replace),
            ..TestHooks::default()
        };
        assert_eq!(
            error_kind(sandbox.start_with(&mut hooks)),
            StartupErrorKind::StartupRefused
        );
        assert!(
            fs::symlink_metadata(control)
                .expect("replacement survives")
                .file_type()
                .is_socket()
        );
    }

    #[test]
    fn hs_t22_inode_change_is_refused() {
        let sandbox = Sandbox::new("control-inode");
        let parent = sandbox.prepare_parent();
        let control = control_path(&parent);
        stale_socket(&control);
        let first_inode = fs::symlink_metadata(&control)
            .expect("stat first socket")
            .ino();
        let replacement_path = control.clone();
        let mut replace = move |parent_fd: &OwnedFd| {
            unlinkat(parent_fd, CONTROL_SOCKET, AtFlags::empty()).expect("remove first socket");
            stale_socket(&replacement_path);
        };
        let mut hooks = TestHooks {
            before_second_stat: Some(&mut replace),
            ..TestHooks::default()
        };
        assert_eq!(
            error_kind(sandbox.start_with(&mut hooks)),
            StartupErrorKind::StartupRefused
        );
        assert_ne!(
            fs::symlink_metadata(control)
                .expect("stat replacement")
                .ino(),
            first_inode
        );
    }

    #[test]
    fn hs_t23_type_change_is_refused_and_survives() {
        let sandbox = Sandbox::new("control-type");
        let parent = sandbox.prepare_parent();
        let control = control_path(&parent);
        stale_socket(&control);
        let replacement_path = control.clone();
        let mut replace = move |parent_fd: &OwnedFd| {
            unlinkat(parent_fd, CONTROL_SOCKET, AtFlags::empty()).expect("remove stale socket");
            fs::write(&replacement_path, b"replacement").expect("create replacement file");
        };
        let mut hooks = TestHooks {
            before_second_stat: Some(&mut replace),
            ..TestHooks::default()
        };
        assert_eq!(
            error_kind(sandbox.start_with(&mut hooks)),
            StartupErrorKind::StartupRefused
        );
        assert_eq!(
            fs::read(control).expect("replacement survives"),
            b"replacement"
        );
    }

    #[test]
    fn hs_t25_overlong_bind_path_never_reports_ready() {
        let sandbox = Sandbox::new("bind-long");
        let long_runtime = sandbox.root.join("x".repeat(90));
        fs::create_dir(&long_runtime).expect("create long runtime path");
        fs::set_permissions(&long_runtime, fs::Permissions::from_mode(PARENT_MODE))
            .expect("secure long runtime path");
        let mut hooks = TestHooks::default();
        let result = startup_from_runtime_value(Some(long_runtime.into_os_string()), &mut hooks);
        assert_eq!(error_kind(result), StartupErrorKind::StartupRefused);
    }

    #[test]
    fn hs_t26_forced_mode_failure_never_reports_ready() {
        let sandbox = Sandbox::new("mode-failure");
        let mut hooks = TestHooks {
            force_mode_failure: true,
            ..TestHooks::default()
        };
        assert_eq!(
            error_kind(sandbox.start_with(&mut hooks)),
            StartupErrorKind::StartupRefused
        );
        assert!(!control_path(&sandbox.root.join(RUNTIME_SUBDIRECTORY)).exists());
    }

    #[test]
    fn hs_t27_final_socket_mode_is_exactly_0600() {
        let sandbox = Sandbox::new("socket-mode");
        let ready = ready(sandbox.start());
        let mode = fs::symlink_metadata(ready.socket_path())
            .expect("stat control socket")
            .mode()
            & 0o7777;
        assert_eq!(mode, SOCKET_MODE);
    }

    #[test]
    fn hs_t28_parent_mode_remains_0700() {
        let sandbox = Sandbox::new("parent-mode-final");
        let ready = ready(sandbox.start());
        let parent = ready.socket_path().parent().expect("socket has parent");
        let mode = fs::symlink_metadata(parent).expect("stat parent").mode() & 0o7777;
        assert_eq!(mode, PARENT_MODE);
    }

    #[test]
    fn hs_t29_missing_xdg_does_not_create_fallback_authority() {
        let mut hooks = TestHooks::default();
        let result = startup_from_runtime_value(None, &mut hooks);
        assert_eq!(error_kind(result), StartupErrorKind::LocalRuntimeUnsafe);
    }

    #[test]
    fn hs_t32_active_socket_is_never_removed() {
        let sandbox = Sandbox::new("active-survives");
        let first = ready(sandbox.start());
        let identity = fs::symlink_metadata(first.socket_path())
            .expect("stat listener")
            .ino();
        let second = sandbox.start().expect("detect active listener");
        drop(second);
        assert_eq!(
            fs::symlink_metadata(first.socket_path())
                .expect("listener survives")
                .ino(),
            identity
        );
        assert!(matches!(
            sandbox.start(),
            Ok(StartupOutcome::AlreadyRunning(_))
        ));
    }

    #[test]
    fn hs_t33_unsafe_parent_is_not_repaired() {
        let sandbox = Sandbox::new("no-repair");
        let parent = sandbox.prepare_parent();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o750)).expect("set unsafe mode");
        assert_eq!(
            error_kind(sandbox.start()),
            StartupErrorKind::LocalRuntimeUnsafe
        );
        assert_eq!(
            fs::symlink_metadata(parent).expect("stat parent").mode() & 0o7777,
            0o750
        );
    }

    #[test]
    fn oracle_rejects_every_security_identity_mutation() {
        let baseline = ObjectIdentity {
            file_type: FileType::Socket,
            uid: 1000,
            device: 7,
            inode: 11,
        };
        assert_ne!(
            baseline,
            ObjectIdentity {
                file_type: FileType::RegularFile,
                ..baseline
            }
        );
        assert_ne!(
            baseline,
            ObjectIdentity {
                uid: 1001,
                ..baseline
            }
        );
        assert_ne!(
            baseline,
            ObjectIdentity {
                device: 8,
                ..baseline
            }
        );
        assert_ne!(
            baseline,
            ObjectIdentity {
                inode: 12,
                ..baseline
            }
        );
        assert_eq!(baseline, ObjectIdentity { ..baseline });
    }

    #[test]
    fn after_bind_replacement_survives_final_verification() {
        let sandbox = Sandbox::new("after-bind-replace");
        let control = control_path(&sandbox.root.join(RUNTIME_SUBDIRECTORY));
        let replacement_path = control.clone();
        let mut replace = move |parent_fd: &OwnedFd| {
            unlinkat(parent_fd, CONTROL_SOCKET, AtFlags::empty()).expect("unlink bound socket");
            fs::write(&replacement_path, b"after-bind replacement")
                .expect("create after-bind replacement");
        };
        let mut hooks = TestHooks {
            after_bind: Some(&mut replace),
            ..TestHooks::default()
        };
        assert_eq!(
            error_kind(sandbox.start_with(&mut hooks)),
            StartupErrorKind::StartupRefused
        );
        assert_eq!(
            fs::read(control).expect("replacement survives"),
            b"after-bind replacement"
        );
    }
}
