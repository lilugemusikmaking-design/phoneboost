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
    external fun traceControl(enabled: Int): Int
    external fun traceCapture(): Int
    external fun traceField(row: Int, field: Int): Long
}

// Diagnostic build only; DUMP permission confines control to the authorized shell.
class TimingDiagnosticReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.hasExtra("enabled")) {
            resultCode = TimingNative.control(intent.getIntExtra("enabled", -1))
            if (resultCode == 0) {
                resultCode = TimingNative.traceControl(
                    if (intent.getIntExtra("enabled", 0) == 1) intent.getIntExtra("put_trace", 0) else 0,
                )
            }
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
        if (intent.getBooleanExtra("put_trace", false)) {
            val eventCount = TimingNative.traceCapture()
            if (eventCount !in 0..32768) { resultCode = -1; return }
            val events = JSONArray()
            repeat(eventCount) { index ->
                val row = JSONArray()
                repeat(6) { field -> row.put(TimingNative.traceField(index, field)) }
                events.put(row)
            }
            // Post-run export into app-private storage, never the PBMUX wire or logcat.
            val trace = JSONObject().put("schema", 1)
                .put("dropped", TimingNative.traceField(-1, 0)).put("rows", events)
            try {
                java.io.File(context.filesDir, "put-timing.json").writeText(trace.toString())
            } catch (_: java.io.IOException) { resultCode = -1; return }
        }
        resultCode = 0
    }
}
