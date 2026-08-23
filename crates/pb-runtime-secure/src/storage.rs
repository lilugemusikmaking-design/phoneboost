use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use curve25519_dalek::montgomery::MontgomeryPoint;
use pb_secure::PairingGuard;
use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, RawDir, fstat, fsync, mkdirat, open, openat, renameat, statat,
    unlinkat,
};
use rustix::io::Errno;
use rustix::process::getuid;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const IDENTITY_FILE: &str = "identity.key";
const GUARD_FILE: &str = "pairing_guard.json";
const PEERS_DIRECTORY: &str = "peers";
const DIRECTORY_MODE: u32 = 0o700;
const AUTHORITY_MODE: u32 = 0o600;
const MAX_AUTHORITY_BYTES: usize = 16 * 1024;
const MAX_PEERS: usize = 64;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageError {
    StateRootUnavailable,
    UnsafePath,
    Io,
    CorruptIdentity,
    CorruptGuard,
    CorruptPeer,
    TooManyPeers,
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::StateRootUnavailable => "state root unavailable",
            Self::UnsafePath => "unsafe state path",
            Self::Io => "state I/O failed",
            Self::CorruptIdentity => "identity is corrupt",
            Self::CorruptGuard => "pairing guard is corrupt",
            Self::CorruptPeer => "peer record is corrupt",
            Self::TooManyPeers => "peer record limit exceeded",
        })
    }
}

impl std::error::Error for StorageError {}

#[derive(Clone, Eq, PartialEq)]
pub struct Identity {
    private: [u8; 32],
    public: [u8; 32],
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Identity")
            .field("private", &"REDACTED")
            .field("public", &hex_encode(&self.public))
            .finish()
    }
}

impl Identity {
    pub const fn private(&self) -> &[u8; 32] {
        &self.private
    }

    pub const fn public(&self) -> &[u8; 32] {
        &self.public
    }

    pub fn peer_id(&self) -> String {
        peer_id(&self.public)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerRecord {
    pub schema_version: u64,
    pub peer_id: String,
    pub static_public_key: [u8; 32],
    pub alias: String,
    pub paired_wall_ms: u64,
    pub last_seen_wall_ms: u64,
    pub core_version: u64,
    pub pbmux_version: u64,
}

impl PeerRecord {
    pub fn new(static_public_key: [u8; 32], alias: impl Into<String>, now_ms: u64) -> Self {
        Self {
            schema_version: 1,
            peer_id: peer_id(&static_public_key),
            static_public_key,
            alias: alias.into(),
            paired_wall_ms: now_ms,
            last_seen_wall_ms: now_ms,
            core_version: 1,
            pbmux_version: 1,
        }
    }
}

pub struct StateStore {
    root: OwnedFd,
    peers: OwnedFd,
}

impl std::fmt::Debug for StateStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("StateStore").finish_non_exhaustive()
    }
}

impl StateStore {
    pub fn open_host() -> Result<Self, StorageError> {
        let root_path = canonical_host_root()?;
        create_secure_directory(&root_path)?;
        reject_symlink_components(&root_path)?;
        let root = open(
            &root_path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| StorageError::UnsafePath)?;
        Self::from_directory_fd(root)
    }

    pub fn from_directory_fd(root: OwnedFd) -> Result<Self, StorageError> {
        validate_directory(&root)?;
        match mkdirat(&root, PEERS_DIRECTORY, Mode::RWXU) {
            Ok(()) | Err(Errno::EXIST) => {}
            Err(_) => return Err(StorageError::Io),
        }
        let peers = openat(
            &root,
            PEERS_DIRECTORY,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| StorageError::UnsafePath)?;
        validate_directory(&peers)?;
        Ok(Self { root, peers })
    }

