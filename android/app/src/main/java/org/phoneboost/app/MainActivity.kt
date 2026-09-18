package org.phoneboost.app

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

class MainActivity : Activity() {
    companion object {
        private const val LOG_TAG = "PhoneBoostA6"
        private const val LOCAL_NETWORK_PERMISSION_REQUEST = 4106
        private const val PREFERENCES = "phoneboost-ui"
        private const val PARTICIPATION = "participation-enabled"
        private const val LANGUAGE = "language"
        private val GREEN = Color.rgb(99, 255, 25)
        private val BACKGROUND = Color.rgb(6, 9, 13)
        private val PANEL = Color.rgb(11, 17, 23)
        private val PANEL_ALT = Color.rgb(14, 20, 28)
        private val BORDER = Color.rgb(31, 43, 54)
        private val PRIMARY = Color.rgb(242, 245, 247)
        private val SECONDARY = Color.rgb(155, 165, 178)
        private val MUTED = Color.rgb(109, 120, 134)
    }

    private lateinit var heroRuntimeStatusView: TextView
    private lateinit var pairingSasView: TextView
    private lateinit var runtimeStatusView: TextView
    private lateinit var connectionView: TextView
    private lateinit var modeView: TextView
    private lateinit var switchView: TextView
    private lateinit var advancedContainer: LinearLayout
    private lateinit var advancedStatusView: TextView
    private val handler = Handler(Looper.getMainLooper())
    private val refresh = object : Runnable {
        override fun run() {
            refreshStatus()
            handler.postDelayed(this, 2_000)
        }
    }

