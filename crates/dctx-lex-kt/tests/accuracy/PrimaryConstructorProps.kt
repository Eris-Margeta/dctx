package people

class Person(val name: String, var age: Int, nickname: String?) {
    val display = nickname ?: name

    fun birthday() {
        age += 1
    }
}

open class Animal(
    open val legs: Int,
    protected val sound: String = "...",
)

class Dog(override val legs: Int = 4) : Animal(legs, "woof")

class Empty()

class Settings internal constructor(private val values: Map<String, String>) {
    operator fun get(key: String): String? = values[key]
}
