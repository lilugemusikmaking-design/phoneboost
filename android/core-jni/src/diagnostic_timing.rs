//! Only linked into an explicitly diagnostic, experimental APK.
use super::*;
use pb_runtime_secure::diagnostic_timing as timing;
static SNAPSHOT: Mutex<Option<timing::Snapshot>> = Mutex::new(None);

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_TimingNative_control(
    _env: JniEnv,
    _object: JObject,
    enabled: JInt,
) -> JInt {
    int_boundary(|| {
        if !matches!(enabled, 0 | 1) {
            return ERROR_BAD_LOCAL_SAMPLE;
        }
        timing::set_enabled(false);
        if enabled == 1 {
            timing::reset();
            timing::set_enabled(true);
        }
        RESULT_OK
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_TimingNative_capture(
    _env: JniEnv,
    _object: JObject,
) -> JInt {
    int_boundary(|| {
        let Ok(mut slot) = SNAPSHOT.lock() else {
            return ERROR_INTERNAL;
        };
        *slot = Some(timing::Snapshot::take());
        timing::NAMES.len() as JInt
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_TimingNative_field(
    _env: JniEnv,
    _object: JObject,
    phase: JInt,
    field: JInt,
) -> JLong {
    long_boundary(|| {
        let Ok(slot) = SNAPSHOT.lock() else {
            return ERROR_INTERNAL as JLong;
        };
        slot.as_ref()
            .and_then(|s| s.value(phase as usize, field as usize))
            .and_then(|v| JLong::try_from(v).ok())
            .unwrap_or(-1)
    })
}