    private val preferences by lazy { getSharedPreferences(PREFERENCES, MODE_PRIVATE) }
    private var participationEnabled: Boolean
        get() = preferences.getBoolean(PARTICIPATION, true)
        set(value) = preferences.edit().putBoolean(PARTICIPATION, value).apply()
    private var language: String
        get() = preferences.getString(LANGUAGE, "fr")?.takeIf { it == "fr" || it == "en" } ?: "fr"
        set(value) = preferences.edit().putString(LANGUAGE, value).apply()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildContent())
        requestNotificationPermission()
        requestLocalNetworkPermission()
        applyParticipationPreference()
        handler.postDelayed(refresh, 350)
        Log.i(LOG_TAG, "UI_CREATED")
    }

    override fun onResume() {
        super.onResume()
        handler.removeCallbacks(refresh)
        handler.postDelayed(refresh, 150)
    }

    override fun onPause() {
        handler.removeCallbacks(refresh)
        super.onPause()
    }

    override fun onDestroy() {
        handler.removeCallbacksAndMessages(null)
        super.onDestroy()
    }

    private fun text(fr: String, en: String): String = if (language == "en") en else fr

    private fun buildContent(): View {
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(18), dp(24), dp(18), dp(22))
        }
        content.addView(buildHeader())
        content.addView(buildHero(), margin(top = 24))
        content.addView(buildStatusCard(), margin(top = 16))
        content.addView(buildAdvancedCard(), margin(top = 16))
        content.addView(buildBottomNavigation(), margin(top = 20))
        return ScrollView(this).apply {
            setBackgroundColor(BACKGROUND)
            isFillViewport = true
            addView(content, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        }
    }

    private fun buildHeader(): View = LinearLayout(this).apply {
        gravity = Gravity.CENTER_VERTICAL
        addView(label("PB", 22f, Color.BLACK, true).apply {
            gravity = Gravity.CENTER
            background = rounded(GREEN, 16f)
        }, LinearLayout.LayoutParams(dp(62), dp(62)))
        addView(LinearLayout(this@MainActivity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(14), 0, 0, 0)
            addView(label("PhoneBoost Worker", 24f, PRIMARY, true))
            addView(label(text("Worker Android", "Android worker"), 15f, MUTED))
        }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        addView(languageButton("FR", "fr"), LinearLayout.LayoutParams(dp(50), dp(44)))
        addView(languageButton("EN", "en"), LinearLayout.LayoutParams(dp(50), dp(44)).apply { leftMargin = dp(4) })
    }

    private fun languageButton(caption: String, value: String): TextView = label(
        caption,
        14f,
        if (language == value) Color.BLACK else SECONDARY,
        true,
    ).apply {
        gravity = Gravity.CENTER
        background = rounded(if (language == value) GREEN else PANEL_ALT, 10f, BORDER)
        setOnClickListener {
            if (language != value) {
                language = value
                setContentView(buildContent())
                refreshStatus()
            }
        }
    }

    private fun buildHero(): View = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(24), dp(26), dp(24), dp(26))
        background = rounded(PANEL, 20f, BORDER)
        addView(label(text("CALCUL DISTRIBUÉ\nUN NUMÉRIQUE PLUS DURABLE", "DISTRIBUTED COMPUTING\nA GREENER TOMORROW"), 11f, MUTED).apply {
            letterSpacing = .16f
            setLineSpacing(0f, 1.35f)
        })
        addView(View(this@MainActivity).apply { setBackgroundColor(GREEN) }, LinearLayout.LayoutParams(dp(42), dp(3)).apply { topMargin = dp(18) })
        addView(label("PhoneBoost", 43f, PRIMARY, true), margin(top = 24))
        addView(label(if (participationEnabled) text("activé", "activated") else text("désactivé", "disabled"), 43f, if (participationEnabled) GREEN else SECONDARY, true))
        addView(label(text("Ce téléphone peut aider votre ordinateur lorsque le runtime l’autorise.", "This phone can help your computer when the runtime permits it."), 19f, PRIMARY), margin(top = 16))
        switchView = label("", 23f, Color.BLACK, true).apply {
            gravity = Gravity.CENTER
            contentDescription = "PhoneBoost participation switch"
            setOnClickListener {
                participationEnabled = !participationEnabled
                applyParticipationPreference()
                setContentView(buildContent())
                refreshStatus()
            }
        }
        updateSwitch()
        addView(switchView, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(76)).apply { topMargin = dp(30) })
        heroRuntimeStatusView = label(text("INITIALISATION", "INITIALIZING"), 14f, SECONDARY, true).apply {
            gravity = Gravity.CENTER
            letterSpacing = .16f
            background = rounded(Color.rgb(14, 31, 20), 24f, Color.rgb(24, 66, 33))
        }
        addView(heroRuntimeStatusView, LinearLayout.LayoutParams(dp(210), dp(50)).apply { gravity = Gravity.CENTER_HORIZONTAL; topMargin = dp(16) })
        pairingSasView = label("", 18f, PRIMARY, true).apply {
            gravity = Gravity.CENTER
            contentDescription = "Pairing code"
            visibility = View.GONE
        }
        addView(pairingSasView, margin(top = 12))
        addView(label("ⓘ  " + text("Autorisé par défaut · statut runtime séparé", "Enabled by default · separate runtime status"), 13f, MUTED).apply { gravity = Gravity.CENTER }, margin(top = 14))
    }

    private fun updateSwitch() {
        switchView.text = if (participationEnabled) "ON       ●" else "●       OFF"
        switchView.setTextColor(if (participationEnabled) Color.BLACK else SECONDARY)
        switchView.background = rounded(if (participationEnabled) GREEN else PANEL_ALT, 38f, if (participationEnabled) GREEN else BORDER)
    }

    private fun buildStatusCard(): View = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(22), dp(22), dp(22), dp(12))
        background = rounded(PANEL, 20f, BORDER)
        addView(LinearLayout(this@MainActivity).apply {
            gravity = Gravity.CENTER_VERTICAL
            addView(label(text("Statut", "Status"), 26f, PRIMARY, true), LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            addView(label("●  " + text("VÉRIFIÉ", "OBSERVED"), 11f, GREEN).apply { letterSpacing = .12f })
        })
        runtimeStatusView = statusValue(text("Initialisation", "Initializing"), GREEN)
        addView(statusRow("◉", text("Runtime", "Runtime"), runtimeStatusView))
        connectionView = statusValue(text("Indisponible", "Unavailable"), PRIMARY)
        addView(statusRow("↗", text("Connexion", "Connection"), connectionView))
        modeView = statusValue(if (participationEnabled) text("Autorisé", "Enabled") else text("Désactivé", "Disabled"), if (participationEnabled) GREEN else SECONDARY)
        addView(statusRow("⚙", text("Mode", "Mode"), modeView))
    }

    private fun statusValue(value: String, color: Int): TextView = label(value, 15f, color, true).apply {
        gravity = Gravity.END
    }

    private fun statusRow(icon: String, title: String, value: TextView): View = LinearLayout(this).apply {
        gravity = Gravity.CENTER_VERTICAL
        setPadding(dp(4), dp(18), dp(4), dp(18))
        addView(label(icon, 22f, SECONDARY), LinearLayout.LayoutParams(dp(42), ViewGroup.LayoutParams.WRAP_CONTENT))
        addView(label(title, 16f, SECONDARY), LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        addView(value, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        contentDescription = "$title: ${value.text}"
    }

    private fun buildAdvancedCard(): View = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(22), dp(18), dp(22), dp(18))
        background = rounded(PANEL, 20f, BORDER)
        val header = label("▥   " + text("Détails avancés", "Advanced details") + "                                      ⌄", 18f, PRIMARY, true).apply {
            contentDescription = "Advanced details toggle"
        }
        addView(header)
        addView(label(text("Portes de confiance, ressources locales et distantes, diagnostics", "Trust gates, local and remote resources, diagnostics"), 13f, MUTED), margin(top = 6))
        advancedContainer = LinearLayout(this@MainActivity).apply {
            orientation = LinearLayout.VERTICAL
            visibility = View.GONE
            setPadding(0, dp(18), 0, 0)
        }
        advancedStatusView = label(text("Observations indisponibles", "Observations unavailable"), 13f, SECONDARY).apply {
            setTextIsSelectable(true)
            typeface = Typeface.MONOSPACE
            setLineSpacing(dp(2).toFloat(), 1f)
        }
        advancedContainer.addView(advancedStatusView)
        advancedContainer.addView(buildPairingActions(), margin(top = 18))
        addView(advancedContainer)
        header.setOnClickListener { advancedContainer.visibility = if (advancedContainer.visibility == View.VISIBLE) View.GONE else View.VISIBLE }
    }

    private fun buildPairingActions(): View = LinearLayout(this).apply {
        orientation = LinearLayout.HORIZONTAL
        listOf(Triple(text("CONFIRMER", "CONFIRM"), 0, text("Confirmer l’appairage", "Confirm pairing")), Triple(text("ANNULER", "CANCEL"), 1, text("Annuler l’appairage", "Cancel pairing")), Triple(text("REFUSER", "MISMATCH"), 2, text("Refuser le code", "Reject pairing code"))).forEachIndexed { index, (caption, action, description) ->
            addView(Button(this@MainActivity).apply {
                this.text = caption
                textSize = 10f
                contentDescription = description
                setOnClickListener { WorkerNative.secureAction(action); refreshStatus() }
            }, LinearLayout.LayoutParams(0, dp(48), 1f).apply { if (index > 0) leftMargin = dp(6) })
        }
    }

    private fun buildBottomNavigation(): View = LinearLayout(this).apply {
        gravity = Gravity.CENTER
        val items = listOf("⌂\n" + text("Statut", "Status"), "▥\n" + text("Activité", "Activity"), "•••\n" + text("Plus", "More"))
        items.forEachIndexed { index, item -> addView(label(item, 14f, if (index == 0) GREEN else MUTED, index == 0).apply { gravity = Gravity.CENTER }, LinearLayout.LayoutParams(0, dp(62), 1f)) }
    }

    private fun applyParticipationPreference() {
        if (participationEnabled) {
            startForegroundService(Intent(this, PhoneBoostService::class.java))
        } else {
            stopService(Intent(this, PhoneBoostService::class.java))
        }
    }

    private fun refreshStatus() {
        if (!::runtimeStatusView.isInitialized) return
        val enabled = participationEnabled
        val observations = readAndroidObservations()
        val transport = PhoneBoostService.transportSnapshot()
        val secureStateCode = WorkerNative.secureState()
        val secureState = secureStateName(secureStateCode)
        val authenticated = WorkerNative.secureField(0) == 1L
        val lease = controllerLeaseStateName(WorkerNative.workerAuthorityState(0))
        val resourceGuard = if (WorkerNative.workerAuthorityState(1) == 1) "ACTIVE" else "UNAVAILABLE"
        val status = when {
            !enabled -> text("DÉSACTIVÉ", "DISABLED")
            !PhoneBoostService.isActive -> text("INITIALISATION", "INITIALIZING")
            secureStateCode == WorkerNative.SECURE_UNPAIRED -> text("APPAIRAGE REQUIS", "PAIRING REQUIRED")
            !authenticated -> text("EN ATTENTE", "WAITING")
            else -> text("AUTHENTIFIÉ", "AUTHENTICATED")
        }
        // Provider readiness is not exposed here, so this screen never infers READY.
        heroRuntimeStatusView.text = status
        val pairingDisplay = if (secureStateCode == WorkerNative.SECURE_SAS_PENDING) {
            pairingSasDisplay(WorkerNative.secureSas(), language)
        } else {
            null
        }
        pairingSasView.text = pairingDisplay.orEmpty()
        pairingSasView.visibility = if (pairingDisplay == null) View.GONE else View.VISIBLE
        runtimeStatusView.text = status
        connectionView.text = if (authenticated) "AUTHENTICATED" else transport.state.toString()
        modeView.text = if (enabled) text("Autorisé", "Enabled") else text("Désactivé", "Disabled")
        if (::advancedStatusView.isInitialized) {
            advancedStatusView.text = buildString {
                appendLine("LOCAL · ANDROID WORKER")
                appendLine("Service: ${if (PhoneBoostService.isActive) "ACTIVE" else "INACTIVE"}")
                appendLine("Thermal: ${observations.thermal}")
                appendLine("Available memory observation: ${observations.availableMemoryMib} MiB")
                appendLine("ResourceGuard: $resourceGuard")
                appendLine()
                appendLine("REMOTE · CONTROLLER")
                appendLine("1  Paired: $secureState")
                appendLine("2  Authenticated: ${if (authenticated) "AUTHENTICATED" else "NOT_AUTHENTICATED"}")
                appendLine("3  Controller lease: $lease")
                appendLine("4  Resource admissible: $resourceGuard")
                appendLine("5  Provider ready: UNAVAILABLE")
                append("Transport: ${if (authenticated) "AUTHENTICATED" else transport.state}")
            }
        }
        Log.i(LOG_TAG, "UI_STATUS preference=${if (enabled) "ON" else "OFF"} runtime=$status secure=$secureState lease=$lease provider=UNAVAILABLE")
    }

    private fun label(value: String, size: Float, color: Int, bold: Boolean = false): TextView = TextView(this).apply {
        text = value
        textSize = size
        setTextColor(color)
        if (bold) setTypeface(typeface, Typeface.BOLD)
        includeFontPadding = false
    }

    private fun rounded(fill: Int, radius: Float, stroke: Int? = null): GradientDrawable = GradientDrawable().apply {
        shape = GradientDrawable.RECTANGLE
        setColor(fill)
        cornerRadius = dp(radius.toInt()).toFloat()
        if (stroke != null) setStroke(dp(1), stroke)
    }

    private fun margin(left: Int = 0, top: Int = 0): LinearLayout.LayoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
        leftMargin = dp(left)
        topMargin = dp(top)
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    private fun secureStateName(value: Int): String = when (value) {
        WorkerNative.SECURE_UNPAIRED -> "UNPAIRED"
        WorkerNative.SECURE_PAIRING_XX -> "PAIRING_XX"
        WorkerNative.SECURE_SAS_PENDING -> "SAS_PENDING"
        WorkerNative.SECURE_LOCAL_CONFIRMED -> "LOCAL_CONFIRMED"
        WorkerNative.SECURE_PEER_CONFIRMED -> "PEER_CONFIRMED"
        WorkerNative.SECURE_MUTUAL_CONFIRMED -> "MUTUAL_CONFIRMED"
        WorkerNative.SECURE_TRUST_COMMITTING -> "TRUST_COMMITTING"
        WorkerNative.SECURE_COMMITTED_WAITING_PEER -> "COMMITTED_WAITING_PEER"
        WorkerNative.SECURE_PAIRED -> "PAIRED"
        WorkerNative.SECURE_AUTHENTICATED -> "AUTHENTICATED"
        WorkerNative.SECURE_PAIR_REJECTED -> "PAIR_REJECTED"
        WorkerNative.SECURE_PAIRING_FAILED -> "PAIRING_FAILED"
        WorkerNative.SECURE_COOLDOWN -> "PAIRING_COOLDOWN"
        else -> "UNAVAILABLE"
    }

    private fun requestNotificationPermission() {
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 4105)
        }
    }

    private fun requestLocalNetworkPermission() {
        if (Build.VERSION.SDK_INT >= 37 && checkSelfPermission(ACCESS_LOCAL_NETWORK_PERMISSION) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(ACCESS_LOCAL_NETWORK_PERMISSION), LOCAL_NETWORK_PERMISSION_REQUEST)
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == LOCAL_NETWORK_PERMISSION_REQUEST && grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED && participationEnabled) {
            startForegroundService(Intent(this, PhoneBoostService::class.java))
        }
    }
}
