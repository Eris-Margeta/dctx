package lambdas

val handlers: Map<String, (Int) -> Int> = mapOf(
    "double" to { x -> x * 2 },
    "square" to fun(x: Int): Int { return x * x },
)

fun apply(op: String, value: Int): Int {
    val f = handlers[op] ?: { it }
    return f(value)
}

val sorter = Comparator<String> { a, b -> a.length - b.length }

fun runAll(vararg blocks: () -> Unit) {
    blocks.forEach { it() }
}

class Button {
    var onClick: (() -> Unit)? = null

    fun click() {
        onClick?.invoke()
    }
}
