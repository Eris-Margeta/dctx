package constants

const val MAX_RETRIES = 3

val DEFAULT_NAME = "anonymous"

private var counter: Int = 0

lateinit var session: String

val lazyConfig: Map<String, String> by lazy {
    mapOf("k" to "v")
}

var observed: Int by kotlin.properties.Delegates.observable(0) { _, old, new ->
    println("$old -> $new")
}

internal val computed: Int
    get() = counter * 2

@JvmField
val PUBLIC_FIELD = 1
