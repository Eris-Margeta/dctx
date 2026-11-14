package text.ext

fun String.isPalindrome(): Boolean = this == reversed()

fun <T> List<T>.secondOrNull(): T? = if (size > 1) this[1] else null

private fun StringBuilder.appendLineIndented(text: String, depth: Int = 1) =
    append("    ".repeat(depth)).append(text).append('\n')

fun Int?.orZero(): Int = this ?: 0

class Formatter {
    fun String.bold(): String = "**$this**"

    fun render(title: String): String = title.bold()
}

fun Map<String, List<Int>>.totals(): Map<String, Int> =
    mapValues { (_, values) -> values.sum() }
