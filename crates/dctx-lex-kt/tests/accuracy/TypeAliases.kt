package aliases

typealias Callback = (Result<String>) -> Unit

typealias StringMap<V> = Map<String, V>

internal typealias Matrix = Array<IntArray>

typealias Predicate<T> =
    (T) -> Boolean

class Node {
    fun children(): StringMap<Node> = emptyMap()
}

fun accept(callback: Callback, filter: Predicate<String>) {}
