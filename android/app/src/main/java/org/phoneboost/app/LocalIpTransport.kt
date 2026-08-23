package org.phoneboost.app

import android.content.Context
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.os.Build
import android.os.ParcelFileDescriptor
import android.util.Log
import java.net.Inet4Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.NetworkInterface
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketException
import java.util.Collections
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong

const val ACCESS_LOCAL_NETWORK_PERMISSION = "android.permission.ACCESS_LOCAL_NETWORK"
private const val LOG_TAG = "PhoneBoostC04"
private const val MAX_ACTIVE_STREAMS = 2
private const val PENDING_STREAMS = 2
private const val PROBE_MAX_BYTES = 64
private val DEBUG_D0_PROBE = "PHONEBOOST-C04-D0".toByteArray(Charsets.US_ASCII)

enum class LanPermissionState {
    NOT_REQUIRED_API_LT_37,
    GRANTED,
    DENIED,
}

enum class AndroidTransportState {
    UNAVAILABLE,
    LAN_UNAVAILABLE,
    LISTENING,
    CONNECTED_UNAUTHENTICATED,
    LOST,
}

data class AndroidTransportSnapshot(
    val state: AndroidTransportState,
    val permission: LanPermissionState,
    val address: String?,
    val port: Int?,
    val acceptedConnections: Long,
    val receivedBytes: Long,
    val transmittedBytes: Long,
) {
    companion object {
        fun unavailable(permission: LanPermissionState) = AndroidTransportSnapshot(
            state = if (permission == LanPermissionState.DENIED) {
                AndroidTransportState.LAN_UNAVAILABLE
            } else {
                AndroidTransportState.UNAVAILABLE
            },
            permission = permission,
            address = null,
            port = null,
            acceptedConnections = 0,
            receivedBytes = 0,
            transmittedBytes = 0,
        )
    }

    fun diagnosticEndpoint(): String = if (address != null && port != null) {
        "$address:$port"
    } else {
        "UNAVAILABLE"
    }
}

fun Context.lanPermissionState(): LanPermissionState = when {
    Build.VERSION.SDK_INT < 37 -> LanPermissionState.NOT_REQUIRED_API_LT_37
    checkSelfPermission(ACCESS_LOCAL_NETWORK_PERMISSION) == PackageManager.PERMISSION_GRANTED -> {
        LanPermissionState.GRANTED
    }
    else -> LanPermissionState.DENIED
}

/**
 * FGS-owned C04 TCP listener. It accepts only opaque byte streams and never
 * creates trust. The wildcard bind is required to accept existing-LAN peers;
 * the OS chooses the non-normative diagnostic port.
 */
class LocalIpTransport(private val context: Context) {
    private val pending = ArrayBlockingQueue<Socket>(PENDING_STREAMS)
    private val active = Collections.synchronizedSet(mutableSetOf<Socket>())
    private val acceptedConnections = AtomicLong(0)
    private val receivedBytes = AtomicLong(0)
    private val transmittedBytes = AtomicLong(0)
    private val threads = mutableListOf<Thread>()

    @Volatile
    private var running = false

    @Volatile
    private var server: ServerSocket? = null

    @Volatile
    private var current = AndroidTransportSnapshot.unavailable(context.lanPermissionState())

    fun snapshot(): AndroidTransportSnapshot = current.copy(
        acceptedConnections = acceptedConnections.get(),
        receivedBytes = receivedBytes.get(),
        transmittedBytes = transmittedBytes.get(),
    )

    fun start(): Boolean {
        if (running) return true
        val permission = context.lanPermissionState()
        if (permission == LanPermissionState.DENIED) {
            current = AndroidTransportSnapshot.unavailable(permission)
            Log.w(
                LOG_TAG,
                "C04_LISTENER state=LAN_UNAVAILABLE reason=TRANSPORT_PERMISSION_DENIED " +
                    "permission=DENIED",
            )
            return false
        }
        val listener = try {
            ServerSocket().apply {
                reuseAddress = true
                bind(InetSocketAddress(InetAddress.getByName("0.0.0.0"), 0), PENDING_STREAMS)
            }
        } catch (_: Exception) {
            current = AndroidTransportSnapshot.unavailable(permission)
            Log.e(LOG_TAG, "C04_LISTENER state=UNAVAILABLE reason=BIND_FAILED")
            return false
        }
        server = listener
        running = true
        val address = localLanIpv4()
        current = AndroidTransportSnapshot(
            state = AndroidTransportState.LISTENING,
            permission = permission,
            address = address,
            port = listener.localPort,
            acceptedConnections = 0,
            receivedBytes = 0,
            transmittedBytes = 0,
        )
        Log.i(
            LOG_TAG,
            "C04_LISTENER state=LISTENING type=LOCAL_IP ip=${address ?: "UNAVAILABLE"} " +
                "port=${listener.localPort} permission=$permission max_active=$MAX_ACTIVE_STREAMS " +
                "trust=NONE",
        )
        repeat(MAX_ACTIVE_STREAMS) { index ->
            startThread("phoneboost-c04-stream-$index") { workerLoop() }
        }
        startThread("phoneboost-c04-accept") { acceptLoop(listener) }
        return true
    }

