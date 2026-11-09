"""Content-hashed static assets: fonts, CSS and JS from tools/website/src and tools/website/fonts.

Each asset is published as /assets/<name>.<first 8 hex of sha256>.<ext>; pages reference it
through Assets.url(logical name). Images keep stable URLs (OG cards, icons) and are registered
with require() so check.py can prove each referenced file exists.
"""
from __future__ import annotations

import base64
import hashlib
import re

from common import read_bytes

FONTS = ["geist-400", "geist-600", "jetbrains-mono-400", "jetbrains-mono-500"]
# One script request per page: site.js plus what the page needs, concatenated (each part is an IIFE).
# anim.js (the explainer animations, pages/animations.py) is inert on a page without figure[data-anim].
BUNDLES = {"page.js": ["site.js"], "home.js": ["site.js", "term.js", "hero.js", "anim.js"], "examples.js": ["site.js", "term.js"],
           "playground.js": ["site.js", "playground.js"], "search.js": ["site.js", "search.js"], "how.js": ["site.js", "anim.js"]}


def minify_css(css: str) -> str:
    """Conservative: drop comments and the whitespace around punctuation and at line starts. Strings and
    url() values in site.css hold no such whitespace; check.py still reads the colour tokens."""
    css = re.sub(r"/\*.*?\*/", "", css, flags=re.S)
    css = re.sub(r"\s*\n\s*", "\n", css)
    css = re.sub(r"\s*([{};])\s*", r"\1", css)
    css = re.sub(r";}", "}", css)
    return css.strip() + "\n"


class Assets:
    def __init__(self) -> None:
        self.files: dict[str, bytes] = {}   # published path (with leading /) -> bytes
        self.urls: dict[str, str] = {}      # logical name -> published path
        self.required: set[str] = set()

    def add(self, logical: str, data: bytes) -> str:
        stem, _, ext = logical.rpartition(".")
        url = f"/assets/{stem}.{hashlib.sha256(data).hexdigest()[:8]}.{ext}"
        self.files[url] = data
        self.urls[logical] = url
        return url

    def url(self, logical: str) -> str:
        return self.urls[logical]

    def require(self, url: str) -> None:
        self.required.add(url)

    def load(self) -> None:
        for name in FONTS:
            self.add(f"{name}.woff2", read_bytes(f"tools/website/fonts/{name}.woff2"))
        css = read_bytes("tools/website/src/site.css").decode("utf-8")
        css = re.sub(r"@font:([a-z0-9-]+)", lambda m: self.url(m.group(1) + ".woff2"), css)
        css = minify_css(css)
        self.add("site.css", css.encode("utf-8"))
        for name, parts in BUNDLES.items():
            self.add(name, b"\n".join(read_bytes(f"tools/website/src/{p}") for p in parts))
        # The theme script runs inline before first paint (no request); nginx.conf allows it by hash.
        self.theme_inline = read_bytes("tools/website/src/theme.js").decode("utf-8").strip()
        self.theme_hash = "sha256-" + base64.b64encode(hashlib.sha256(self.theme_inline.encode()).digest()).decode()
        for name in ("OFL-Geist.txt", "OFL-JetBrainsMono.txt"):
            self.files[f"/assets/fonts/{name}"] = read_bytes(f"tools/website/fonts/{name}")
