package core.generic

interface Mapper<in A, out B> {
    fun map(input: A): B
}

class Pipeline<T>
    where T : CharSequence,
          T : Comparable<T> {

    private val steps = mutableListOf<Mapper<T, T>>()

    fun add(step: Mapper<T, T>): Pipeline<T> = apply { steps.add(step) }

    fun <R> run(
        input: T,
        finish: (T) -> R,
    ): R {
        var current = input
        for (step in steps) current = step.map(current)
        return finish(current)
    }

    class Builder<T> where T : CharSequence, T : Comparable<T> {
        fun build(): Pipeline<T> = Pipeline()
    }
}

abstract class Node<out Self : Node<Self>> {
    abstract val id: Long
    open fun self(): Self = throw UnsupportedOperationException('"'.toString())
}

expect fun platformName(): String

actual class Clock actual constructor() {
    actual fun now(): Long = 0L
}
