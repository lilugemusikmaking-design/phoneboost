//! Daemon-owned durable scheduling preference. Never a remote admission authority.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, fstat, fsync, mkdirat, open, openat, renameat, unlinkat,
};
use rustix::io::Errno;
use rustix::process::getuid;
use serde::{Deserialize, Serialize};

use crate::AutoUseController;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
const CONFIG: &str = "config.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PreferencePersistence {
    Default,
    Saved,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AutoUsePreference {
    pub enabled: bool,
    pub persistence: PreferencePersistence,
}

#[derive(Debug)]
pub struct PreferenceWriteFailed;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema_version: u8,
    auto_use_enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WriteStage {
    Write,
    FileSync,
    Rename,
    DirectorySync,
}

struct ConfigStore {
    directory: OwnedFd,
}

fn valid_config_metadata(stat: &rustix::fs::Stat) -> bool {
    FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile
        && stat.st_uid == getuid().as_raw()
        && stat.st_mode & 0o7777 == 0o600
        && (0..=256).contains(&stat.st_size)
}

impl ConfigStore {
    fn open(state_home: &Path) -> Result<Self, PreferenceWriteFailed> {
        if !state_home.is_absolute() {
            return Err(PreferenceWriteFailed);
        }
        // Walk descriptors, not a pre-check followed by a path open. A symlink
        // in any ancestor is refused even if it points to an otherwise safe dir.
        let mut directory = open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| PreferenceWriteFailed)?;
        for component in state_home.components() {
            match component {
                Component::RootDir => (),
                Component::Normal(name) => {
                    directory = openat(
                        &directory,
                        name,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(|_| PreferenceWriteFailed)?;
                }
                _ => return Err(PreferenceWriteFailed),
            }
        }
        let stat = fstat(&directory).map_err(|_| PreferenceWriteFailed)?;
        if stat.st_uid != getuid().as_raw() {
            return Err(PreferenceWriteFailed);
        }
        match mkdirat(&directory, "phoneboost", Mode::RWXU) {
            Ok(()) => fsync(&directory).map_err(|_| PreferenceWriteFailed)?,
            Err(Errno::EXIST) => (),
            Err(_) => return Err(PreferenceWriteFailed),
        }
        let directory = openat(
            &directory,
            "phoneboost",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| PreferenceWriteFailed)?;
        let store = Self { directory };
        store.validate_directory()?;
        Ok(store)
    }

    fn validate_directory(&self) -> Result<(), PreferenceWriteFailed> {
        let stat = fstat(&self.directory).map_err(|_| PreferenceWriteFailed)?;
        if stat.st_uid != getuid().as_raw() || stat.st_mode & 0o7777 != 0o700 {
            return Err(PreferenceWriteFailed);
        }
        Ok(())
    }

