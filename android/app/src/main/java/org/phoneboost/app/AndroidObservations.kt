package org.phoneboost.app

import android.app.ActivityManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.Build
import android.os.PowerManager

data class AndroidObservations(
    val api: Int,
    val thermal: String,
    val thermalCode: Int,
    val batteryPercent: Int?,
    val charging: Boolean?,
    val powerSave: Boolean,
    val availableMemoryMib: Long,
    val availableMemoryBytes: Long,
    val lowMemory: Boolean,
    val totalMemoryBytes: Long,
)

fun Context.readAndroidObservations(): AndroidObservations {
    val power = getSystemService(PowerManager::class.java)
    val memory = ActivityManager.MemoryInfo().also {
        getSystemService(ActivityManager::class.java).getMemoryInfo(it)
    }
    val battery = registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
    val level = battery?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1
    val scale = battery?.getIntExtra(BatteryManager.EXTRA_SCALE, -1) ?: -1
    val percent = batteryPercentage(level, scale)
    val batteryStatus = battery?.getIntExtra(BatteryManager.EXTRA_STATUS, -1) ?: -1
    val thermal = power.currentThermalStatus

    return AndroidObservations(
        api = Build.VERSION.SDK_INT,
        thermal = thermalName(thermal),
        thermalCode = thermal,
        batteryPercent = percent,
        charging = batteryCharging(batteryStatus),
        powerSave = power.isPowerSaveMode,
        availableMemoryMib = memory.availMem / (1024L * 1024L),
        availableMemoryBytes = memory.availMem,
        lowMemory = memory.lowMemory,
        totalMemoryBytes = memory.totalMem,
    )
}

internal fun batteryPercentage(level: Int, scale: Int): Int? =
    if (scale > 0 && level in 0..scale) (level.toLong() * 100 / scale).toInt() else null

internal fun batteryCharging(status: Int): Boolean? = when (status) {
    BatteryManager.BATTERY_STATUS_CHARGING, BatteryManager.BATTERY_STATUS_FULL -> true
    BatteryManager.BATTERY_STATUS_DISCHARGING, BatteryManager.BATTERY_STATUS_NOT_CHARGING -> false
    else -> null
}

// A partial observation is never a healthy sample. In particular, neither an
// absent battery status nor an invalid thermal code is a nominal/false value.
internal fun AndroidObservations.healthUnavailableReason(): String? = when {
    totalMemoryBytes <= 0 || availableMemoryBytes !in 0..totalMemoryBytes -> "MEMORY_INCONSISTENT"
    thermalCode !in PowerManager.THERMAL_STATUS_NONE..PowerManager.THERMAL_STATUS_SHUTDOWN ->
        "THERMAL_UNAVAILABLE"
    batteryPercent == null || batteryPercent !in 0..100 -> "BATTERY_UNAVAILABLE"
    charging == null -> "CHARGING_UNAVAILABLE"
    else -> null
}

internal fun AndroidObservations.publishHealth(sampledAtWorkerMs: Long): Int {
    if (healthUnavailableReason() != null) {
        val result = WorkerNative.workerInvalidateHealth()
        return if (result == WorkerNative.RESULT_OK) WorkerNative.ERROR_BAD_LOCAL_SAMPLE else result
    }
    return WorkerNative.workerUpdateHealth(
        availableMemoryBytes,
        if (lowMemory) 1 else 0,
        thermalCode,
        requireNotNull(batteryPercent),
        if (charging == true) 1 else 0,
        if (powerSave) 1 else 0,
        sampledAtWorkerMs,
    )
}

private fun thermalName(status: Int): String = when (status) {
    PowerManager.THERMAL_STATUS_NONE -> "NONE"
    PowerManager.THERMAL_STATUS_LIGHT -> "LIGHT"
    PowerManager.THERMAL_STATUS_MODERATE -> "MODERATE"
    PowerManager.THERMAL_STATUS_SEVERE -> "SEVERE"
    PowerManager.THERMAL_STATUS_CRITICAL -> "CRITICAL"
    PowerManager.THERMAL_STATUS_EMERGENCY -> "EMERGENCY"
    PowerManager.THERMAL_STATUS_SHUTDOWN -> "SHUTDOWN"
    else -> "UNKNOWN"
}
