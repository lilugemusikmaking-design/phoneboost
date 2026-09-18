package org.phoneboost.app

import java.util.Locale

internal fun pairingSasDisplay(sas: Int, language: String): String? {
    if (sas !in 0..999_999) return null
    val code = String.format(Locale.ROOT, "%06d", sas)
    return if (language == "en") "Pairing code: $code" else "Code d’appairage : $code"
}
