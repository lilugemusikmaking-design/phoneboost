package org.phoneboost.app

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.util.Log
import java.security.SecureRandom

private const val DNS_SD_LOG_TAG = "PhoneBoostDiscovery"
private const val PHONEBOOST_SERVICE_TYPE = "_phoneboost._tcp"

/**
 * Runtime-scoped DNS-SD advertisement. The service record is only an
 * attacker-visible transport hint; it contains no identity or authority data.
 */
class PhoneBoostDnsSdRegistration(
    context: Context,
    private val onRegistrationFailure: () -> Unit,
) {
    private val manager = context.getSystemService(NsdManager::class.java)

    @Volatile
    private var listener: NsdManager.RegistrationListener? = null

    fun start(port: Int): Boolean {
        if (listener != null || port !in 1..65535) return false
        val service = NsdServiceInfo().apply {
            serviceName = runtimeInstanceName()
            serviceType = PHONEBOOST_SERVICE_TYPE
            setPort(port)
            // V0 intentionally publishes no TXT attributes.
        }
        val registrationListener = object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(serviceInfo: NsdServiceInfo) {
                if (listener !== this) return
                Log.i(
                    DNS_SD_LOG_TAG,
                    "DNS_SD state=AVAILABLE type=$PHONEBOOST_SERVICE_TYPE " +
                        "instance=${serviceInfo.serviceName} port=$port trust=NONE txt=NONE",
                )
            }

            override fun onRegistrationFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
                if (listener !== this) return
                listener = null
                Log.e(
                    DNS_SD_LOG_TAG,
                    "DNS_SD state=UNAVAILABLE reason=DISCOVERY_BACKEND_UNAVAILABLE " +
                        "error=$errorCode trust=NONE",
                )
                onRegistrationFailure()
            }

            override fun onServiceUnregistered(serviceInfo: NsdServiceInfo) {
                if (listener === this) listener = null
                Log.i(DNS_SD_LOG_TAG, "DNS_SD state=UNAVAILABLE reason=TRANSPORT_STOPPED")
            }

            override fun onUnregistrationFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
                if (listener === this) listener = null
                Log.e(
                    DNS_SD_LOG_TAG,
                    "DNS_SD state=UNAVAILABLE reason=UNREGISTER_FAILED error=$errorCode",
                )
            }
        }
        listener = registrationListener
        return try {
            manager.registerService(
                service,
                NsdManager.PROTOCOL_DNS_SD,
                registrationListener,
            )
            true
        } catch (_: RuntimeException) {
            listener = null
            Log.e(
                DNS_SD_LOG_TAG,
                "DNS_SD state=UNAVAILABLE reason=DISCOVERY_BACKEND_UNAVAILABLE trust=NONE",
            )
            false
        }
    }

    fun stop() {
        val registrationListener = listener ?: return
        listener = null
        try {
            manager.unregisterService(registrationListener)
        } catch (_: RuntimeException) {
            Log.e(DNS_SD_LOG_TAG, "DNS_SD state=UNAVAILABLE reason=UNREGISTER_FAILED")
        }
    }

    private fun runtimeInstanceName(): String {
        val random = ByteArray(4)
        SecureRandom().nextBytes(random)
        val suffix = random.joinToString(separator = "") { byte -> "%02x".format(byte.toInt() and 0xff) }
        return "PhoneBoost-$suffix"
    }
}
