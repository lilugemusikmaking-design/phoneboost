package org.phoneboost.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject

// Compiled only with -PphoneboostMulticoreSpike=true, under a distinct APK ID.
// DUMP is a signature/development permission available to the authorized ADB
// shell, not ordinary apps. This receiver never handles a network command.
object MulticoreNative {
    init { System.loadLibrary("phoneboost_core_jni") }
    external fun configure(threads: Int): Int
    external fun field(field: Int): Long
}

class MulticoreProfileReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.hasExtra("threads")) {
            val threads = intent.getIntExtra("threads", -1)
            val result = if (threads in listOf(1, 2, 4)) MulticoreNative.configure(threads) else -5
            resultCode = result
            resultData = "configured=$threads result=$result"
            Log.i("PhoneBoostMC", resultData!!)
            return
        }
        val names = listOf("sequence", "configured_threads", "input_bytes", "wall_ns", "cpu_ns",
            "peak_hash_scopes", "executed_threads", "cpu_mask", "refusal", "outcome",
            "scratch_bytes", "held_before", "held_after", "safety_before", "safety_after",
            "thermal_before", "thermal_after", "age_before_ms", "age_after_ms", "available_bytes",
            "charging", "power_save", "budget_bytes", "battery_percent")
        val record = JSONObject()
        record.put("probe_token", intent.getStringExtra("probe_token") ?: "")
        names.forEachIndexed { index, name -> record.put(name, MulticoreNative.field(index)) }
        val threads = JSONArray()
        val threadNames = listOf("tid", "begin_ns", "end_ns", "cpu_ns", "cpu_mask",
            "observed_migrations", "bytes", "cpu_id_overflow", "worker_index")
        repeat(record.getLong("executed_threads").coerceIn(0, 4).toInt()) { index ->
            val thread = JSONObject()
            threadNames.forEachIndexed { field, name -> thread.put(name, MulticoreNative.field(100 + index * 10 + field)) }
            threads.put(thread)
        }
        record.put("threads", threads)
        // A concurrent new compute must never produce a falsely coherent record.
        record.put("coherent", record.getLong("sequence") == MulticoreNative.field(0))
        Log.i("PhoneBoostMC", "MC_JOB $record")
        resultCode = 0
    }
}