    pub fn load_or_create_identity(&self) -> Result<Identity, StorageError> {
        match statat(&self.root, IDENTITY_FILE, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => self.load_identity(),
            Err(Errno::NOENT) => {
                let mut private = [0_u8; 32];
                File::open("/dev/urandom")
                    .and_then(|mut random| random.read_exact(&mut private))
                    .map_err(|_| StorageError::Io)?;
                if private.iter().all(|byte| *byte == 0) {
                    return Err(StorageError::Io);
                }
                self.atomic_write(&self.root, IDENTITY_FILE, &private)?;
                self.load_identity()
            }
            Err(_) => Err(StorageError::UnsafePath),
        }
    }

    pub fn load_identity(&self) -> Result<Identity, StorageError> {
        let bytes =
            read_authority(&self.root, IDENTITY_FILE).map_err(|_| StorageError::CorruptIdentity)?;
        let private: [u8; 32] = bytes
            .try_into()
            .map_err(|_| StorageError::CorruptIdentity)?;
        if private.iter().all(|byte| *byte == 0) {
            return Err(StorageError::CorruptIdentity);
        }
        let public = MontgomeryPoint::mul_base_clamped(private).to_bytes();
        if public.iter().all(|byte| *byte == 0) {
            return Err(StorageError::CorruptIdentity);
        }
        Ok(Identity { private, public })
    }

    pub fn load_guard(&self, now_ms: u64) -> Result<PairingGuard, StorageError> {
        match statat(&self.root, GUARD_FILE, AtFlags::SYMLINK_NOFOLLOW) {
            Err(Errno::NOENT) => Ok(PairingGuard::new(now_ms)),
            Err(_) => Err(StorageError::UnsafePath),
            Ok(_) => parse_guard(&read_authority(&self.root, GUARD_FILE)?),
        }
    }

    pub fn persist_guard(&self, guard: &PairingGuard) -> Result<(), StorageError> {
        let bytes = serde_json::to_vec(&json!({
            "schema_version": 1,
            "mismatch_count": guard.mismatch_count,
            "cooldown_until_wall_ms": guard.cooldown_until_wall_ms,
            "updated_wall_ms": guard.updated_wall_ms,
        }))
        .map_err(|_| StorageError::Io)?;
        self.atomic_write_json(&self.root, GUARD_FILE, bytes)
    }

    pub fn load_peers(&self) -> Result<Vec<PeerRecord>, StorageError> {
        let listing = openat(
            &self.peers,
            ".",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| StorageError::Io)?;
        let mut buffer = [MaybeUninit::<u8>::uninit(); 8_192];
        let mut names = Vec::new();
        let mut directory = RawDir::new(listing, &mut buffer);
        while let Some(entry) = directory.next() {
            let entry = entry.map_err(|_| StorageError::Io)?;
            let name = entry.file_name().to_bytes();
            if matches!(name, b"." | b"..") || name.starts_with(b".tmp-") {
                continue;
            }
            let name = std::str::from_utf8(name).map_err(|_| StorageError::CorruptPeer)?;
            if !name.ends_with(".json") || name.len() != 64 + 5 {
                return Err(StorageError::CorruptPeer);
            }
            names.push(name.to_owned());
            if names.len() > MAX_PEERS {
                return Err(StorageError::TooManyPeers);
            }
        }
        names.sort();
        names
            .into_iter()
            .map(|name| {
                let record = parse_peer(&read_authority(&self.peers, &name)?)?;
                if name != format!("{}.json", record.peer_id) {
                    return Err(StorageError::CorruptPeer);
                }
                Ok(record)
            })
            .collect()
    }

    pub fn commit_peer(&self, record: &PeerRecord) -> Result<(), StorageError> {
        validate_peer(record)?;
        let mut bytes = serde_json::to_vec(&json!({
            "schema_version": record.schema_version,
            "peer_id": record.peer_id,
            "static_public_key_b64": base64_encode(&record.static_public_key),
            "alias": record.alias,
            "paired_wall_ms": record.paired_wall_ms,
            "last_seen_wall_ms": record.last_seen_wall_ms,
            "core_version": record.core_version,
            "pbmux_version": record.pbmux_version,
        }))
        .map_err(|_| StorageError::Io)?;
        bytes.push(b'\n');
        self.atomic_write(&self.peers, &format!("{}.json", record.peer_id), &bytes)
    }

