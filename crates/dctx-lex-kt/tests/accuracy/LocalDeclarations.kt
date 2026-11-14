package local

fun process(input: List<Int>): Int {
    fun square(x: Int) = x * x

    class Acc(var total: Int = 0) {
        fun add(v: Int) {
            total += v
        }
    }

    val acc = Acc()
    for (v in input) acc.add(square(v))
    return acc.total
}

class Parser {
    fun parse(text: String): List<String> {
        fun isSep(c: Char) = c == ',' || c == ';'
        val out = mutableListOf<String>()
        text.split { isSep(it) }
        return out
    }
}

private fun String.split(pred: (Char) -> Boolean): List<String> = listOf(this)
