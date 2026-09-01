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
    val charging: Boolean,
    val powerSave: Boolean,
    val availableMemoryMib: Long,
    val availableMemoryBytes: Long,
    val lowMemory: Boolean,
)

fun Context.readAndroidObservations(): AndroidObservations {
    val power = getSystemService(PowerManager::class.java)
    val memory = ActivityManager.MemoryInfo().also {
        getSystemService(ActivityManager::class.java).getMemoryInfo(it)
    }
    val battery = registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
    val level = battery?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1
    val scale = battery?.getIntExtra(BatteryManager.EXTRA_SCALE, -1) ?: -1
    val percent = if (level >= 0 && scale > 0) level * 100 / scale else null
    val batteryStatus = battery?.getIntExtra(BatteryManager.EXTRA_STATUS, -1) ?: -1
    val charging = batteryStatus == BatteryManager.BATTERY_STATUS_CHARGING ||
        batteryStatus == BatteryManager.BATTERY_STATUS_FULL

    return AndroidObservations(
        api = Build.VERSION.SDK_INT,
        thermal = thermalName(power.currentThermalStatus),
        thermalCode = power.currentThermalStatus,
        batteryPercent = percent,
        charging = charging,
        powerSave = power.isPowerSaveMode,
        availableMemoryMib = memory.availMem / (1024L * 1024L),
        availableMemoryBytes = memory.availMem,
        lowMemory = memory.lowMemory,
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