    fn read(&self) -> Result<Option<bool>, PreferenceWriteFailed> {
        self.validate_directory()?;
        let fd = match openat(
            &self.directory,
            CONFIG,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(Errno::NOENT) => return Ok(None),
            Err(_) => return Err(PreferenceWriteFailed),
        };
        let before = fstat(&fd).map_err(|_| PreferenceWriteFailed)?;
        if !valid_config_metadata(&before) {
            return Err(PreferenceWriteFailed);
        }
        let mut file = File::from(fd);
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(257)
            .read_to_end(&mut bytes)
            .map_err(|_| PreferenceWriteFailed)?;
        let after = fstat(&file).map_err(|_| PreferenceWriteFailed)?;
        if bytes.len() > 256
            || before.st_size != after.st_size
            || before.st_mtime != after.st_mtime
            || before.st_mtime_nsec != after.st_mtime_nsec
            || before.st_ctime != after.st_ctime
            || before.st_ctime_nsec != after.st_ctime_nsec
        {
            return Err(PreferenceWriteFailed);
        }
        if bytes.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
            return Err(PreferenceWriteFailed);
        }
        let config: Config = serde_json::from_slice(&bytes).map_err(|_| PreferenceWriteFailed)?;
        if config.schema_version != 1 {
            return Err(PreferenceWriteFailed);
        }
        Ok(Some(config.auto_use_enabled))
    }

    fn write(
        &self,
        enabled: bool,
        mut stage: impl FnMut(WriteStage) -> Result<(), PreferenceWriteFailed>,
    ) -> Result<(), PreferenceWriteFailed> {
        self.validate_directory()?;
        let name = format!(
            ".config-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        );
        let fd = openat(
            &self.directory,
            name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|_| PreferenceWriteFailed)?;
        let result = (|| {
            let mut file = File::from(fd);
            let stat = fstat(&file).map_err(|_| PreferenceWriteFailed)?;
            if stat.st_mode & 0o7777 != 0o600 {
                return Err(PreferenceWriteFailed);
            }
            let bytes = serde_json::to_vec(&Config {
                schema_version: 1,
                auto_use_enabled: enabled,
            })
            .map_err(|_| PreferenceWriteFailed)?;
            stage(WriteStage::Write)?;
            file.write_all(&bytes).map_err(|_| PreferenceWriteFailed)?;
            stage(WriteStage::FileSync)?;
            fsync(&file).map_err(|_| PreferenceWriteFailed)?;
            stage(WriteStage::Rename)?;
            renameat(&self.directory, name.as_str(), &self.directory, CONFIG)
                .map_err(|_| PreferenceWriteFailed)?;
            stage(WriteStage::DirectorySync)?;
            fsync(&self.directory).map_err(|_| PreferenceWriteFailed)
        })();
        if result.is_err() {
            // No success or rollback claim if cleanup fails. The name is private;
            // only config.json is ever loaded, never a leftover temporary file.
            let _ = unlinkat(&self.directory, name.as_str(), AtFlags::empty());
        }
        result
    }
}

struct PreferenceData {
    store: Option<ConfigStore>,
    view: AutoUsePreference,
    invalid_store: bool,
}

pub struct AutoUseControl {
    controller: Arc<AutoUseController>,
    data: Mutex<PreferenceData>,
}

