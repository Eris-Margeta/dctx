/* outer /* nested */ still a comment */
/// String coverage.
func render() {
    let ordinary = "struct Fake {} // not a comment"
    let escaped = "quote: \" and slash \\"
    let raw = #"func Invisible() {} \(notInterpolation())"#
    let extended = ##"a "# quote \##(outer(inner("text")))"##
    let multi = """
    class Hidden {}
    \(format({ () -> String in nested("ok") }()))
    """
    let rawMulti = #"""
    \#(client?.render())
    """#
    print(ordinary, escaped, raw, extended, multi, rawMulti)
}
