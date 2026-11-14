package documented

/**
 * A cache keyed by string.
 *
 * Example:
 * ```kotlin
 * class PhantomKdocExample {
 *     fun phantomKdocFun() = 1
 * }
 * ```
 * @param T the value type
 */
class Cache<T> {
    /** Returns the cached value, or computes it via `fun compute()`. */
    fun get(key: String, compute: () -> T): T = compute()

    /**
     * Drops everything.
     * @see Cache.get
     */
    fun clear() {}
}

/** Top-level helper; see [Cache]. object PhantomInKdoc */
fun newCache(): Cache<String> = Cache()
