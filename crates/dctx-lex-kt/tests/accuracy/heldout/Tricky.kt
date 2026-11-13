package edge.cases

@Target(AnnotationTarget.CLASS, AnnotationTarget.FUNCTION)
@Retention(AnnotationRetention.RUNTIME)
annotation class Marker(val names: Array<String> = ["a", "b"])

class Registry {
    val index: Map<String, List<Pair<Int, Set<String>>>> = emptyMap()

    @Marker(names = ["x"])
    fun lookup(key: String): Int = when (val hit = index[key]) {
        null -> -1
        else -> hit.size
    }

    fun render(): String = "${"}" + "{ fun phantomBrace() }"}${index.size}"

    fun scan(rows: List<List<Int>>): Int {
        outer@ for (row in rows) {
            for (cell in row) if (cell < 0) continue@outer
        }
        return rows.size
    }

    companion object {
        val EMPTY = Registry()

        @JvmStatic
        fun create(): Registry = EMPTY
    }
}

fun <T> T.tap(block: (T) -> Unit): T {
    block(this)
    return this
}

private val cache: MutableMap<String, () -> Int> =
    mutableMapOf("c" to { 'c'.code })

internal object Ids {
    private var next = 0L
    @Synchronized fun take(): Long = next++
}

class Wrapper<T : Comparable<T>>(private val items: List<T>) : Iterable<T> by items {
    fun max(): T? = items.maxOrNull()
}
