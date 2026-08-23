package org.phoneboost.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import android.util.Log

class PhoneBoostService : Service() {
    companion object {
        private const val CHANNEL_ID = "phoneboost_worker"
        private const val NOTIFICATION_ID = 4105
        private const val LOG_TAG = "PhoneBoostA6"
        private const val HEALTH_INTERVAL_MS = 2_000L

        @Volatile
        var isActive: Boolean = false
            private set

        @Volatile
        private var latestTransport = AndroidTransportSnapshot.unavailable(
            LanPermissionState.NOT_REQUIRED_API_LT_37,
        )

        fun transportSnapshot(): AndroidTransportSnapshot = latestTransport
    }

    private var healthThread: HandlerThread? = null
    private var healthHandler: Handler? = null
    private var localIpTransport: LocalIpTransport? = null
    private val sampleHealth = object : Runnable {
        override fun run() {
            try {
                val observations = readAndroidObservations()
                val battery = observations.batteryPercent
                if (battery != null) {
                    val sampledAt = SystemClock.elapsedRealtime()
                    val result = WorkerNative.workerUpdateHealth(
                        observations.availableMemoryBytes,
                        if (observations.lowMemory) 1 else 0,
                        observations.thermalCode,
                        battery,
                        if (observations.charging) 1 else 0,
                        if (observations.powerSave) 1 else 0,
                        sampledAt,
                    )
                    val health = WorkerNative.healthSnapshot(sampledAt)
                    Log.i(
                        LOG_TAG,
                        "HEALTH_SAMPLE count=${health.samples} at_ms=$sampledAt " +
                            "result=$result safety=${health.safety} thermal=${health.thermal} " +
                            "battery=${health.battery} charging=${observations.charging} " +
                            "power_save=${observations.powerSave} low_memory=${observations.lowMemory} " +
                            "memory_mib=${observations.availableMemoryMib}",
                    )
                } else {
                    Log.w(LOG_TAG, "HEALTH_SAMPLE_SKIPPED reason=BATTERY_UNAVAILABLE")
                }
            } catch (_: RuntimeException) {
                Log.e(LOG_TAG, "HEALTH_SAMPLE_FAILED")
            } finally {
                healthHandler?.postDelayed(this, HEALTH_INTERVAL_MS)
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        val startResult = WorkerNative.workerStart()
        if (startResult != WorkerNative.RESULT_OK &&
            startResult != WorkerNative.RESULT_ALREADY_RUNNING
        ) {
            stopSelf()
            return
        }

        val notification = workerNotification()
        startForeground(
            NOTIFICATION_ID,
            notification,
            ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
        )
        isActive = true
        startHealthSampler()
        startLocalIpTransport()
        Log.i(LOG_TAG, "FGS_STARTED state=PAIRING_REQUIRED")
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (localIpTransport == null && lanPermissionState() != LanPermissionState.DENIED) {
            startLocalIpTransport()
        }
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        isActive = false
        healthHandler?.removeCallbacksAndMessages(null)
        healthThread?.quitSafely()
        healthHandler = null
        healthThread = null
        localIpTransport?.stop()
        latestTransport = localIpTransport?.snapshot()
            ?: AndroidTransportSnapshot.unavailable(lanPermissionState())
        localIpTransport = null
        WorkerNative.workerStop()
        stopForeground(STOP_FOREGROUND_REMOVE)
        Log.i(LOG_TAG, "WORKER_STOP")
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun startHealthSampler() {
        val thread = HandlerThread("phoneboost-health")
        thread.start()
        healthThread = thread
        healthHandler = Handler(thread.looper).also { it.post(sampleHealth) }
    }

    private fun startLocalIpTransport() {
        val transport = LocalIpTransport(this)
        latestTransport = transport.snapshot()
        if (!transport.start()) {
            latestTransport = transport.snapshot()
            return
        }
        localIpTransport = transport
        latestTransport = transport.snapshot()
        Handler(mainLooper).post(object : Runnable {
            override fun run() {
                val activeTransport = localIpTransport ?: return
                latestTransport = activeTransport.snapshot()
                Handler(mainLooper).postDelayed(this, 500)
            }
        })
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            "PhoneBoost worker",
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = "Trusted worker foreground service"
            setShowBadge(false)
        }
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    private fun workerNotification(): Notification {
        val openApp = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_phoneboost)
            .setContentTitle("PhoneBoost")
            .setContentText("Worker core: PAIRING_REQUIRED")
            .setContentIntent(openApp)
            .setOngoing(true)
            .setCategory(Notification.CATEGORY_SERVICE)
            .build()
    }
}
