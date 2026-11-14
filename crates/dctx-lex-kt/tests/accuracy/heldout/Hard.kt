package edge.hard

interface Logger {
    fun info(msg: String)
}

context(Logger)
fun audit(event: String) = info(event)

enum class Level : Comparable<Level> {
    LOW,
    HIGH,
}

fun <T : Any> T?.orDefault(default: T): T = this ?: default

fun (String.() -> Unit)?.runOn(target: String) = this?.invoke(target)

class Engine(
    @Suppress("unused") private val name: String,
    vararg val parts: String,
) {
    init {
        val opening = '{'
        require(opening == '{') { "class PhantomInit" }
    }

    /**
     * Old entry point.
     */
    @Deprecated(
        "use start",
        ReplaceWith("start()"),
    )
    fun begin() = start()

    fun start() {}

    operator fun invoke(): Engine = this

    fun `stop now`() {}

    val handler: Runnable = object : Runnable {
        override fun run() = start()
    }

    val names: List<String>
        get() = parts.toList()
}

abstract class Shape {
    abstract fun area(): Double
}

class Square(private val side: Double) : Shape() {
    override fun area() = side * side
}

fun shapes(): List<Shape> = listOf(Square(1.0), object : Shape() {
    override fun area() = 0.0
})