    fun stop() {
        if (!running && server == null) return
        running = false
        try {
            server?.close()
        } catch (_: Exception) {
            // The listener is already unusable, which is the required stop state.
        }
        server = null
        while (true) {
            val queued = pending.poll() ?: break
            closeQuietly(queued)
        }
        synchronized(active) {
            active.forEach(::closeQuietly)
            active.clear()
        }
        threads.forEach(Thread::interrupt)
        threads.forEach { thread ->
            try {
                thread.join(250)
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
            }
        }
        threads.clear()
        current = current.copy(state = AndroidTransportState.UNAVAILABLE, port = null)
        Log.i(LOG_TAG, "C04_LISTENER state=UNAVAILABLE reason=FGS_STOP trust=NONE")
    }

    private fun startThread(name: String, action: () -> Unit) {
        Thread(action, name).apply {
            isDaemon = true
            threads.add(this)
            start()
        }
    }

    private fun acceptLoop(listener: ServerSocket) {
        while (running) {
            val socket = try {
                listener.accept()
            } catch (_: SocketException) {
                break
            } catch (_: Exception) {
                if (running) Log.e(LOG_TAG, "C04_ACCEPT_FAILED")
                continue
            }
            socket.tcpNoDelay = true
            if (!pending.offer(socket)) {
                closeQuietly(socket)
                Log.w(LOG_TAG, "C04_STREAM_REFUSED reason=BOUNDED_QUEUE_FULL")
            }
        }
    }

    private fun workerLoop() {
        while (running) {
            val socket = try {
                pending.poll(500, TimeUnit.MILLISECONDS)
            } catch (_: InterruptedException) {
                break
            } ?: continue
            active.add(socket)
            acceptedConnections.incrementAndGet()
            current = current.copy(state = AndroidTransportState.CONNECTED_UNAUTHENTICATED)
            Log.i(LOG_TAG, "C04_STREAM state=CONNECTED_UNAUTHENTICATED trust=NONE")
            try {
                if (isDebuggable()) {
                    runBoundedDebugOrSecure(socket)
                } else {
                    runSecureSession(socket, null)
                }
            } catch (_: Exception) {
                // Loss is represented only as transport loss; bytes are never inspected.
            } finally {
                active.remove(socket)
                closeQuietly(socket)
                current = current.copy(
                    state = when {
                        !running -> AndroidTransportState.LOST
                        active.isNotEmpty() -> AndroidTransportState.CONNECTED_UNAUTHENTICATED
                        else -> AndroidTransportState.LISTENING
                    },
                )
                Log.i(LOG_TAG, "C04_STREAM state=LOST next=LISTENING trust=NONE")
            }
        }
    }

    private fun runBoundedDebugOrSecure(socket: Socket) {
        val prefix = ByteArray(2)
        val input = socket.getInputStream()
        if (input.read(prefix, 0, 2) != 2) return
        if (prefix.contentEquals(DEBUG_D0_PROBE.copyOfRange(0, 2))) {
            val bytes = ByteArray(DEBUG_D0_PROBE.size)
            prefix.copyInto(bytes)
            var offset = 2
            while (offset < bytes.size) {
                val read = input.read(bytes, offset, bytes.size - offset)
                if (read <= 0) return
                offset += read
            }
            if (!bytes.contentEquals(DEBUG_D0_PROBE) || bytes.size > PROBE_MAX_BYTES) return
            receivedBytes.addAndGet(bytes.size.toLong())
            socket.getOutputStream().write(bytes)
            socket.getOutputStream().flush()
            transmittedBytes.addAndGet(bytes.size.toLong())
            return
        }
        runSecureSession(socket, prefix)
    }

    private fun runSecureSession(socket: Socket, prefix: ByteArray?) {
        val detached = try {
            ParcelFileDescriptor.fromSocket(socket).detachFd()
        } catch (_: Exception) {
            Log.e(LOG_TAG, "C05_SECURE state=LOST reason=DEVICE_LOST")
            return
        }
        val result = WorkerNative.secureAcceptFd(
            detached,
            prefix?.get(0)?.toInt()?.and(0xff) ?: -1,
            prefix?.get(1)?.toInt()?.and(0xff) ?: -1,
        )
        Log.i(
            LOG_TAG,
            if (result == WorkerNative.RESULT_OK) {
                "C05_SECURE state=LOST reason=SESSION_CLOSED"
            } else {
                "C05_SECURE state=LOST reason=SECURE_SESSION_FAILED"
            },
        )
    }

    private fun isDebuggable(): Boolean =
        context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0

    private fun closeQuietly(socket: Socket) {
        try {
            socket.close()
        } catch (_: Exception) {
            // Closing is best effort; the stream is never reused.
        }
    }
}

private fun localLanIpv4(): String? {
    val candidates = mutableListOf<Pair<Int, String>>()
    val interfaces = try {
        NetworkInterface.getNetworkInterfaces()?.toList().orEmpty()
    } catch (_: SocketException) {
        return null
    }
    for (network in interfaces) {
        val usable = try {
            network.isUp && !network.isLoopback
        } catch (_: SocketException) {
            false
        }
        if (!usable) continue
        for (address in network.inetAddresses.toList()) {
            if (address is Inet4Address && address.isSiteLocalAddress && !address.isLoopbackAddress) {
                val priority = if (network.name.startsWith("wlan")) 0 else 1
                candidates.add(priority to address.hostAddress.orEmpty())
            }
        }
    }
    return candidates.sortedWith(compareBy<Pair<Int, String>> { it.first }.thenBy { it.second })
        .firstOrNull()?.second
}
