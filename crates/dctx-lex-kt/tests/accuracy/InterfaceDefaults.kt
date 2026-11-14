package contracts

interface Shape {
    val name: String
    val sides: Int get() = 0

    fun area(): Double

    fun describe(): String {
        return "$name with $sides sides"
    }
}

fun interface Transformer<A, B> {
    fun transform(input: A): B
}

interface Closeable2 : AutoCloseable {
    override fun close() {
        println("closing")
    }

    companion object {
        fun noop(): Closeable2 = object : Closeable2 {}
    }
}

private interface Internal
