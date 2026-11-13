package templates.nested

fun greet(name: String) = "Hello, ${name.ifEmpty { "class PhantomEmpty {}" }}!"

fun nested(items: List<String>): String =
    "Items: ${items.joinToString { "\"fun phantomItem() {}\" ${it.length}" }}"

val braces = "{ class PhantomBrace { fun x() {} } }"

val escapes = "tab\tquote\" fun phantomEscaped() {} \${ class PhantomNotTemplate }"

class Report(val rows: Int) {
    fun summary() = "Report with $rows rows, ${if (rows > 1) "object PhantomPlural" else "one"}"

    fun table() = """
        |rows: ${rows.toString().padStart(3)}
        |${listOf("fun phantomRow()").first()}
    """.trimMargin()
}

fun afterTemplates() = Unit
