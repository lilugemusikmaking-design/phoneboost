//! Only linked into an explicitly diagnostic, experimental APK.
use super::*;
use pb_runtime_secure::diagnostic_timing as timing;
static SNAPSHOT: Mutex<Option<timing::Snapshot>> = Mutex::new(None);
use pb_runtime_secure::put_timing;
static PUT_SNAPSHOT: Mutex<(Vec<put_timing::Row>, u64)> = Mutex::new((Vec::new(), 0));

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_TimingNative_traceControl(
    _env: JniEnv,
    _object: JObject,
    enabled: JInt,
) -> JInt {
    int_boundary(|| {
        if !matches!(enabled, 0 | 1) {
            return ERROR_BAD_LOCAL_SAMPLE;
        }
        put_timing::control(enabled == 1);
        RESULT_OK
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_TimingNative_traceCapture(
    _env: JniEnv,
    _object: JObject,
) -> JInt {
    int_boundary(|| {
        let Ok(mut slot) = PUT_SNAPSHOT.lock() else {
            return ERROR_INTERNAL;
        };
        *slot = put_timing::snapshot();
        slot.0.len() as JInt
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_TimingNative_traceField(
    _env: JniEnv,
    _object: JObject,
    row: JInt,
    field: JInt,
) -> JLong {
    long_boundary(|| {
        let Ok(slot) = PUT_SNAPSHOT.lock() else {
            return ERROR_INTERNAL as JLong;
        };
        if row == -1 && field == 0 {
            return slot.1.min(i64::MAX as u64) as JLong;
        }
        slot.0
            .get(row as usize)
            .and_then(|r| r.get(field as usize))
            .copied()
            .and_then(|v| JLong::try_from(v).ok())
            .unwrap_or(-1)
    })
}

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