    fn atomic_write_json(
        &self,
        directory: &OwnedFd,
        name: &str,
        mut bytes: Vec<u8>,
    ) -> Result<(), StorageError> {
        bytes.push(b'\n');
        self.atomic_write(directory, name, &bytes)
    }

    fn atomic_write(
        &self,
        directory: &OwnedFd,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), StorageError> {
        if name.contains('/') || name.starts_with('.') {
            return Err(StorageError::UnsafePath);
        }
        let temp = format!(
            ".tmp-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let fd = openat(
            directory,
            &temp,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|_| StorageError::Io)?;
        let write_result = (|| {
            let mut file = File::from(fd);
            file.write_all(bytes).map_err(|_| StorageError::Io)?;
            file.sync_all().map_err(|_| StorageError::Io)?;
            let stat = fstat(&file).map_err(|_| StorageError::Io)?;
            if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
                || permission_bits(stat.st_mode) != AUTHORITY_MODE
            {
                return Err(StorageError::UnsafePath);
            }
            renameat(directory, &temp, directory, name).map_err(|_| StorageError::Io)?;
            fsync(directory).map_err(|_| StorageError::Io)
        })();
        if write_result.is_err() {
            let _ = unlinkat(directory, &temp, AtFlags::empty());
        }
        write_result
    }
}

pub fn wall_clock_ms() -> Result<u64, StorageError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StorageError::Io)?;
    u64::try_from(elapsed.as_millis()).map_err(|_| StorageError::Io)
}

fn canonical_host_root() -> Result<PathBuf, StorageError> {
    let base = if let Some(state) = env::var_os("XDG_STATE_HOME") {
        if state.is_empty() {
            return Err(StorageError::StateRootUnavailable);
        }
        PathBuf::from(state)
    } else {
        let home = env::var_os("HOME").ok_or(StorageError::StateRootUnavailable)?;
        if home.is_empty() {
            return Err(StorageError::StateRootUnavailable);
        }
        PathBuf::from(home).join(".local/state")
    };
    if !base.is_absolute() {
        return Err(StorageError::StateRootUnavailable);
    }
    Ok(base.join("phoneboost"))
}

fn create_secure_directory(path: &Path) -> Result<(), StorageError> {
    if path.exists() {
        return Ok(());
    }
    let parent = path.parent().ok_or(StorageError::UnsafePath)?;
    fs::create_dir_all(parent).map_err(|_| StorageError::Io)?;
    let mut builder = fs::DirBuilder::new();
    builder.mode(DIRECTORY_MODE);
    match builder.create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(_) => Err(StorageError::Io),
    }
}

fn reject_symlink_components(path: &Path) -> Result<(), StorageError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push(Path::new("/")),
            Component::Normal(name) => current.push(name),
            _ => return Err(StorageError::UnsafePath),
        }
        let metadata = fs::symlink_metadata(&current).map_err(|_| StorageError::UnsafePath)?;
        if metadata.file_type().is_symlink() {
            return Err(StorageError::UnsafePath);
        }
    }
    Ok(())
}

fn validate_directory(fd: &OwnedFd) -> Result<(), StorageError> {
    let stat = fstat(fd).map_err(|_| StorageError::UnsafePath)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != getuid().as_raw()
        || permission_bits(stat.st_mode) != DIRECTORY_MODE
    {
        return Err(StorageError::UnsafePath);
    }
    Ok(())
}

