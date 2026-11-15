//! JNI symbols exist only in the explicit experimental native build.
use super::*;

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_MulticoreNative_configure(
    _env: JniEnv,
    _object: JObject,
    threads: JInt,
) -> JInt {
    int_boundary(|| {
        let Ok(mut slot) = worker_slot().lock() else {
            return ERROR_INTERNAL;
        };
        let Some(worker) = slot.as_mut() else {
            return ERROR_NOT_RUNNING;
        };
        if worker.set_experimental_cpu_profile(threads as usize) {
            RESULT_OK
        } else {
            ERROR_BAD_LOCAL_SAMPLE
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_phoneboost_app_MulticoreNative_field(
    _env: JniEnv,
    _object: JObject,
    field: JInt,
) -> JLong {
    long_boundary(|| {
        let Ok(slot) = worker_slot().lock() else {
            return ERROR_INTERNAL as JLong;
        };
        slot.as_ref()
            .map_or(-1, |worker| worker.experimental_cpu_field(field as usize))
    })
}
