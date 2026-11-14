package text.props

val String.lastChar: Char
    get() = this[length - 1]

var StringBuilder.lastChar: Char
    get() = this[length - 1]
    set(value) {
        setCharAt(length - 1, value)
    }

val <T> List<T>.penultimate: T?
    get() = if (size >= 2) this[size - 2] else null

class Box(val width: Int, val height: Int)

val Box.area get() = width * height

object Units {
    val Int.kb: Long get() = this * 1024L
}
