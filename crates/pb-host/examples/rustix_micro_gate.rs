use std::error::Error;
use std::fs;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rustix::fs::{AtFlags, Mode, OFlags, open, openat, statat, unlinkat};
use rustix::net::sockopt::socket_peercred;
use rustix::net::{AddressFamily, SocketFlags, SocketType, socketpair};
use rustix::process::getuid;

const PROBE_FILE: &str = "rustix-gate-probe";

fn require_as_fd(fd: &impl AsFd) {
    let _borrowed = fd.as_fd();
}

fn run_gate(root: &Path) -> Result<(), Box<dyn Error>> {
    let parent: OwnedFd = open(
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    require_as_fd(&parent);

    let probe: OwnedFd = openat(
        &parent,
        PROBE_FILE,
        OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR,
    )?;
    require_as_fd(&probe);
    drop(probe);

    let metadata = statat(&parent, PROBE_FILE, AtFlags::SYMLINK_NOFOLLOW)?;
    if metadata.st_size != 0 {
        return Err("new probe file was not empty".into());
    }
    unlinkat(&parent, PROBE_FILE, AtFlags::empty())?;

    let (left, right): (OwnedFd, OwnedFd) = socketpair(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC,
        None,
    )?;
    require_as_fd(&left);
    require_as_fd(&right);
    let peer = socket_peercred(&left)?;
    if peer.uid != getuid() {
        return Err("SO_PEERCRED uid did not match the current uid".into());
    }

    println!("RUSTIX_GATE_PASS open/openat statat-nofollow unlinkat socket_peercred OwnedFd AsFd");
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!(
        "phoneboost-rustix-gate-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&root)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;

    let result = run_gate(&root);
    if result.is_err() {
        let _cleanup_file = fs::remove_file(root.join(PROBE_FILE));
    }
    let cleanup = fs::remove_dir(&root);
    result?;
    cleanup?;
    Ok(())
}
