package templates

val sql = """
    SELECT * FROM users
    WHERE name = 'class PhantomSql'
    -- fun phantomSqlFun() {}
""".trimIndent()

fun page(title: String): String = """
    <html>
      <title>$title</title>
      <script>function phantomJs() { class PhantomJs {} }</script>
      <p>${title.uppercase()} costs ${'$'}5 and "quotes" fun phantom() {}</p>
    </html>
""".trimIndent()

val kotlinSnippet = """
fun phantomGenerated() {
    val x = 1
}
class PhantomGenerated
"""

val withQuotes = """she said ""hello"" then left"""

fun afterRaw(): Int = 1

class RawHolder {
    val regex = """\d+\.\d+ class PhantomRegex""".toRegex()

    fun use() = regex.matches("1.0")
}