impl AutoUseControl {
    pub fn from_environment(controller: Arc<AutoUseController>) -> Self {
        let path = match std::env::var_os("XDG_STATE_HOME") {
            Some(path) => PathBuf::from(path),
            None => std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|p| p.join(".local/state"))
                .unwrap_or_default(),
        };
        Self::from_state_home(controller, &path)
    }

    pub fn from_state_home(controller: Arc<AutoUseController>, path: &Path) -> Self {
        let store = ConfigStore::open(path).ok();
        let loaded = store
            .as_ref()
            .ok_or(PreferenceWriteFailed)
            .and_then(ConfigStore::read);
        let (enabled, persistence, invalid_store) = match loaded {
            Ok(Some(enabled)) => (enabled, PreferencePersistence::Saved, false),
            Ok(None) => (true, PreferencePersistence::Default, false),
            Err(_) => (false, PreferencePersistence::Error, true),
        };
        if enabled {
            controller.enable();
        } else {
            controller.disable();
        }
        Self {
            controller,
            data: Mutex::new(PreferenceData {
                store,
                view: AutoUsePreference {
                    enabled,
                    persistence,
                },
                invalid_store,
            }),
        }
    }

    pub fn get(&self) -> AutoUsePreference {
        self.data.lock().unwrap_or_else(|p| p.into_inner()).view
    }

    pub fn set(&self, enabled: bool) -> Result<AutoUsePreference, PreferenceWriteFailed> {
        self.set_with(enabled, |_| Ok(()))
    }

    fn set_with(
        &self,
        enabled: bool,
        stage: impl FnMut(WriteStage) -> Result<(), PreferenceWriteFailed>,
    ) -> Result<AutoUsePreference, PreferenceWriteFailed> {
        let mut data = self.data.lock().unwrap_or_else(|p| p.into_inner());
        if !enabled {
            self.controller.disable();
        }
        // Refuse corruption discovered since startup too; never silently repair.
        if data
            .store
            .as_ref()
            .is_none_or(|store| store.read().is_err())
        {
            data.invalid_store = true;
        }
        let written = if data.invalid_store {
            Err(PreferenceWriteFailed)
        } else {
            data.store
                .as_ref()
                .ok_or(PreferenceWriteFailed)
                .and_then(|store| store.write(enabled, stage))
        };
        if written.is_err() {
            self.controller.disable();
            data.view = AutoUsePreference {
                enabled: false,
                persistence: PreferencePersistence::Error,
            };
            return Err(PreferenceWriteFailed);
        }
        if enabled {
            self.controller.enable();
        }
        data.view = AutoUsePreference {
            enabled,
            persistence: PreferencePersistence::Saved,
        };
        Ok(data.view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AutoUseState, DeviceDiscovery, DiscoveryError, TransportCandidate};
    use pb_runtime_secure::{EndpointRole, SecureRuntime, StateStore};
    use std::fs;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};

    struct NoDevice;
    impl DeviceDiscovery for NoDevice {
        fn discover(&self) -> Result<Option<TransportCandidate>, DiscoveryError> {
            Ok(None)
        }
    }
    struct Fixture {
        path: PathBuf,
        controller: Arc<AutoUseController>,
    }
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "phoneboost-control-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            let runtime_dir = path.join("runtime");
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&runtime_dir)
                .unwrap();
            let store =
                StateStore::from_directory_fd(File::open(runtime_dir).unwrap().into()).unwrap();
            store.load_or_create_identity().unwrap();
            let runtime =
                Arc::new(SecureRuntime::initialize(EndpointRole::LinuxInitiator, store).unwrap());
            let controller = Arc::new(AutoUseController::new(runtime, Arc::new(NoDevice)).unwrap());
            Self { path, controller }
        }
        fn control(&self) -> AutoUseControl {
            AutoUseControl::from_state_home(Arc::clone(&self.controller), &self.path)
        }
        fn config(&self) -> PathBuf {
            self.path.join("phoneboost/config.json")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.path).unwrap();
        }
    }

    #[test]
    fn preference_default_and_durable_off_on_roundtrip() {
        let fixture = Fixture::new();
        let control = fixture.control();
        assert_eq!(
            control.get(),
            AutoUsePreference {
                enabled: true,
                persistence: PreferencePersistence::Default
            }
        );
        assert!(!fixture.config().exists()); // get never writes
        control.set(false).unwrap();
        assert_eq!(fixture.controller.current_state(), AutoUseState::Off);
        drop(control);
        let control = fixture.control();
        assert_eq!(
            control.get(),
            AutoUsePreference {
                enabled: false,
                persistence: PreferencePersistence::Saved
            }
        );
        control.set(true).unwrap();
        assert!(fixture.control().get().enabled);
        assert_eq!(
            fs::metadata(fixture.config()).unwrap().permissions().mode() & 0o7777,
            0o600
        );
    }

    #[test]
    fn preference_write_failures_disable_without_false_durable_success() {
        for fail in [
            WriteStage::Write,
            WriteStage::FileSync,
            WriteStage::Rename,
            WriteStage::DirectorySync,
        ] {
            for desired in [false, true] {
                let fixture = Fixture::new();
                let control = fixture.control();
                control.set(!desired).unwrap();
                assert!(
                    control
                        .set_with(desired, |at| if at == fail {
                            Err(PreferenceWriteFailed)
                        } else {
                            Ok(())
                        })
                        .is_err()
                );
                assert_eq!(fixture.controller.current_state(), AutoUseState::Off);
                assert_eq!(
                    control.get(),
                    AutoUsePreference {
                        enabled: false,
                        persistence: PreferencePersistence::Error
                    }
                );
                let store = ConfigStore::open(&fixture.path).unwrap();
                assert_eq!(
                    store.read().unwrap(),
                    Some(if fail == WriteStage::DirectorySync {
                        desired
                    } else {
                        !desired
                    })
                );
                assert_eq!(
                    fs::read_dir(fixture.path.join("phoneboost"))
                        .unwrap()
                        .count(),
                    1
                );
                // Only a new explicit command retries a transient write failure.
                control.set(desired).unwrap();
                assert_eq!(control.get().enabled, desired);
            }
        }
    }

    #[test]
    fn preference_invalid_config_is_never_repaired() {
        for bytes in [
            "{}",
            "[1,true]",
            "{",
            "null",
            "{\"schema_version\":2,\"auto_use_enabled\":true}",
            "{\"schema_version\":1,\"auto_use_enabled\":true,\"auto_use_enabled\":false}",
            "{\"schema_version\":1,\"auto_use_enabled\":1}",
            "{\"schema_version\":1,\"auto_use_enabled\":false,\"path\":\"x\"}",
        ] {
            let fixture = Fixture::new();
            fixture.control().set(true).unwrap();
            fs::write(fixture.config(), bytes).unwrap();
            let control = fixture.control();
            assert_eq!(control.get().persistence, PreferencePersistence::Error);
            assert_eq!(fixture.controller.current_state(), AutoUseState::Off);
            assert!(control.set(true).is_err());
            assert!(control.set(false).is_err());
            assert_eq!(fs::read(fixture.config()).unwrap(), bytes.as_bytes());
        }
    }

    #[test]
    fn preference_unsafe_objects_and_ancestors_fail_closed() {
        let fixture = Fixture::new();
        fixture.control().set(true).unwrap();
        fs::set_permissions(fixture.config(), fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            fixture.control().get().persistence,
            PreferencePersistence::Error
        );
        fs::set_permissions(fixture.config(), fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(fixture.config(), vec![b' '; 257]).unwrap();
        assert_eq!(
            fixture.control().get().persistence,
            PreferencePersistence::Error
        );
        fs::remove_file(fixture.config()).unwrap();
        symlink(fixture.path.join("missing"), fixture.config()).unwrap();
        assert_eq!(
            fixture.control().get().persistence,
            PreferencePersistence::Error
        );
        fs::remove_file(fixture.config()).unwrap();
        fs::create_dir(fixture.config()).unwrap();
        assert_eq!(
            fixture.control().get().persistence,
            PreferencePersistence::Error
        );
        let link = fixture.path.join("alias");
        symlink(&fixture.path, &link).unwrap();
        assert!(ConfigStore::open(&link).is_err());
        fs::set_permissions(
            fixture.path.join("phoneboost"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert!(ConfigStore::open(&fixture.path).is_err());
        assert!(ConfigStore::open(Path::new("relative")).is_err());
    }

    #[test]
    fn preference_corruption_after_startup_blocks_set() {
        let fixture = Fixture::new();
        let control = fixture.control();
        control.set(true).unwrap();
        fs::write(fixture.config(), b"unknown version").unwrap();
        assert!(control.set(true).is_err());
        assert_eq!(fixture.controller.current_state(), AutoUseState::Off);
        assert_eq!(fs::read(fixture.config()).unwrap(), b"unknown version");
    }

    #[test]
    fn preference_owner_gate_rejects_other_uid_metadata() {
        let fixture = Fixture::new();
        fixture.control().set(false).unwrap();
        let mut stat = fstat(File::open(fixture.config()).unwrap()).unwrap();
        assert!(valid_config_metadata(&stat));
        stat.st_uid = getuid().as_raw().wrapping_add(1);
        assert!(!valid_config_metadata(&stat));
    }

    #[test]
    fn preference_concurrent_setters_match_storage_and_off_falls_back() {
        let fixture = Fixture::new();
        let control = Arc::new(fixture.control());
        std::thread::scope(|scope| {
            for enabled in [false, true, false, true] {
                let control = Arc::clone(&control);
                scope.spawn(move || {
                    control.set(enabled).unwrap();
                });
            }
        });
        let persisted = ConfigStore::open(&fixture.path)
            .unwrap()
            .read()
            .unwrap()
            .unwrap();
        assert_eq!(persisted, control.get().enabled);
        control.set(false).unwrap();
        let result = fixture.controller.execute_blake3(b"off");
        assert_eq!(
            result.source(),
            crate::ExecutionSource::LocalFallbackAfterRemoteUnavailable
        );
        assert_eq!(result.reason(), crate::AutoUseReason::Off);
        assert_eq!(result.digest(), *blake3::hash(b"off").as_bytes());
    }
}
