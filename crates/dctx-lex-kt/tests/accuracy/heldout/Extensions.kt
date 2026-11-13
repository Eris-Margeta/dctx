@file:JvmName("CollectionsKt2")

package util.ext

inline fun <reified T : Any> List<*>.firstOfType(): T? =
    firstOrNull { it is T } as T?

fun <K, V> Map<K, List<V>>.flattenValues(): List<V> = values.flatten()

val <T> List<T>.secondOrNull: T?
    get() = getOrNull(1)

var StringBuilder.lastChar: Char
    get() = this[length - 1]
    set(value) {
        setCharAt(length - 1, value)
    }

infix fun Int.clampTo(range: IntRange): Int = coerceIn(range)

operator fun String.times(n: Int): String = repeat(n)

/* fun phantomBlock() { /* nested fun phantomNested() */ } */
private tailrec fun gcd(a: Int, b: Int): Int = if (b == 0) a else gcd(b, a % b)
