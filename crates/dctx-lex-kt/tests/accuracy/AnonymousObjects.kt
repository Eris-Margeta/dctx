package anon

class Holder {
    val x = 1
    companion object {
        val y = 2
    }
}

class Watcher {
    val callback = Runnable { println("run") }
    private val token = 3
    companion object Named {
        fun make() = Watcher()
    }
}

fun register(target: Any) = target

fun wire() {
    register(object : Runnable {
        override fun run() {}
    })
    val k = String::class
    class LocalAfterLiteral(val v: Int)
}

val comparator = object : Comparator<String> {
    override fun compare(a: String, b: String) = a.compareTo(b)
}

data object Singleton

fun typeOf(x: Any) = x::class.simpleName
fun afterLiteral() = 1
