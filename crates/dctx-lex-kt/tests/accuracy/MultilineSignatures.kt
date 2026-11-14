package layout

public open class Widget
@JvmOverloads
constructor(
    val id: String,
    val visible: Boolean = true,
) {
    protected open fun render(
        width: Int,
        height: Int,
    ): String = "$id $width x $height"

    internal fun
        measure(): Int = 0

    @Suppress("unused")
    private
    fun hidden() = Unit
}

abstract class Base {
    abstract val title: String
    open var subtitle: String? = null
        protected set

    abstract fun build(): Widget
}
