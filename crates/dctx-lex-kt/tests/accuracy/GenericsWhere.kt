package generics

interface Repo<T : Any, in K> {
    fun find(key: K): T?
}

class Cache<K : Comparable<K>, V>(private val capacity: Int) where V : Any, V : java.io.Serializable {
    private val map = sortedMapOf<K, V>()

    fun <R> mapValues(transform: (V) -> R): Map<K, R> = map.mapValues { transform(it.value) }
}

fun <T> copyWhenGreater(list: List<T>, threshold: T): List<String>
    where T : CharSequence,
          T : Comparable<T> {
    return list.filter { it > threshold }.map { it.toString() }
}

class Producer<out T>(private val value: T) {
    fun get(): T = value
}

fun <T : Comparable<in T>> maxOf3(a: T, b: T, c: T): T = maxOf(a, maxOf(b, c))

val emptyMapOfLists: Map<String, List<Pair<Int, Map<String, Int>>>> = emptyMap()
