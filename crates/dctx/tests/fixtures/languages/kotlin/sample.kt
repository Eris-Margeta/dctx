package sample
@Deprecated("fixture")
class Boxed<T>(val value: T) {
    fun <U> copy(item: U): U {
        fun nested(input: U): U { return input }
        return nested(item)
    }
    fun café(): Int = 1
}
