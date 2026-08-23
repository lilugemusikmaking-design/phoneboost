package org.phoneboost.app

object WorkerNative {
    const val RESULT_OK = 0
    const val RESULT_ALREADY_RUNNING = 1
    const val ERROR_PANIC_CONTAINED = -4
    const val STATE_STOPPED = 0
    const val STATE_COLD_START = 1
    const val STATE_PAIRING_REQUIRED = 2

    init {
        System.loadLibrary("phoneboost_core_jni")
    }

    external fun workerStart(): Int
    external fun workerStatusState(): Int
    external fun workerIncarnationHigh(): Long
    external fun workerIncarnationLow(): Long
    external fun workerStop(): Int

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

