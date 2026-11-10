"""Platform-independent inline symbols and readable plain-text equivalents.

Source documents and CLI captures remain untouched. Published Markdown/LLM text
uses the ASCII spellings below; HTML uses the matching vector paths.
"""
from __future__ import annotations

import html
import re
from html.parser import HTMLParser

FORBIDDEN = re.compile("[\u2039\u203a\u2190-\u21ff\u2300-\u23ff\u25a0-\u27bf\u27f0-\u27ff\u2900-\u297f\u2b00-\u2bff\U0001f000-\U0001faff\ufe0f]")
SYMBOLS = {
    0x2039: ("previous", "m15 5-7 7 7 7"),
    0x203A: ("next", "m9 5 7 7-7 7"),
    0x2192: ("->", "M3 12h18m-6-6 6 6-6 6"),
    0x21D2: ("=>", "M3 9h15M3 15h15m-5-10 8 7-8 7"),
    0x2190: ("<-", "M21 12H3m6-6-6 6 6 6"),
    0x2191: ("up", "M12 21V3m-6 6 6-6 6 6"),
    0x2193: ("down", "M12 3v18m-6-6 6 6 6-6"),
    0x2705: ("[yes]", "M4 12l5 5L20 6"),
    0x274C: ("[no]", "m5 5 14 14M19 5 5 19"),
    0x26A0: ("[warning]", "M12 3 2 21h20L12 3Zm0 6v5m0 3v1"),
    0x2699: ("[settings]", "M9 3h6l1 4 4 1 2 5-3 3-1 5h-5l-3-3-5-1-2-5 4-3 2-6ZM9 12a3 3 0 1 0 6 0 3 3 0 1 0-6 0"),
    0x23F3: ("[pending]", "M5 3h14M5 21h14M7 3v4l10 10v4M17 3v4L7 17v4"),
}


def plain(text: str) -> str:
    def replace(m):
        cp = ord(m[0])
        if cp == 0xFE0F:
            return ""
        if cp not in SYMBOLS:
            raise ValueError(f"unmapped decorative glyph U+{cp:04X}")
        return SYMBOLS[cp][0]
    return FORBIDDEN.sub(replace, text)


def icon(cp: int, cls: str = "glyph") -> str:
    label, path = SYMBOLS[cp]
    return (f'<span class="{cls}-wrap" role="img" aria-label="{html.escape(label, quote=True)}">'
            f'<svg class="{cls}" viewBox="0 0 24 24" width="16" height="16" '
            'fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" '
            f'stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="{path}"/></svg></span>')


class VectorHTML(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=False)
        self.parts = []
        self.script = False

    def handle_starttag(self, tag, attrs):
        raw = self.get_starttag_text()
        # Accessible labels, titles and metadata are text, never SVG markup.
        self.parts.append(self.attributes(raw))
        if tag == "script":
            self.script = True

    def handle_startendtag(self, tag, attrs):
        self.parts.append(self.attributes(self.get_starttag_text()))

    @staticmethod
    def attributes(raw):
        raw = FORBIDDEN.sub(lambda m: html.escape(plain(m[0]), quote=True), raw)
        return re.sub(r"&(?:#x[0-9a-fA-F]+|#\d+|[A-Za-z]+);",
                      lambda m: html.escape(plain(html.unescape(m[0])), quote=True)
                      if FORBIDDEN.search(html.unescape(m[0])) else m[0], raw)

    def handle_endtag(self, tag):
        self.parts.append(f"</{tag}>")
        if tag == "script":
            self.script = False

    def handle_data(self, text):
        if self.script:
            self.parts.append(plain(text))
        else:
            self.parts.append(FORBIDDEN.sub(lambda m: "" if ord(m[0]) == 0xFE0F else icon(ord(m[0])), text))

    def handle_entityref(self, name):
        self.entity(f"&{name};")

    def handle_charref(self, name):
        self.entity(f"&#{name};")

    def entity(self, value):
        decoded = html.unescape(value)
        if FORBIDDEN.search(decoded):
            self.handle_data(decoded)
        else:
            self.parts.append(value)

    def handle_comment(self, data):
        self.parts.append(f"<!--{plain(data)}-->")

    def handle_decl(self, decl):
        self.parts.append(f"<!{decl}>")


def vectorize(text: str) -> str:
    parser = VectorHTML()
    parser.feed(text)
    parser.close()
    return "".join(parser.parts)
