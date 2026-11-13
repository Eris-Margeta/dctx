package net.result

sealed class Result<out T> {
    data class Success<T>(val value: T) : Result<T>()
    data class Failure(val error: Throwable) : Result<Nothing>()
    object Loading : Result<Nothing>()

    fun isTerminal(): Boolean = this !is Loading
}

sealed interface Event {
    val at: Long
}

data class Click(override val at: Long, val x: Int, val y: Int) : Event

class Scroll(override val at: Long, val delta: Int) : Event

fun describe(event: Event): String = when (event) {
    is Click -> "click at ${event.x},${event.y}"
    is Scroll -> "scroll ${event.delta}"
}
