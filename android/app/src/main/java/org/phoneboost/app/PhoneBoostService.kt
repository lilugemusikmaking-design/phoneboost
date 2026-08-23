package org.phoneboost.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import android.util.Log

class PhoneBoostService : Service() {
    companion object {
        private const val CHANNEL_ID = "phoneboost_worker"
        private const val NOTIFICATION_ID = 4105
        private const val LOG_TAG = "PhoneBoostA5"

        @Volatile
        var isActive: Boolean = false
            private set
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
        Log.i(LOG_TAG, "FGS_STARTED state=PAIRING_REQUIRED")
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int = START_NOT_STICKY

    override fun onDestroy() {
        isActive = false
        WorkerNative.workerStop()
        stopForeground(STOP_FOREGROUND_REMOVE)
        Log.i(LOG_TAG, "WORKER_STOP")
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

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
