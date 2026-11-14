package app.data

import kotlinx.coroutines.flow.Flow

/**
 * Stores users. Example: `class Fake : Repository<Phantom>`.
 */
@JvmSuppressWildcards
interface Repository<
    K : Comparable<K>,
    V : Any,
> {
    suspend fun find(key: K): V?

    fun observe(): Flow<List<V>>

    fun count(): Int = 0
}

@Suppress(
    "UNCHECKED_CAST",
    "unused",
)
internal class MemoryRepository<K : Comparable<K>, V : Any>(
    private val store: MutableMap<K, V> = mutableMapOf(),
) : Repository<K, V> {
    override suspend fun find(key: K): V? = store[key]

    override fun observe(): Flow<List<V>> =
        TODO("fun phantomInBody() is not real")

    private companion object Defaults {
        const val CAPACITY = 64
        object Limits {
            val max: Int = CAPACITY * 2
        }
    }
}
