package org.phoneboost.app

internal object ControllerLeaseStateNameTest {
    @JvmStatic
    fun main(args: Array<String>) {
        val cases =
            listOf(
                0 to "NONE",
                1 to "ACTIVE",
                2 to "REVOKING",
                3 to "EXPIRED",
                Int.MIN_VALUE to "ERROR",
            )

        cases.forEachIndexed { index, (state, expected) ->
            val actual = controllerLeaseStateName(state)
            check(actual == expected) {
                "case ${index + 1}: state=$state expected=$expected actual=$actual"
            }
        }

        println("ControllerLeaseStateNameTest PASS (${cases.size}/${cases.size})")
    }
}
