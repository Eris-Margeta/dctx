@Deprecated("fixture")
fun <T> copy(value: T): T {
    fun nested(item: T): T = item
    return nested(value)
}
