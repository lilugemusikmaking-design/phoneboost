package org.phoneboost.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject

object TimingNative {
    init { System.loadLibrary("phoneboost_core_jni") }
    external fun control(enabled: Int): Int
    external fun capture(): Int
    external fun field(phase: Int, field: Int): Long
}

// Diagnostic build only; DUMP permission confines control to the authorized shell.
class TimingDiagnosticReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.hasExtra("enabled")) {
            resultCode = TimingNative.control(intent.getIntExtra("enabled", -1))
            resultData = "timing_control=$resultCode"
            return
        }
        val count = TimingNative.capture()
        if (count !in 1..128) { resultCode = -1; return }
        val rows = JSONArray()
        repeat(count) { phase ->
            val row = JSONArray()
            repeat(3) { field -> row.put(TimingNative.field(phase, field)) }
            rows.put(row)
        }
        val record = JSONObject().put("schema", 1).put("metrics", rows)
        Log.i("PhoneBoostTiming", "TIMING_JOB $record")
        resultCode = 0
    }
}
