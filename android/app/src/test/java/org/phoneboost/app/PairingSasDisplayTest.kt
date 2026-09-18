package org.phoneboost.app

internal object PairingSasDisplayTest {
    @JvmStatic
    fun main(args: Array<String>) {
        check(pairingSasDisplay(42, "en") == "Pairing code: 000042")
        check(pairingSasDisplay(987_654, "fr") == "Code d’appairage : 987654")
        check(pairingSasDisplay(-1, "en") == null)
        check(pairingSasDisplay(1_000_000, "fr") == null)
        println("PairingSasDisplayTest PASS (4/4)")
    }
}
