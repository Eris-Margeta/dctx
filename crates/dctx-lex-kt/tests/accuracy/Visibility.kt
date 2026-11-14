package vis

private class Hidden {
    internal fun helper() = Unit
}

internal object Module {
    private const val SECRET = "s"

    fun expose() = SECRET.length
}

open class Base protected constructor() {
    protected open fun hook() {}
    public override fun toString(): String = "Base"
}

class Derived : Base() {
    final override fun hook() {}
}

private fun fileLocal() = Unit
