package delegation

interface Logger {
    fun log(msg: String)
}

class ConsoleLogger : Logger {
    override fun log(msg: String) = println(msg)
}

class Service(logger: Logger) : Logger by logger {
    var state: String = "idle"
        private set(value) {
            log("state $field -> $value")
            field = value
        }

    val isIdle: Boolean
        get() {
            return state == "idle"
        }

    var retries by mutableMapOf<String, Any?>("retries" to 0)

    fun start() {
        state = "running"
    }
}
