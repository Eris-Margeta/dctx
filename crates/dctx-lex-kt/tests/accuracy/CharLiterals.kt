package chars

const val QUOTE = '"'
const val APOS = '\''
const val OPEN = '{'
const val CLOSE = '}'
val unicode = '\u0041'
val dollar = '$'

fun classify(c: Char): String = when (c) {
    '{', '}' -> "brace"
    '"' -> "quote"
    '/' -> "slash"
    else -> "other"
}

class Lexer {
    fun isOpen(c: Char) = c == '('

    fun isComment(c: Char, next: Char) = c == '/' && next == '*'
}

fun afterChars() = "done"
