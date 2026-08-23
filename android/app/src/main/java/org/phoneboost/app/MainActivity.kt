package org.phoneboost.app

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Typeface
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

class MainActivity : Activity() {
    companion object {
        private const val LOG_TAG = "PhoneBoostA6"
        private const val LOCAL_NETWORK_PERMISSION_REQUEST = 4106
    }

    private lateinit var statusView: TextView
    private val handler = Handler(Looper.getMainLooper())

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildContent())
        requestNotificationPermission()
        requestLocalNetworkPermission()
        startForegroundService(Intent(this, PhoneBoostService::class.java))
        handler.postDelayed(::refreshStatus, 350)
        Log.i(LOG_TAG, "UI_CREATED")
    }

    override fun onResume() {
        super.onResume()
        handler.postDelayed(::refreshStatus, 150)
    }

    override fun onDestroy() {
        handler.removeCallbacksAndMessages(null)
        super.onDestroy()
    }

    private fun buildContent(): ScrollView {
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(40, 56, 40, 40)
        }
        content.addView(TextView(this).apply {
            text = "PhoneBoost"
            textSize = 28f
            setTypeface(typeface, Typeface.BOLD)
        })
        content.addView(Button(this).apply {
            text = "Recreate UI"
            contentDescription = "Recreate UI"
            setOnClickListener { recreate() }
        })
        content.addView(Button(this).apply {
            text = "Refresh observations"
            contentDescription = "Refresh observations"
            setOnClickListener { refreshStatus() }
        })
        statusView = TextView(this).apply {
            text = "Worker core: STARTING"
            textSize = 18f
            setPadding(0, 24, 0, 0)
        }
        content.addView(statusView)
        return ScrollView(this).apply {
            addView(
                content,
                ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.WRAP_CONTENT,
                ),
            )
        }
    }

    private fun refreshStatus() {
        val worker = WorkerNative.snapshot()
        val observations = readAndroidObservations()
        val health = WorkerNative.healthSnapshot(SystemClock.elapsedRealtime())
        val transport = PhoneBoostService.transportSnapshot()
        val running = worker.state == WorkerNative.STATE_PAIRING_REQUIRED &&
            worker.incarnationNonzero && PhoneBoostService.isActive
        val panicContained = WorkerNative.workerPanicProbe() == WorkerNative.ERROR_PANIC_CONTAINED
        val state = when (worker.state) {
            WorkerNative.STATE_PAIRING_REQUIRED -> "PAIRING_REQUIRED"
            WorkerNative.STATE_COLD_START -> "COLD_START"
            WorkerNative.STATE_STOPPED -> "STOPPED"
            else -> "ERROR"
        }
        val incarnation = if (worker.incarnationNonzero) worker.shortIncarnation() else "UNAVAILABLE"
        val battery = observations.batteryPercent?.let { "$it%" } ?: "UNKNOWN"

        statusView.text = buildString {
            appendLine("Worker core: ${if (running) "RUNNING" else "NOT_RUNNING"}")
            appendLine("State: $state")
            appendLine("Incarnation: $incarnation")
            appendLine("Incarnation bits: ${if (worker.incarnationNonzero) 128 else 0}")
            appendLine("Foreground service: ${if (PhoneBoostService.isActive) "ACTIVE" else "INACTIVE"}")
            appendLine("JNI panic boundary: ${if (panicContained) "PASS" else "FAIL"}")
            appendLine("Android API: ${observations.api}")
            appendLine("Thermal: ${observations.thermal}")
            appendLine("Battery: $battery")
            appendLine("Charging: ${if (observations.charging) "YES" else "NO"}")
            appendLine("Power save: ${if (observations.powerSave) "ON" else "OFF"}")
            appendLine("Memory observed: ${observations.availableMemoryMib} MiB")
            appendLine("Low memory: ${if (observations.lowMemory) "YES" else "NO"}")
            appendLine("Health scheduler: ACTIVE (2s)")
            appendLine("Health samples: ${health.samples.coerceAtLeast(0)}")
            appendLine("Health safety band: ${safetyName(health.safety)}")
            appendLine("Controller lease: ${if (WorkerNative.workerAuthorityState(0) == 0) "NONE" else "ERROR"}")
            appendLine("ResourceGuard: ${if (WorkerNative.workerAuthorityState(1) == 1) "ACTIVE" else "ERROR"}")
            appendLine("Remote control: INACTIVE_FOR_REMOTE_CONTROL")
            appendLine("Transport: ${transport.state}")
            appendLine("Transport permission: ${transport.permission}")
            append("Diagnostic endpoint: ${transport.diagnosticEndpoint()}")
        }
        Log.i(
            LOG_TAG,
            "UI_STATUS state=$state incarnation=$incarnation " +
                "fgs=${if (PhoneBoostService.isActive) "ACTIVE" else "INACTIVE"} " +
                "health_samples=${health.samples.coerceAtLeast(0)} safety=${safetyName(health.safety)} " +
                "lease=NONE resource_guard=ACTIVE remote=INACTIVE_FOR_REMOTE_CONTROL " +
                "transport=${transport.state} transport_permission=${transport.permission}",
        )
    }

    private fun safetyName(value: Long): String = when (value.toInt()) {
        WorkerNative.SAFETY_NOMINAL -> "NOMINAL"
        WorkerNative.SAFETY_THROTTLE -> "THROTTLE"
        WorkerNative.SAFETY_REFUSED_MEMORY -> "REFUSED_MEMORY_PRESSURE"
        WorkerNative.SAFETY_REFUSED_THERMAL -> "REFUSED_THERMAL"
        WorkerNative.SAFETY_REFUSED_BATTERY -> "REFUSED_BATTERY"
        WorkerNative.SAFETY_REFUSED_STALE -> "REFUSED_STALE_STATE"
        else -> "UNAVAILABLE"
    }

    private fun requestNotificationPermission() {
        if (Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 4105)
        }
    }

    private fun requestLocalNetworkPermission() {
        if (Build.VERSION.SDK_INT >= 37 &&
            checkSelfPermission(ACCESS_LOCAL_NETWORK_PERMISSION) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(
                arrayOf(ACCESS_LOCAL_NETWORK_PERMISSION),
                LOCAL_NETWORK_PERMISSION_REQUEST,
            )
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == LOCAL_NETWORK_PERMISSION_REQUEST &&
            grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED
        ) {
            startForegroundService(Intent(this, PhoneBoostService::class.java))
        }
    }
}
