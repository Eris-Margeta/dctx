package net.fetch

import kotlinx.coroutines.delay

suspend fun fetchUser(id: Int): String {
    delay(10)
    return "user-$id"
}

class Repository(private val api: Api) {
    suspend fun load(ids: List<Int>): List<String> = ids.map { api.get(it) }

    private suspend inline fun <R> retry(times: Int, block: suspend () -> R): R {
        repeat(times - 1) { runCatching { return block() } }
        return block()
    }
}

interface Api {
    suspend fun get(id: Int): String
}

val loader: suspend (Int) -> String = { fetchUser(it) }
