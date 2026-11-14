package org.phoneboost.app

internal object AndroidHealthSourceTest {
    @JvmStatic
    fun main(args: Array<String>) {
        val percentages = listOf(
            Triple(40, 80, 50), Triple(0, 100, 0), Triple(100, 100, 100),
            Triple(Int.MAX_VALUE, Int.MAX_VALUE, 100),
            Triple(-1, 100, null), Triple(50, 0, null), Triple(101, 100, null),
        )
        percentages.forEach { (level, scale, expected) ->
            check(batteryPercentage(level, scale) == expected)
        }
        // Public BatteryManager status codes: UNKNOWN, CHARGING,
        // DISCHARGING, NOT_CHARGING, FULL. UNKNOWN is never false.
        listOf(-1 to null, 1 to null, 2 to true, 3 to false, 4 to false, 5 to true, 6 to null)
            .forEach { (status, expected) -> check(batteryCharging(status) == expected) }

        val normal = AndroidObservations(
            api = 36, thermal = "NONE", thermalCode = 0,
            batteryPercent = 80, charging = false, powerSave = false,
            availableMemoryMib = 1024, availableMemoryBytes = 1_073_741_824,
            lowMemory = false, totalMemoryBytes = 4_294_967_296,
        )
        check(normal.healthUnavailableReason() == null)
        check(normal.copy(availableMemoryBytes = 0, lowMemory = true).healthUnavailableReason() == null)
        check(normal.copy(availableMemoryBytes = normal.totalMemoryBytes).healthUnavailableReason() == null)
        val unavailable = listOf(
            normal.copy(totalMemoryBytes = 0) to "MEMORY_INCONSISTENT",
            normal.copy(availableMemoryBytes = -1) to "MEMORY_INCONSISTENT",
            normal.copy(availableMemoryBytes = normal.totalMemoryBytes + 1) to "MEMORY_INCONSISTENT",
            normal.copy(thermalCode = -1) to "THERMAL_UNAVAILABLE",
            normal.copy(thermalCode = 7) to "THERMAL_UNAVAILABLE",
            normal.copy(batteryPercent = null) to "BATTERY_UNAVAILABLE",
            normal.copy(batteryPercent = 101) to "BATTERY_UNAVAILABLE",
            normal.copy(charging = null) to "CHARGING_UNAVAILABLE",
        )

        // This loads the actual host JNI library, not a mocked WorkerNative.
        check(WorkerNative.workerStart() == WorkerNative.RESULT_OK)
        try {
            unavailable.forEach { (observation, reason) ->
                check(normal.publishHealth(WorkerNative.workerHealthSampleTimeMs()) == WorkerNative.RESULT_OK)
                val accepted = WorkerNative.healthSnapshot(Long.MAX_VALUE)
                check(accepted.ageMs in 0..6000)
                check(accepted.thermal == 0L && accepted.battery == 0L)
                // Initial ten-second memory recovery still applies.
                check(accepted.safety == WorkerNative.SAFETY_REFUSED_MEMORY.toLong())
                check(observation.healthUnavailableReason() == reason)
                check(observation.publishHealth(WorkerNative.workerHealthSampleTimeMs()) ==
                    WorkerNative.ERROR_BAD_LOCAL_SAMPLE)
                val refused = WorkerNative.healthSnapshot(0)
                check(refused.samples == accepted.samples)
                check(refused.safety == WorkerNative.SAFETY_REFUSED_STALE.toLong())
                check(refused.thermal == 7L && refused.battery == 3L)
                check(refused.ageMs == -1L && refused.budgetBytes == 0L)
            }
            check(normal.publishHealth(Long.MAX_VALUE) == WorkerNative.ERROR_BAD_LOCAL_SAMPLE)
            check(WorkerNative.healthSnapshot(0).safety == WorkerNative.SAFETY_REFUSED_STALE.toLong())
            check(normal.copy(thermalCode = 3, thermal = "SEVERE").publishHealth(
                WorkerNative.workerHealthSampleTimeMs(),
            ) == WorkerNative.RESULT_OK)
            check(WorkerNative.healthSnapshot(0).thermal == 3L)
            check(WorkerNative.workerInvalidateHealth() == WorkerNative.RESULT_OK)
            check(WorkerNative.healthSnapshot(0).ageMs == -1L)
        } finally {
            check(WorkerNative.workerStop() == WorkerNative.RESULT_OK)
        }
        println("AndroidHealthSourceTest PASS (API-value fixtures + real Kotlin/JNI/ResourceGuard on host; no device proof)")
    }
}
