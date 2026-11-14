package util.reflect

inline fun <reified T> Any?.castOrNull(): T? = this as? T

inline fun <reified T : Any> typeName(): String = T::class.java.simpleName

inline fun measure(label: String, crossinline block: () -> Unit, noinline after: () -> Unit) {
    val start = System.nanoTime()
    block()
    println("$label took ${System.nanoTime() - start}")
    after()
}

class Registry {
    val entries = mutableMapOf<Class<*>, Any>()

    inline fun <reified T : Any> register(value: T) {
        entries[T::class.java] = value
    }

    inline fun <reified T : Any> lookup(): T? = entries[T::class.java] as? T
}