fn read_authority(directory: &OwnedFd, name: &str) -> Result<Vec<u8>, StorageError> {
    let fd = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| StorageError::UnsafePath)?;
    let stat = fstat(&fd).map_err(|_| StorageError::UnsafePath)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_uid != getuid().as_raw()
        || permission_bits(stat.st_mode) != AUTHORITY_MODE
        || stat.st_size < 0
        || stat.st_size as usize > MAX_AUTHORITY_BYTES
    {
        return Err(StorageError::UnsafePath);
    }
    let mut bytes = Vec::with_capacity(stat.st_size as usize);
    File::from(fd)
        .take(MAX_AUTHORITY_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| StorageError::Io)?;
    if bytes.len() > MAX_AUTHORITY_BYTES {
        return Err(StorageError::Io);
    }
    Ok(bytes)
}

fn parse_guard(bytes: &[u8]) -> Result<PairingGuard, StorageError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| StorageError::CorruptGuard)?;
    let object = value.as_object().ok_or(StorageError::CorruptGuard)?;
    if object.len() != 4 || object.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(StorageError::CorruptGuard);
    }
    let mismatch_count = object
        .get("mismatch_count")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or(StorageError::CorruptGuard)?;
    let cooldown_until_wall_ms = match object.get("cooldown_until_wall_ms") {
        Some(Value::Null) => None,
        Some(value) => Some(value.as_u64().ok_or(StorageError::CorruptGuard)?),
        None => return Err(StorageError::CorruptGuard),
    };
    let updated_wall_ms = object
        .get("updated_wall_ms")
        .and_then(Value::as_u64)
        .ok_or(StorageError::CorruptGuard)?;
    Ok(PairingGuard {
        mismatch_count,
        cooldown_until_wall_ms,
        updated_wall_ms,
    })
}

fn parse_peer(bytes: &[u8]) -> Result<PeerRecord, StorageError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| StorageError::CorruptPeer)?;
    let object = value.as_object().ok_or(StorageError::CorruptPeer)?;
    let expected = [
        "schema_version",
        "peer_id",
        "static_public_key_b64",
        "alias",
        "paired_wall_ms",
        "last_seen_wall_ms",
        "core_version",
        "pbmux_version",
    ];
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err(StorageError::CorruptPeer);
    }
    let key = base64_decode_32(required_string(object, "static_public_key_b64")?)?;
    let record = PeerRecord {
        schema_version: required_u64(object, "schema_version")?,
        peer_id: required_string(object, "peer_id")?.to_owned(),
        static_public_key: key,
        alias: required_string(object, "alias")?.to_owned(),
        paired_wall_ms: required_u64(object, "paired_wall_ms")?,
        last_seen_wall_ms: required_u64(object, "last_seen_wall_ms")?,
        core_version: required_u64(object, "core_version")?,
        pbmux_version: required_u64(object, "pbmux_version")?,
    };
    validate_peer(&record)?;
    Ok(record)
}

fn validate_peer(record: &PeerRecord) -> Result<(), StorageError> {
    if record.schema_version != 1
        || record.core_version != 1
        || record.pbmux_version != 1
        || record.peer_id != peer_id(&record.static_public_key)
        || record.alias.is_empty()
        || record.alias.len() > 128
        || record.static_public_key.iter().all(|byte| *byte == 0)
    {
        return Err(StorageError::CorruptPeer);
    }
    Ok(())
}

fn required_string<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a str, StorageError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(StorageError::CorruptPeer)
}

fn required_u64(object: &serde_json::Map<String, Value>, key: &str) -> Result<u64, StorageError> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .ok_or(StorageError::CorruptPeer)
}

