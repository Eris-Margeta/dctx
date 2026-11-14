package html.dsl

@DslMarker
annotation class HtmlDsl

@HtmlDsl
abstract class Tag(val name: String) {
    val children = mutableListOf<Tag>()

    protected fun <T : Tag> initTag(tag: T, init: T.() -> Unit): T {
        tag.init()
        children.add(tag)
        return tag
    }
}

class Html : Tag("html") {
    fun body(init: Body.() -> Unit) = initTag(Body(), init)
}

class Body : Tag("body") {
    fun p(text: String) = initTag(P()) { this.text = text }
}

class P : Tag("p") {
    var text = ""
}

fun html(init: Html.() -> Unit): Html = Html().apply(init)

val page = html {
    body {
        p("class PhantomDslText")
    }
}
