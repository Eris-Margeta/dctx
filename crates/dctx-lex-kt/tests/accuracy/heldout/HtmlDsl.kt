package dsl.html

@DslMarker
annotation class HtmlTag

@HtmlTag
abstract class Tag(val name: String) {
    val children = mutableListOf<Tag>()

    protected fun <T : Tag> initTag(tag: T, init: T.() -> Unit): T {
        tag.init()
        children += tag
        return tag
    }
}

class Html : Tag("html") {
    fun body(init: Body.() -> Unit) = initTag(Body(), init)
}

class Body : Tag("body") {
    fun p(text: String) {
        val raw = """
            fun phantomRaw() {}
            class PhantomRaw
        """
        children += Text(text + raw)
    }
}

class Text(val text: String) : Tag("#text")

fun html(init: Html.() -> Unit): Html = Html().apply(init)

val page = html {
    body {
        p("hello")
    }
}
