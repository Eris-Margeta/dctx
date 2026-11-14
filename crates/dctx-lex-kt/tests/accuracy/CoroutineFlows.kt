package flows

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.map

class Ticker(private val periodMs: Long) {
    fun ticks(): Flow<Long> = flow {
        var n = 0L
        while (true) {
            emit(n++)
            kotlinx.coroutines.delay(periodMs)
        }
    }

    fun labels(): Flow<String> = ticks().map { "tick $it" }
}

suspend fun <T> Flow<T>.firstMatching(predicate: suspend (T) -> Boolean): T? {
    var found: T? = null
    collect { if (found == null && predicate(it)) found = it }
    return found
}

object Dispatchers2 {
    val io = "io"
    val main = "main"
}