fn peer_id(public: &[u8; 32]) -> String {
    hex_encode(&Sha256::digest(public))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        output.push(ALPHABET[((word >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((word >> 12) & 0x3f) as usize] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[((word >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[(word & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn base64_decode_32(value: &str) -> Result<[u8; 32], StorageError> {
    if value.len() != 44 || !value.ends_with('=') {
        return Err(StorageError::CorruptPeer);
    }
    let mut output = Vec::with_capacity(32);
    for chunk in value.as_bytes().as_chunks::<4>().0 {
        let a = base64_value(chunk[0])?;
        let b = base64_value(chunk[1])?;
        let c = if chunk[2] == b'=' {
            0
        } else {
            base64_value(chunk[2])?
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            base64_value(chunk[3])?
        };
        let word = (u32::from(a) << 18) | (u32::from(b) << 12) | (u32::from(c) << 6) | u32::from(d);
        output.push((word >> 16) as u8);
        if chunk[2] != b'=' {
            output.push((word >> 8) as u8);
        }
        if chunk[3] != b'=' {
            output.push(word as u8);
        }
    }
    output.try_into().map_err(|_| StorageError::CorruptPeer)
}

fn base64_value(byte: u8) -> Result<u8, StorageError> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(StorageError::CorruptPeer),
    }
}

fn permission_bits(mode: u32) -> u32 {
    mode & 0o777
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::fs::symlink;

    fn temporary_store() -> (PathBuf, StateStore) {
        let path = env::temp_dir().join(format!(
            "phoneboost-c05-store-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = fs::DirBuilder::new();
        builder
            .mode(DIRECTORY_MODE)
            .create(&path)
            .expect("temp root");
        let fd = open(
            &path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .expect("open root");
        (path, StateStore::from_directory_fd(fd).expect("store"))
    }

    #[test]
    fn identity_is_generated_once_and_corruption_fails_closed() {
        let (path, store) = temporary_store();
        let first = store.load_or_create_identity().expect("identity");
        let second = store.load_or_create_identity().expect("identity reload");
        assert_eq!(first, second);
        assert_eq!(
            fs::metadata(path.join(IDENTITY_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::write(path.join(IDENTITY_FILE), b"corrupt").expect("corrupt fixture");
        fs::set_permissions(path.join(IDENTITY_FILE), fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            store.load_or_create_identity(),
            Err(StorageError::CorruptIdentity)
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn peer_commit_is_atomic_strict_and_no_intermediate_state_exists() {
        let (path, store) = temporary_store();
        let record = PeerRecord::new([7; 32], "peer", 42);
        store.commit_peer(&record).expect("commit");
        assert_eq!(store.load_peers().unwrap(), vec![record]);
        let entries: Vec<_> = fs::read_dir(path.join(PEERS_DIRECTORY))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries.len(), 1);
        assert!(
            !fs::read_to_string(path.join(PEERS_DIRECTORY).join(&entries[0]))
                .unwrap()
                .contains("SAS_PENDING")
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn stale_atomic_temp_is_ignored_and_corrupt_peer_fails_closed() {
        let (path, store) = temporary_store();
        fs::write(path.join(PEERS_DIRECTORY).join(".tmp-stale"), b"partial").unwrap();
        assert!(store.load_peers().unwrap().is_empty());
        let bad = path
            .join(PEERS_DIRECTORY)
            .join(format!("{}.json", "0".repeat(64)));
        fs::write(&bad, b"{}\n").unwrap();
        fs::set_permissions(&bad, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(store.load_peers(), Err(StorageError::CorruptPeer));
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn authority_symlink_is_rejected_without_touching_target() {
        let (path, store) = temporary_store();
        let target = path.join("target");
        fs::write(&target, [9; 32]).unwrap();
        symlink(&target, path.join(IDENTITY_FILE)).unwrap();
        assert!(store.load_or_create_identity().is_err());
        assert_eq!(fs::read(target).unwrap(), vec![9; 32]);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn guard_roundtrip_preserves_cooldown() {
        let (path, store) = temporary_store();
        let mut guard = PairingGuard::new(100);
        guard.record_mismatch(101);
        guard.record_mismatch(102);
        guard.record_mismatch(103);
        store.persist_guard(&guard).unwrap();
        assert_eq!(store.load_guard(200).unwrap(), guard);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn base64_roundtrip_is_canonical() {
        let key = [0x5a; 32];
        assert_eq!(base64_decode_32(&base64_encode(&key)).unwrap(), key);
    }
}
