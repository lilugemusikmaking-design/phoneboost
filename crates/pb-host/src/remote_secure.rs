use std::sync::{Arc, OnceLock};
use std::time::Duration;

use pb_runtime_secure::{
    EndpointRole, PairingActionResult, RuntimeError, RuntimeSnapshot, SecureRuntime, StateStore,
};

static REMOTE_SECURE: OnceLock<Arc<SecureRuntime>> = OnceLock::new();

pub fn initialize_remote_secure() -> Result<Arc<SecureRuntime>, RuntimeError> {
    if let Some(runtime) = REMOTE_SECURE.get() {
        return Ok(Arc::clone(runtime));
    }
    let store = StateStore::open_host()?;
    let runtime = Arc::new(SecureRuntime::initialize(
        EndpointRole::LinuxInitiator,
        store,
    )?);
    REMOTE_SECURE
        .set(Arc::clone(&runtime))
        .map_err(|_| RuntimeError::SessionBusy)?;
    Ok(runtime)
}

pub fn remote_secure_runtime() -> Option<Arc<SecureRuntime>> {
    REMOTE_SECURE.get().map(Arc::clone)
}

pub fn remote_status() -> Option<RuntimeSnapshot> {
    REMOTE_SECURE.get().map(|runtime| runtime.snapshot())
}

pub fn remote_pairing_begin() -> Result<RuntimeSnapshot, RuntimeError> {
    REMOTE_SECURE
        .get()
        .ok_or(RuntimeError::NoConnectedTransport)?
        .begin_pairing(Duration::from_secs(10))
}

pub fn remote_pairing_confirm() -> Result<PairingActionResult, RuntimeError> {
    let runtime = REMOTE_SECURE
        .get()
        .ok_or(RuntimeError::NoConnectedTransport)?;
    Ok(runtime.local_confirm())
}

pub fn remote_pairing_cancel() -> Result<PairingActionResult, RuntimeError> {
    let runtime = REMOTE_SECURE
        .get()
        .ok_or(RuntimeError::NoConnectedTransport)?;
    Ok(runtime.cancel())
}
