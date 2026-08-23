package org.phoneboost.app

object WorkerNative {
    const val RESULT_OK = 0
    const val RESULT_ALREADY_RUNNING = 1
    const val ERROR_PANIC_CONTAINED = -4
    const val STATE_STOPPED = 0
    const val STATE_COLD_START = 1
    const val STATE_PAIRING_REQUIRED = 2
    const val SAFETY_NOMINAL = 0
    const val SAFETY_THROTTLE = 1
    const val SAFETY_REFUSED_MEMORY = 2
    const val SAFETY_REFUSED_THERMAL = 3
    const val SAFETY_REFUSED_BATTERY = 4
    const val SAFETY_REFUSED_STALE = 5

    init {
        System.loadLibrary("phoneboost_core_jni")
    }

    external fun workerStart(): Int
    external fun workerStatusState(): Int
    external fun workerIncarnationHigh(): Long
    external fun workerIncarnationLow(): Long
    external fun workerStop(): Int
    external fun workerUpdateHealth(
        availableMemoryBytes: Long,
        lowMemory: Int,
        thermalCode: Int,
        batteryPercent: Int,
        charging: Int,
        powerSave: Int,
        monotonicMs: Long,
    ): Int
    external fun workerHealthField(field: Int, nowMs: Long): Long
    external fun workerAuthorityState(field: Int): Int

    // The A5 debug build enables the matching Rust jni-test-probes feature.
    external fun workerPanicProbe(): Int

    fun snapshot(): WorkerSnapshot {
        val state = workerStatusState()
        if (state != STATE_PAIRING_REQUIRED) {
            return WorkerSnapshot(state, 0, 0)
        }
        return WorkerSnapshot(state, workerIncarnationHigh(), workerIncarnationLow())
    }
}

data class HealthSnapshot(
    val samples: Long,
    val safety: Long,
    val thermal: Long,
    val battery: Long,
    val ageMs: Long,
    val budgetBytes: Long,
)

fun WorkerNative.healthSnapshot(nowMs: Long): HealthSnapshot = HealthSnapshot(
    samples = workerHealthField(0, nowMs),
    safety = workerHealthField(1, nowMs),
    thermal = workerHealthField(2, nowMs),
    battery = workerHealthField(3, nowMs),
    ageMs = workerHealthField(4, nowMs),
    budgetBytes = workerHealthField(5, nowMs),
)

data class WorkerSnapshot(
    val state: Int,
    val incarnationHigh: Long,
    val incarnationLow: Long,
) {
    val incarnationNonzero: Boolean
        get() = incarnationHigh != 0L || incarnationLow != 0L

    fun shortIncarnation(): String {
        val word = if (incarnationHigh != 0L) incarnationHigh else incarnationLow
        return java.lang.Long.toUnsignedString(word, 16).padStart(16, '0').take(8)
    }
}
