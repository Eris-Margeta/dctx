package control

fun firstEven(matrix: List<List<Int>>): Int? {
    outer@ for (row in matrix) {
        for (v in row) {
            if (v % 2 == 0) return v
            if (v < 0) continue@outer
        }
    }
    return null
}

fun kind(x: Any): String = when (x) {
    is Int -> "int"
    is String -> if (x.isEmpty()) "empty" else "string"
    in listOf<Any>() -> "listed"
    else -> "other"
}

class Machine {
    var state = 0

    fun step() = when {
        state > 10 -> state = 0
        else -> state++
    }
}

fun after() = Unit
