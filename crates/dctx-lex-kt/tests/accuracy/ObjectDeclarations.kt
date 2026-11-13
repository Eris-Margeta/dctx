package app

object Config {
    var debug = false
    val version: String = "1.0"

    fun dump(): String = "debug=$debug version=$version"
}

object DefaultHandler : Runnable, AutoCloseable {
    override fun run() {}
    override fun close() {}
}

val listener = object : Runnable {
    override fun run() {
        println("anonymous run")
    }
}

fun counter(): () -> Int {
    val state = object {
        var count = 0
    }
    return { ++state.count }
}
