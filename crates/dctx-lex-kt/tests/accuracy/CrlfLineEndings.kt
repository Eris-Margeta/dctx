package win.crlf

class CrlfService(val port: Int) {
    fun start(): String = "listening on $port"

    /* class PhantomCrlf
       spans lines */
    fun stop() = Unit
}

val raw = """
fun phantomCrlfRaw() {}
"""

fun crlfTopLevel(
    a: Int,
    b: Int,
) = a + b

enum class Mode { A, B }
