class Templates {
    String html = """
        class PhantomText { void bad() {} }
        /* interface PhantomBlock {} */
        \""" still inside the text block
        """;
    int render() { return html.length(); }
}
