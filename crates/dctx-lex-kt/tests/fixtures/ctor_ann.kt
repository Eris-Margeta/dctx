// Minimal annotated primary constructors; PARSERS-v1 regression.
// Depends on Kotlin annotation syntax; used by dctx_lex_kt.rs.
// Constructor annotations may continue across inserted semicolons.
class PluginResult @JvmOverloads constructor(json: JSObject? = JSObject()) {
    fun put() {}
}

internal class ReadClipDataSerializer @JvmOverloads constructor(t: Class<ReadClipData>? = null) :
    StdSerializer<ReadClipData>(t)

class Marked @param:Keep([1]) constructor(val n: Int)

class Split
@JvmOverloads
constructor(n: Int = zero()) {
    fun ready() {}
}
