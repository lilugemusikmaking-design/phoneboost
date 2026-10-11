//! C13 private, atomic performance-profile persistence.

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, fchmod, fstat, fsync, mkdirat, open, openat, renameat, statat,
    unlinkat,
};
use rustix::io::Errno;
use rustix::process::getuid;
use serde::{Deserialize, Serialize};

use crate::planner::{
    BatteryPolicy, PERFORMANCE_PROFILE_TTL, PerformanceEvidence, PerformanceProfile,
    PerformanceProfileKey, ThermalClass,
};

pub const MAXIMUM_PROFILE_BYTES: usize = 65_536;
const PROFILE_SCHEMA_VERSION: u16 = 1;
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const PHONEBOOST_DIRECTORY: &str = "phoneboost";
const PERFORMANCE_DIRECTORY: &str = "perf";
const VERSION_DIRECTORY: &str = "v1";
const BENCHMARK_CONTRACT: &str = "pb-bench-v1";
const LINK_MEASUREMENT_CONTRACT: &str = "pb-link-metrics-v1";
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum C13RequesterPath {
    NativeC12,
    PbCli,
    McpAdapter,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PersistedThermalClass {
    Nominal,
    Light,
    Moderate,
    Severe,
}

impl From<ThermalClass> for PersistedThermalClass {
    fn from(value: ThermalClass) -> Self {
        match value {
            ThermalClass::Nominal => Self::Nominal,
            ThermalClass::Light => Self::Light,
            ThermalClass::Moderate => Self::Moderate,
            ThermalClass::Severe => Self::Severe,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PersistedBatteryPolicy {
    ExternalPower,
    BatteryAllowed,
    PowerSave,
    RemoteDisallowed,
}

impl From<BatteryPolicy> for PersistedBatteryPolicy {
    fn from(value: BatteryPolicy) -> Self {
        match value {
            BatteryPolicy::ExternalPower => Self::ExternalPower,
            BatteryPolicy::BatteryAllowed => Self::BatteryAllowed,
            BatteryPolicy::PowerSave => Self::PowerSave,
            BatteryPolicy::RemoteDisallowed => Self::RemoteDisallowed,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct C13ProfileKeyV1 {
    pub peer_id_sha256_hex: String,
    pub worker_build_id: String,
    pub provider_id: String,
    pub operation_id: String,
    pub core_protocol_version: u16,
    pub provider_protocol_version: u16,
    pub transport_signature: String,
    pub workload_signature: String,
    thermal_class: PersistedThermalClass,
    battery_policy: PersistedBatteryPolicy,
}

impl C13ProfileKeyV1 {
    pub fn from_planner_key(key: &PerformanceProfileKey) -> Self {
        Self {
            peer_id_sha256_hex: lowercase_hex(key.peer_id.as_bytes()),
            worker_build_id: key.worker_build_id.clone(),
            provider_id: key.provider_id.clone(),
            operation_id: key.operation_id.clone(),
            core_protocol_version: key.core_protocol_version,
            provider_protocol_version: key.provider_protocol_version,
            transport_signature: key.transport_signature.clone(),
            workload_signature: key.workload_signature.clone(),
            thermal_class: key.thermal_class.into(),
            battery_policy: key.battery_policy.into(),
        }
    }

    fn validate(&self) -> bool {
        is_lowercase_hex_64(&self.peer_id_sha256_hex)
            && is_bounded_ascii_id(&self.worker_build_id)
            && is_bounded_ascii_id(&self.provider_id)
            && is_bounded_ascii_id(&self.operation_id)
            && matches!(
                self.transport_signature.as_str(),
                "pb-link-v1:wifi_lan"
                    | "pb-link-v1:usb_network"
                    | "pb-link-v1:ethernet_ip"
                    | "pb-link-v1:local_ip_unknown"
            )
            && is_workload_signature(&self.workload_signature)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct C13PerformanceEvidenceV1 {
    pub local_warmup_complete: bool,
    pub remote_warmup_complete: bool,
    pub measured_local_runs: u16,
    pub measured_remote_runs: u16,
    pub local_median_duration_ns: u64,
    pub remote_median_duration_ns: u64,
    pub remote_median_gain_basis_points: i32,
    pub bootstrap_95_lower_bound_basis_points: i32,
    pub sustained_check_duration_ms: u64,
    pub thermal_severe_observed: bool,
}

impl C13PerformanceEvidenceV1 {
    fn validate(self) -> bool {
        self.local_median_duration_ns > 0 && self.remote_median_duration_ns > 0
    }

    fn planner_evidence(self) -> PerformanceEvidence {
        PerformanceEvidence {
            local_warmup_complete: self.local_warmup_complete,
            remote_warmup_complete: self.remote_warmup_complete,
            measured_local_runs: self.measured_local_runs,
            measured_remote_runs: self.measured_remote_runs,
            remote_median_gain_basis_points: self.remote_median_gain_basis_points,
            bootstrap_95_lower_bound_basis_points: self.bootstrap_95_lower_bound_basis_points,
            sustained_check_duration: Duration::from_millis(self.sustained_check_duration_ms),
            thermal_severe_observed: self.thermal_severe_observed,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct C13BenchmarkProvenanceV1 {
    pub benchmark_contract: String,
    pub benchmark_build_id: String,
    pub benchmark_config_id: String,
    pub requester_path: C13RequesterPath,
    pub link_measurement_contract: String,
    pub median_rtt_ms: u64,
    pub median_tx_goodput_bytes_per_second: u64,
    pub median_rx_goodput_bytes_per_second: u64,
}

impl C13BenchmarkProvenanceV1 {
    pub fn new(
        benchmark_build_id: impl Into<String>,
        benchmark_config_id: impl Into<String>,
        requester_path: C13RequesterPath,
        median_rtt_ms: u64,
        median_tx_goodput_bytes_per_second: u64,
        median_rx_goodput_bytes_per_second: u64,
    ) -> Result<Self, PerformanceProfileStoreError> {
        let provenance = Self {
            benchmark_contract: BENCHMARK_CONTRACT.to_owned(),
            benchmark_build_id: benchmark_build_id.into(),
            benchmark_config_id: benchmark_config_id.into(),
            requester_path,
            link_measurement_contract: LINK_MEASUREMENT_CONTRACT.to_owned(),
            median_rtt_ms,
            median_tx_goodput_bytes_per_second,
            median_rx_goodput_bytes_per_second,
        };
        provenance
            .validate()
            .then_some(provenance)
            .ok_or(PerformanceProfileStoreError::InvalidProfile)
    }

    fn validate(&self) -> bool {
        self.benchmark_contract == BENCHMARK_CONTRACT
            && is_bounded_ascii_id(&self.benchmark_build_id)
            && is_bounded_ascii_id(&self.benchmark_config_id)
            && self.link_measurement_contract == LINK_MEASUREMENT_CONTRACT
            && self.median_rtt_ms > 0
            && self.median_tx_goodput_bytes_per_second > 0
            && self.median_rx_goodput_bytes_per_second > 0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct C13PerformanceProfileV1 {
    pub schema_version: u16,
    pub created_wall_ms: u64,
    pub expires_wall_ms: u64,
    pub key: C13ProfileKeyV1,
    pub evidence: C13PerformanceEvidenceV1,
    pub provenance: C13BenchmarkProvenanceV1,
}

impl C13PerformanceProfileV1 {
    pub fn new(
        key: &PerformanceProfileKey,
        created_wall_ms: u64,
        evidence: C13PerformanceEvidenceV1,
        provenance: C13BenchmarkProvenanceV1,
    ) -> Result<Self, PerformanceProfileStoreError> {
        let ttl_ms = u64::try_from(PERFORMANCE_PROFILE_TTL.as_millis())
            .map_err(|_| PerformanceProfileStoreError::InvalidProfile)?;
        let expires_wall_ms = created_wall_ms
            .checked_add(ttl_ms)
            .ok_or(PerformanceProfileStoreError::InvalidProfile)?;
        let profile = Self {
            schema_version: PROFILE_SCHEMA_VERSION,
            created_wall_ms,
            expires_wall_ms,
            key: C13ProfileKeyV1::from_planner_key(key),
            evidence,
            provenance,
        };
        profile.validate()?;
        Ok(profile)
    }

    pub fn profile_id(&self) -> Result<String, PerformanceProfileStoreError> {
        self.validate()?;
        profile_id_for_key(&self.key)
    }

    fn validate(&self) -> Result<(), PerformanceProfileStoreError> {
        let ttl_ms = u64::try_from(PERFORMANCE_PROFILE_TTL.as_millis())
            .map_err(|_| PerformanceProfileStoreError::InvalidProfile)?;
        if self.schema_version != PROFILE_SCHEMA_VERSION
            || self.created_wall_ms.checked_add(ttl_ms) != Some(self.expires_wall_ms)
            || !self.key.validate()
            || !self.evidence.validate()
            || !self.provenance.validate()
        {
            return Err(PerformanceProfileStoreError::InvalidProfile);
        }
        Ok(())
    }

    fn planner_profile(&self, current_key: &PerformanceProfileKey) -> PerformanceProfile {
        PerformanceProfile {
            schema_version: self.schema_version,
            key: current_key.clone(),
            created_wall_ms: self.created_wall_ms,
            evidence: self.evidence.planner_evidence(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileStoreInvalidationReason {
    UnsafeObject,
    Oversized,
    Malformed,
    SchemaUnsupported,
    KeyChanged,
    ClockMovedBackwards,
    TtlExpired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PerformanceProfileLoadOutcome {
    Missing,
    Valid(Box<PerformanceProfile>),
    Invalid(ProfileStoreInvalidationReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PerformanceProfileStoreError {
    StateRootUnavailable,
    UnsafeStateRoot,
    UnsafeDirectory,
    UnsafeProfileObject,
    InvalidProfile,
    ProfileTooLarge,
    Io,
    DurabilityUnknown,
}

impl fmt::Display for PerformanceProfileStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::StateRootUnavailable => "performance profile state root unavailable",
            Self::UnsafeStateRoot => "performance profile state root is unsafe",
            Self::UnsafeDirectory => "performance profile directory is unsafe",
            Self::UnsafeProfileObject => "performance profile object is unsafe",
            Self::InvalidProfile => "performance profile is invalid",
            Self::ProfileTooLarge => "performance profile exceeds its size limit",
            Self::Io => "performance profile I/O failed",
            Self::DurabilityUnknown => "performance profile durability is unknown",
        })
    }
}

impl std::error::Error for PerformanceProfileStoreError {}

pub struct PerformanceProfileStore {
    directory: OwnedFd,
}

impl fmt::Debug for PerformanceProfileStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PerformanceProfileStore")
            .finish_non_exhaustive()
    }
}

impl PerformanceProfileStore {
    pub fn from_environment() -> Result<Self, PerformanceProfileStoreError> {
        let state_home =
            state_home_from_values(env::var_os("XDG_STATE_HOME"), env::var_os("HOME"))?;
        Self::from_state_home(&state_home)
    }

    pub fn from_state_home(state_home: &Path) -> Result<Self, PerformanceProfileStoreError> {
        if !state_home.is_absolute() {
            return Err(PerformanceProfileStoreError::UnsafeStateRoot);
        }
        fs::create_dir_all(state_home)
            .map_err(|_| PerformanceProfileStoreError::StateRootUnavailable)?;
        reject_symlink_components(state_home)?;
        let state_root = open(
            state_home,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| PerformanceProfileStoreError::UnsafeStateRoot)?;
        validate_owned_directory(&state_root, None)
            .map_err(|_| PerformanceProfileStoreError::UnsafeStateRoot)?;
        let phoneboost = ensure_private_directory(&state_root, PHONEBOOST_DIRECTORY)?;
        let performance = ensure_private_directory(&phoneboost, PERFORMANCE_DIRECTORY)?;
        let directory = ensure_private_directory(&performance, VERSION_DIRECTORY)?;
        Ok(Self { directory })
    }

    pub fn store(
        &self,
        profile: &C13PerformanceProfileV1,
    ) -> Result<String, PerformanceProfileStoreError> {
        profile.validate()?;
        let profile_id = profile.profile_id()?;
        let final_name = format!("{profile_id}.json");
        validate_existing_destination(&self.directory, &final_name)?;
        let bytes = serde_json::to_vec(profile)
            .map_err(|_| PerformanceProfileStoreError::InvalidProfile)?;
        if bytes.len() > MAXIMUM_PROFILE_BYTES {
            return Err(PerformanceProfileStoreError::ProfileTooLarge);
        }

        let temporary_name = self.create_temporary_name();
        let temporary = openat(
            &self.directory,
            temporary_name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|_| PerformanceProfileStoreError::Io)?;
        if validate_owned_regular_file(&temporary).is_err() {
            let _ = unlinkat(&self.directory, temporary_name.as_str(), AtFlags::empty());
            return Err(PerformanceProfileStoreError::UnsafeProfileObject);
        }

        let write_result = (|| {
            let mut file = File::from(temporary);
            file.write_all(&bytes)
                .map_err(|_| PerformanceProfileStoreError::Io)?;
            file.flush().map_err(|_| PerformanceProfileStoreError::Io)?;
            fsync(&file).map_err(|_| PerformanceProfileStoreError::Io)?;
            renameat(
                &self.directory,
                temporary_name.as_str(),
                &self.directory,
                final_name.as_str(),
            )
            .map_err(|_| PerformanceProfileStoreError::Io)?;
            fsync(&self.directory).map_err(|_| PerformanceProfileStoreError::DurabilityUnknown)
        })();
        if write_result.is_err() {
            let _ = unlinkat(&self.directory, temporary_name.as_str(), AtFlags::empty());
        }
        write_result.map(|()| profile_id)
    }

    pub fn load(
        &self,
        current_key: &PerformanceProfileKey,
        now_wall_ms: u64,
    ) -> PerformanceProfileLoadOutcome {
        let persisted_key = C13ProfileKeyV1::from_planner_key(current_key);
        if !persisted_key.validate() {
            return PerformanceProfileLoadOutcome::Invalid(
                ProfileStoreInvalidationReason::KeyChanged,
            );
        }
        let profile_id = match profile_id_for_key(&persisted_key) {
            Ok(profile_id) => profile_id,
            Err(_) => {
                return PerformanceProfileLoadOutcome::Invalid(
                    ProfileStoreInvalidationReason::Malformed,
                );
            }
        };
        let name = format!("{profile_id}.json");
        let descriptor = match openat(
            &self.directory,
            name.as_str(),
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        ) {
            Ok(descriptor) => descriptor,
            Err(Errno::NOENT) => return PerformanceProfileLoadOutcome::Missing,
            Err(_) => {
                return PerformanceProfileLoadOutcome::Invalid(
                    ProfileStoreInvalidationReason::UnsafeObject,
                );
            }
        };
        let before = match validate_owned_regular_file(&descriptor) {
            Ok(stat) => stat,
            Err(_) => {
                return PerformanceProfileLoadOutcome::Invalid(
                    ProfileStoreInvalidationReason::UnsafeObject,
                );
            }
        };
        let Ok(size) = usize::try_from(before.st_size) else {
            return PerformanceProfileLoadOutcome::Invalid(
                ProfileStoreInvalidationReason::Oversized,
            );
        };
        if size > MAXIMUM_PROFILE_BYTES {
            return PerformanceProfileLoadOutcome::Invalid(
                ProfileStoreInvalidationReason::Oversized,
            );
        }

        let mut bytes = Vec::with_capacity(size);
        let mut file = File::from(descriptor);
        if Read::by_ref(&mut file)
            .take((MAXIMUM_PROFILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .is_err()
        {
            return PerformanceProfileLoadOutcome::Invalid(
                ProfileStoreInvalidationReason::UnsafeObject,
            );
        }
        if bytes.len() > MAXIMUM_PROFILE_BYTES {
            return PerformanceProfileLoadOutcome::Invalid(
                ProfileStoreInvalidationReason::Oversized,
            );
        }
        let after = match validate_owned_regular_file(&file) {
            Ok(stat) => stat,
            Err(_) => {
                return PerformanceProfileLoadOutcome::Invalid(
                    ProfileStoreInvalidationReason::UnsafeObject,
                );
            }
        };
        if before.st_dev != after.st_dev
            || before.st_ino != after.st_ino
            || before.st_size != after.st_size
            || before.st_mtime != after.st_mtime
            || before.st_mtime_nsec != after.st_mtime_nsec
        {
            return PerformanceProfileLoadOutcome::Invalid(
                ProfileStoreInvalidationReason::UnsafeObject,
            );
        }

        let profile: C13PerformanceProfileV1 = match serde_json::from_slice(&bytes) {
            Ok(profile) => profile,
            Err(_) => {
                return PerformanceProfileLoadOutcome::Invalid(
                    ProfileStoreInvalidationReason::Malformed,
                );
            }
        };
        if profile.schema_version != PROFILE_SCHEMA_VERSION {
            return PerformanceProfileLoadOutcome::Invalid(
                ProfileStoreInvalidationReason::SchemaUnsupported,
            );
        }
        if profile.validate().is_err() {
            return PerformanceProfileLoadOutcome::Invalid(
                ProfileStoreInvalidationReason::Malformed,
            );
        }
        if profile.key != persisted_key {
            return PerformanceProfileLoadOutcome::Invalid(
                ProfileStoreInvalidationReason::KeyChanged,
            );
        }
        if now_wall_ms < profile.created_wall_ms {
            return PerformanceProfileLoadOutcome::Invalid(
                ProfileStoreInvalidationReason::ClockMovedBackwards,
            );
        }
        if now_wall_ms >= profile.expires_wall_ms {
            return PerformanceProfileLoadOutcome::Invalid(
                ProfileStoreInvalidationReason::TtlExpired,
            );
        }
        PerformanceProfileLoadOutcome::Valid(Box::new(profile.planner_profile(current_key)))
    }

    fn create_temporary_name(&self) -> String {
        let sequence = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        format!(".profile-{}-{sequence:016x}.tmp", std::process::id())
    }
}

fn state_home_from_values(
    xdg_state_home: Option<OsString>,
    home: Option<OsString>,
) -> Result<PathBuf, PerformanceProfileStoreError> {
    if let Some(value) = xdg_state_home {
        if value.is_empty() {
            return Err(PerformanceProfileStoreError::UnsafeStateRoot);
        }
        let path = PathBuf::from(value);
        return path
            .is_absolute()
            .then_some(path)
            .ok_or(PerformanceProfileStoreError::UnsafeStateRoot);
    }
    let home = home.ok_or(PerformanceProfileStoreError::StateRootUnavailable)?;
    if home.is_empty() {
        return Err(PerformanceProfileStoreError::StateRootUnavailable);
    }
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return Err(PerformanceProfileStoreError::UnsafeStateRoot);
    }
    Ok(home.join(".local/state"))
}

fn reject_symlink_components(path: &Path) -> Result<(), PerformanceProfileStoreError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push(Path::new("/")),
            Component::Normal(name) => current.push(name),
            _ => return Err(PerformanceProfileStoreError::UnsafeStateRoot),
        }
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| PerformanceProfileStoreError::UnsafeStateRoot)?;
        if metadata.file_type().is_symlink() {
            return Err(PerformanceProfileStoreError::UnsafeStateRoot);
        }
    }
    Ok(())
}

fn ensure_private_directory<Fd: AsFd>(
    parent: Fd,
    name: &str,
) -> Result<OwnedFd, PerformanceProfileStoreError> {
    let created = match mkdirat(&parent, name, Mode::RWXU) {
        Ok(()) => true,
        Err(Errno::EXIST) => false,
        Err(_) => return Err(PerformanceProfileStoreError::UnsafeDirectory),
    };
    let directory = openat(
        &parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| PerformanceProfileStoreError::UnsafeDirectory)?;
    if created {
        fchmod(&directory, Mode::RWXU)
            .map_err(|_| PerformanceProfileStoreError::UnsafeDirectory)?;
    }
    validate_owned_directory(&directory, Some(PRIVATE_DIRECTORY_MODE))
        .map_err(|_| PerformanceProfileStoreError::UnsafeDirectory)?;
    Ok(directory)
}

fn validate_owned_directory<Fd: AsFd>(fd: Fd, exact_mode: Option<u32>) -> Result<(), ()> {
    let stat = fstat(fd).map_err(|_| ())?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != getuid().as_raw()
        || exact_mode.is_some_and(|mode| permission_bits(&stat) != mode)
    {
        return Err(());
    }
    Ok(())
}

fn validate_owned_regular_file<Fd: AsFd>(fd: Fd) -> Result<rustix::fs::Stat, ()> {
    let stat = fstat(fd).map_err(|_| ())?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_uid != getuid().as_raw()
        || permission_bits(&stat) != PRIVATE_FILE_MODE
    {
        return Err(());
    }
    Ok(stat)
}

fn validate_existing_destination<Fd: AsFd>(
    directory: Fd,
    name: &str,
) -> Result<(), PerformanceProfileStoreError> {
    match statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat)
            if FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile
                && stat.st_uid == getuid().as_raw()
                && permission_bits(&stat) == PRIVATE_FILE_MODE =>
        {
            Ok(())
        }
        Ok(_) => Err(PerformanceProfileStoreError::UnsafeProfileObject),
        Err(Errno::NOENT) => Ok(()),
        Err(_) => Err(PerformanceProfileStoreError::UnsafeProfileObject),
    }
}

fn profile_id_for_key(key: &C13ProfileKeyV1) -> Result<String, PerformanceProfileStoreError> {
    let canonical =
        serde_json::to_vec(key).map_err(|_| PerformanceProfileStoreError::InvalidProfile)?;
    Ok(lowercase_hex(blake3::hash(&canonical).as_bytes()))
}

const fn permission_bits(stat: &rustix::fs::Stat) -> u32 {
    stat.st_mode & 0o7777
}

fn is_bounded_ascii_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b':' | b'/' | b'-')
        })
}

fn is_workload_signature(value: &str) -> bool {
    const PREFIX: &str = "pb-workload-v1:";
    let bytes = value.as_bytes();
    (16..=256).contains(&bytes.len())
        && value.strip_prefix(PREFIX).is_some_and(|suffix| {
            !suffix.is_empty()
                && suffix.as_bytes().iter().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b':' | b'-')
                })
        })
}

fn is_lowercase_hex_64(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
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

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

    use super::*;

    static NEXT_SANDBOX: AtomicU64 = AtomicU64::new(0);
    const CREATED: u64 = 1_791_744_000_000;

    struct Sandbox {
        root: PathBuf,
    }

    impl Sandbox {
        fn new() -> Self {
            let id = NEXT_SANDBOX.fetch_add(1, Ordering::Relaxed);
            let root = env::temp_dir().join(format!("phoneboost-c13-{}-{id}", std::process::id()));
            fs::create_dir(&root).expect("sandbox root");
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("sandbox mode");
            Self { root }
        }

        fn profile_directory(&self) -> PathBuf {
            self.root.join("phoneboost/perf/v1")
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn planner_key() -> PerformanceProfileKey {
        PerformanceProfileKey {
            peer_id: pb_types::PeerId::from_static_public_key(&[7; 32]),
            worker_build_id: "android-worker-0.1.0+fixture".to_owned(),
            provider_id: "pb.native".to_owned(),
            operation_id: "pb.native.blake3/1".to_owned(),
            core_protocol_version: 1,
            provider_protocol_version: 1,
            transport_signature: "pb-link-v1:usb_network".to_owned(),
            workload_signature: "pb-workload-v1:blake3:64m:fixture".to_owned(),
            thermal_class: ThermalClass::Nominal,
            battery_policy: BatteryPolicy::ExternalPower,
        }
    }

    fn evidence() -> C13PerformanceEvidenceV1 {
        C13PerformanceEvidenceV1 {
            local_warmup_complete: true,
            remote_warmup_complete: true,
            measured_local_runs: 15,
            measured_remote_runs: 15,
            local_median_duration_ns: 1_000_000_000,
            remote_median_duration_ns: 900_000_000,
            remote_median_gain_basis_points: 1_000,
            bootstrap_95_lower_bound_basis_points: 500,
            sustained_check_duration_ms: 300_000,
            thermal_severe_observed: false,
        }
    }

    fn provenance() -> C13BenchmarkProvenanceV1 {
        C13BenchmarkProvenanceV1::new(
            "phoneboost-bench-0.1.0+fixture",
            "linux-native-c12-usb-v1",
            C13RequesterPath::NativeC12,
            8,
            25_000_000,
            30_000_000,
        )
        .expect("valid provenance")
    }

    fn profile() -> C13PerformanceProfileV1 {
        C13PerformanceProfileV1::new(&planner_key(), CREATED, evidence(), provenance())
            .expect("valid profile")
    }

    #[test]
    fn c13_fixture_matches_typed_profile_and_canonical_key() {
        let fixture: C13PerformanceProfileV1 = serde_json::from_slice(include_bytes!(
            "../../../docs/agentic-integration/fixtures/perf-profile-v1-valid.json"
        ))
        .expect("locked fixture");
        assert_eq!(fixture, profile());
        assert!(fixture.validate().is_ok());
        let profile_id = fixture.profile_id().expect("canonical profile id");
        assert!(is_lowercase_hex_64(&profile_id));
        assert!(!serde_json::to_string(&fixture).unwrap().contains("192.0.2"));
    }

    #[test]
    fn c13_private_atomic_round_trip_and_exact_expiry() {
        let sandbox = Sandbox::new();
        let store = PerformanceProfileStore::from_state_home(&sandbox.root).expect("store");
        let profile = profile();
        let profile_id = store.store(&profile).expect("atomic store");
        let path = sandbox
            .profile_directory()
            .join(format!("{profile_id}.json"));
        let metadata = fs::symlink_metadata(&path).expect("profile metadata");
        assert!(metadata.is_file());
        assert_eq!(metadata.mode() & 0o7777, PRIVATE_FILE_MODE);
        for directory in [
            sandbox.root.join("phoneboost"),
            sandbox.root.join("phoneboost/perf"),
            sandbox.profile_directory(),
        ] {
            assert_eq!(
                fs::symlink_metadata(directory).unwrap().mode() & 0o7777,
                PRIVATE_DIRECTORY_MODE
            );
        }
        assert_eq!(
            fs::read_dir(sandbox.profile_directory()).unwrap().count(),
            1
        );

        let replacement =
            C13PerformanceProfileV1::new(&planner_key(), CREATED + 1_000, evidence(), provenance())
                .expect("valid replacement");
        assert_eq!(store.store(&replacement).unwrap(), profile_id);
        assert_eq!(
            fs::read_dir(sandbox.profile_directory()).unwrap().count(),
            1
        );

        assert!(matches!(
            store.load(&planner_key(), replacement.expires_wall_ms - 1),
            PerformanceProfileLoadOutcome::Valid(_)
        ));
        assert_eq!(
            store.load(&planner_key(), replacement.expires_wall_ms),
            PerformanceProfileLoadOutcome::Invalid(ProfileStoreInvalidationReason::TtlExpired)
        );
        assert_eq!(
            store.load(&planner_key(), replacement.created_wall_ms - 1),
            PerformanceProfileLoadOutcome::Invalid(
                ProfileStoreInvalidationReason::ClockMovedBackwards
            )
        );
    }

    #[test]
    fn c13_complete_key_changes_never_reuse_a_profile() {
        let sandbox = Sandbox::new();
        let store = PerformanceProfileStore::from_state_home(&sandbox.root).expect("store");
        store.store(&profile()).expect("store profile");
        let original = planner_key();
        let original_id =
            profile_id_for_key(&C13ProfileKeyV1::from_planner_key(&original)).unwrap();
        let mut changed_keys = Vec::new();
        let mut changed = original.clone();
        changed.peer_id = pb_types::PeerId::from([8; 32]);
        changed_keys.push(changed);
        let mut changed = original.clone();
        changed.worker_build_id.push_str("-next");
        changed_keys.push(changed);
        let mut changed = original.clone();
        changed.provider_id.push_str(".next");
        changed_keys.push(changed);
        let mut changed = original.clone();
        changed.operation_id.push_str(".next");
        changed_keys.push(changed);
        let mut changed = original.clone();
        changed.core_protocol_version += 1;
        changed_keys.push(changed);
        let mut changed = original.clone();
        changed.provider_protocol_version += 1;
        changed_keys.push(changed);
        let mut changed = original.clone();
        changed.transport_signature = "pb-link-v1:wifi_lan".to_owned();
        changed_keys.push(changed);
        let mut changed = original.clone();
        changed.workload_signature.push_str(":next");
        changed_keys.push(changed);
        let mut changed = original.clone();
        changed.thermal_class = ThermalClass::Light;
        changed_keys.push(changed);
        let mut changed = original.clone();
        changed.battery_policy = BatteryPolicy::BatteryAllowed;
        changed_keys.push(changed);

        let mut changed_ids = Vec::new();
        for changed in changed_keys {
            assert_eq!(
                store.load(&changed, CREATED),
                PerformanceProfileLoadOutcome::Missing
            );
            let changed_id =
                profile_id_for_key(&C13ProfileKeyV1::from_planner_key(&changed)).unwrap();
            assert_ne!(changed_id, original_id);
            assert!(!changed_ids.contains(&changed_id));
            changed_ids.push(changed_id);
        }
    }

    #[test]
    fn c13_parser_rejects_unknown_duplicate_and_invalid_contract_fields() {
        let valid = serde_json::to_string(&profile()).unwrap();
        let unknown = valid.replacen(
            "\"schema_version\":1",
            "\"schema_version\":1,\"unexpected\":true",
            1,
        );
        assert!(serde_json::from_str::<C13PerformanceProfileV1>(&unknown).is_err());
        let duplicate = valid.replacen(
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
            1,
        );
        assert!(serde_json::from_str::<C13PerformanceProfileV1>(&duplicate).is_err());
        let mut invalid = profile();
        invalid.provenance.benchmark_contract = "other".to_owned();
        assert_eq!(
            invalid.validate(),
            Err(PerformanceProfileStoreError::InvalidProfile)
        );
        invalid = profile();
        invalid.expires_wall_ms += 1;
        assert_eq!(
            invalid.validate(),
            Err(PerformanceProfileStoreError::InvalidProfile)
        );
    }

    #[test]
    fn c13_load_classifies_unsupported_and_malformed_without_payload_echo() {
        let sandbox = Sandbox::new();
        let store = PerformanceProfileStore::from_state_home(&sandbox.root).expect("store");
        let profile = profile();
        let path = sandbox
            .profile_directory()
            .join(format!("{}.json", profile.profile_id().unwrap()));

        let mut unsupported = profile.clone();
        unsupported.schema_version = 2;
        fs::write(&path, serde_json::to_vec(&unsupported).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(PRIVATE_FILE_MODE)).unwrap();
        assert_eq!(
            store.load(&planner_key(), CREATED),
            PerformanceProfileLoadOutcome::Invalid(
                ProfileStoreInvalidationReason::SchemaUnsupported
            )
        );

        let duplicate = serde_json::to_string(&profile).unwrap().replacen(
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
            1,
        );
        fs::write(&path, duplicate).unwrap();
        assert_eq!(
            store.load(&planner_key(), CREATED),
            PerformanceProfileLoadOutcome::Invalid(ProfileStoreInvalidationReason::Malformed)
        );
        assert!(!format!("{:?}", store.load(&planner_key(), CREATED)).contains("benchmark_build"));
    }

    #[test]
    fn c13_symlink_nonregular_wrong_mode_and_oversize_fail_closed() {
        let sandbox = Sandbox::new();
        let store = PerformanceProfileStore::from_state_home(&sandbox.root).expect("store");
        let profile = profile();
        let profile_id = profile.profile_id().unwrap();
        let path = sandbox
            .profile_directory()
            .join(format!("{profile_id}.json"));

        let target = sandbox.root.join("target");
        fs::write(&target, b"outside").unwrap();
        symlink(&target, &path).unwrap();
        assert_eq!(
            store.load(&planner_key(), CREATED),
            PerformanceProfileLoadOutcome::Invalid(ProfileStoreInvalidationReason::UnsafeObject)
        );
        assert_eq!(
            store.store(&profile),
            Err(PerformanceProfileStoreError::UnsafeProfileObject)
        );
        fs::remove_file(&path).unwrap();

        fs::create_dir(&path).unwrap();
        assert_eq!(
            store.load(&planner_key(), CREATED),
            PerformanceProfileLoadOutcome::Invalid(ProfileStoreInvalidationReason::UnsafeObject)
        );
        fs::remove_dir(&path).unwrap();

        fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            store.load(&planner_key(), CREATED),
            PerformanceProfileLoadOutcome::Invalid(ProfileStoreInvalidationReason::UnsafeObject)
        );
        fs::remove_file(&path).unwrap();

        fs::write(&path, vec![b' '; MAXIMUM_PROFILE_BYTES + 1]).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(PRIVATE_FILE_MODE)).unwrap();
        assert_eq!(
            store.load(&planner_key(), CREATED),
            PerformanceProfileLoadOutcome::Invalid(ProfileStoreInvalidationReason::Oversized)
        );
    }

    #[test]
    fn c13_state_root_and_private_directories_reject_unsafe_shapes() {
        assert_eq!(
            PerformanceProfileStore::from_state_home(Path::new("relative"))
                .expect_err("relative root rejected"),
            PerformanceProfileStoreError::UnsafeStateRoot
        );
        let sandbox = Sandbox::new();
        fs::create_dir(sandbox.root.join("phoneboost")).unwrap();
        fs::set_permissions(
            sandbox.root.join("phoneboost"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert_eq!(
            PerformanceProfileStore::from_state_home(&sandbox.root)
                .expect_err("public directory rejected"),
            PerformanceProfileStoreError::UnsafeDirectory
        );

        let symlink_sandbox = Sandbox::new();
        let actual = symlink_sandbox.root.join("actual");
        fs::create_dir(&actual).unwrap();
        let linked = symlink_sandbox.root.join("linked");
        symlink(&actual, &linked).unwrap();
        assert_eq!(
            PerformanceProfileStore::from_state_home(&linked)
                .expect_err("symlink state root rejected"),
            PerformanceProfileStoreError::UnsafeStateRoot
        );
    }

    #[test]
    fn c13_environment_resolution_is_exact_and_fail_closed() {
        assert_eq!(
            state_home_from_values(Some(OsString::from("relative")), None),
            Err(PerformanceProfileStoreError::UnsafeStateRoot)
        );
        assert_eq!(
            state_home_from_values(None, Some(OsString::from("/home/test"))).unwrap(),
            PathBuf::from("/home/test/.local/state")
        );
        assert_eq!(
            state_home_from_values(None, None),
            Err(PerformanceProfileStoreError::StateRootUnavailable)
        );
    }
}
