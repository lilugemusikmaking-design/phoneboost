package org.phoneboost.app

internal fun controllerLeaseStateName(state: Int): String =
    when (state) {
        0 -> "NONE"
        1 -> "ACTIVE"
        2 -> "REVOKING"
        3 -> "EXPIRED"
        else -> "ERROR"
    }
