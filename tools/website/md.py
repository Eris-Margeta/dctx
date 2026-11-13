"""Markdown documents of the repository -> website pages and Markdown twins.

Docs are not copied into the site source: every page is rendered from the repository file at build
time, and the Markdown twin (index.md) is the file itself with only link targets rewritten (a link to
a rendered document becomes its website route, any other repository file a GitHub link, and a link
to a file the public tree does not contain becomes plain text). check.py compares each twin with its
source. The one presentational change is on the README page: its badge header (third-party images)
is replaced by the title and strapline it contains.
"""
from __future__ import annotations

import hashlib
import html
import json
import posixpath
import re
from dataclasses import dataclass, field

import markdown

import highlight
from common import ALIASES, BLOB, ORIGIN, ROOT, TREE, read, slugify

# External link targets a rendered document may keep. Anything else is unlinked (text kept).
ALLOWED_EXTERNAL = ("https://github.com/Eris-Margeta", "https://github.com/sponsors/Eris-Margeta", "https://tejl.com",
                    "https://tejl.hr", "https://schema.org", "https://www.apache.org/", "https://keepachangelog.com",
                    "https://semver.org", "https://developercertificate.org", "https://docs.github.com/en/actions")
HTML_TAGS = set("a abbr b blockquote br caption code col colgroup dd del details div dl dt em h1 h2 h3 h4 h5 h6 hr i img "
                "ins kbd li ol p pre s samp small span strong sub summary sup table tbody td tfoot th thead tr u ul".split())

_removed: dict | None = None


def removed(path: str) -> bool:
    """True for a path the public tree does not contain (hashed list in data/removed-paths.json)."""
    global _removed
    if _removed is None:
        _removed = json.loads(read("tools/website/data/removed-paths.json"))
    digest = lambda text: hashlib.sha256(text.encode()).hexdigest()  # noqa: E731
    parts = path.strip("/").split("/")
    if digest(parts[-1]) in _removed["basenames"]:
        return True
    return any(digest("/".join(parts[:i]) + ("/" if i < len(parts) else "")) in _removed["paths"]
               or digest("/".join(parts[:i])) in _removed["paths"] for i in range(1, len(parts) + 1))


@dataclass
class Doc:
    rel: str
    title: str
    html: str
    toc: list[tuple[int, str, str]]
    twin: str
    words: int
    warnings: list[str] = field(default_factory=list)


class Linker:
    """Resolves repository-relative links. `routes` maps repository files to website paths."""

    def __init__(self, routes: dict[str, str]):
        self.routes = routes
        self.warnings: list[str] = []

    def resolve(self, source: str, href: str) -> str | None:
        href = href.strip()
        if href.startswith("https://dctx.app/"):
            return "/" + href[len("https://dctx.app/"):]
        for alias in ALIASES:
            if href.startswith(alias + "/") or href == alias:
                return href[len(alias):] or "/"
        for prefix in (BLOB, TREE):
            if href.startswith(prefix):
                return self.resolve_repo(href[len(prefix):], source, href)
        if re.match(r"^[a-z][a-z0-9+.-]*:", href):
            if href.startswith("mailto:"):
                return href
            if href.startswith(ALLOWED_EXTERNAL):
                return href
            self.warnings.append(f"{source}: external link {href} unlinked")
            return None
        if href.startswith("#"):
            return href
        path, _, frag = href.partition("#")
        target = posixpath.normpath(posixpath.join(posixpath.dirname(source), path)) if path else source
        return self.resolve_repo(target + ("#" + frag if frag else ""), source, href)

    def resolve_repo(self, target: str, source: str, original: str) -> str | None:
        path, _, frag = target.partition("#")
        path = path.rstrip("/") or "."
        if path.startswith("..") or path.startswith("/"):
            self.warnings.append(f"{source}: link {original} leaves the repository")
            return None
        if removed(path):
            self.warnings.append(f"{source}: link {original} points outside the public tree; unlinked")
            return None
        if path in self.routes:
            return self.routes[path] + ("#" + frag if frag else "")
        full = ROOT / path
        if not full.exists():
            self.warnings.append(f"{source}: link {original} points to a missing file; unlinked")
            return None
        return (TREE if full.is_dir() else BLOB) + path + ("#" + frag if frag else "")


def prepare(rel: str, for_html: bool = True) -> str:
    """The document text to render: the file itself; for HTML, the README badge header is replaced
    by its title and strapline, and HTML comments (generator markers) are dropped."""
    text = read(rel)
    if not for_html:
        return text
    if rel == "README.md":
        head, sep, rest = text.partition("\n---\n")
        strap = re.search(r"<strong>(.*?)</strong><br>\s*(.*?)\s*</p>", head, re.S)
        if not sep or not strap:
            raise SystemExit("md.py: README header changed; update prepare()")
        text = f"# dctx\n\n**{strap.group(1).strip()}** {strap.group(2).strip()}\n\n---\n" + rest
    text = re.sub(r"<!--.*?-->\n?", "", text, flags=re.S)
    # GitHub nests lists indented by 2 spaces; Python-Markdown needs 4. Double list-item indents
    # outside fenced code (HTML rendering only; the Markdown twin is the file itself).
    out, fence = [], False
    for line in text.split("\n"):
        if line.lstrip().startswith("```"):
            fence = not fence
        m = None if fence else re.match(r"^( +)((?:[-*+]|\d+\.) .*)$", line)
        out.append(" " * (len(m.group(1)) * 2) + m.group(2) if m else line)
    return "\n".join(out)


def _md_links(text: str, rel: str, linker: Linker) -> str:
    """Rewrite Markdown links to absolute website or GitHub URLs, outside fenced code."""
    def absolute(url: str) -> str:
        return ORIGIN + url if url.startswith("/") else url

    def inline(m: re.Match) -> str:
        bang, label, url, title = m.group(1), m.group(2), m.group(3), m.group(4) or ""
        target = linker.resolve(rel, url)
        if target is None:
            return m.group(0) if bang else label
        if target.startswith("#"):
            return m.group(0)
        return f"{bang}[{label}]({absolute(target)}{title})"

    def reference(m: re.Match) -> str:
        target = linker.resolve(rel, m.group(2))
        return m.group(0) if target is None or target.startswith("#") else f"{m.group(1)}{absolute(target)}"

    def attribute(m: re.Match) -> str:
        target = linker.resolve(rel, m.group(2))
        return m.group(0) if target is None or target.startswith("#") else f"{m.group(1)}{absolute(target)}{m.group(3)}"

    parts = re.split(r"(^```.*?^```[ \t]*$)", text, flags=re.M | re.S)
    for i in range(0, len(parts), 2):
        parts[i] = re.sub(r"(!?)\[([^\]\n]*)\]\(<?([^)\s>]+)>?(\s+\"[^\"]*\")?\)", inline, parts[i])
        parts[i] = re.sub(r"(?m)^(\[[^\]]+\]:\s*)(\S+)", reference, parts[i])
        parts[i] = re.sub(r'((?:href|src)=")([^"]+)(")', attribute, parts[i])
    return "".join(parts)


def _converter() -> markdown.Markdown:
    return markdown.Markdown(extensions=["extra", "sane_lists"])


def _code_block(m: re.Match) -> str:
    """A fenced or indented block, highlighted at build time (highlight.py); the language label is shown."""
    lang, body = m.group(1) or "", html.unescape(m.group(2))
    body = body[:-1] if body.endswith("\n") else body
    shown = highlight.ALIASES.get(lang.lower(), lang.lower())
    if shown in ("", "text", "txt", "plain"):
        shown = highlight.detect(body)
    label = {"sh": "shell", "js": "js/ts", "transcript": "shell", "text": "", "pack": "dctx pack", "map": "dctx map"}.get(shown, shown)
    attr = f' data-lang="{html.escape(label)}"' if label else ""
    return f'<div class="hl"{attr}><pre><code>{highlight.highlight(body, lang)}</code></pre></div>'



def to_html(text: str, rel: str, linker: Linker) -> tuple[str, list[tuple[int, str, str]], str]:
    """Markdown -> HTML with GitHub-style heading ids, anchors, wrapped tables and rewritten links."""
    body = _converter().convert(text)
    # Escape tags that are not HTML (for example a bare <snapshot> in prose).
    body = re.sub(r"<(/?)([A-Za-z][\w-]*)", lambda m: m.group(0) if m.group(2).lower() in HTML_TAGS else "&lt;" + m.group(1) + m.group(2), body)
    body = re.sub(r"<a href=\"([^\"]*)\"([^>]*)>(.*?)</a>", lambda m: _link(m, rel, linker), body, flags=re.S)
    seen: dict[str, int] = {}
    toc: list[tuple[int, str, str]] = []
    title = ""

    def heading(m: re.Match) -> str:
        nonlocal title
        level, inner = int(m.group(1)), m.group(2)
        text_only = html.unescape(re.sub(r"<[^>]+>", "", inner)).strip()
        base = slugify(inner) or "section"
        n = seen.get(base, 0)
        seen[base] = n + 1
        slug = base if n == 0 else f"{base}-{n}"
        if level == 1 and not title:
            title = text_only
            return f'<h1 id="{slug}">{inner}</h1>'
        if level in (2, 3):
            toc.append((level, slug, text_only))
        return (f'<h{level} id="{slug}">{inner}<a class="hd" href="#{slug}" aria-label="Link to {html.escape(text_only)}">#</a>'
                f"</h{level}>")

    # One h1 per page: further h1 headings (the Parts of the specification) become h2. Anchors are unchanged.
    first = body.find("</h1>")
    if first >= 0:
        body = body[:first + 5] + re.sub(r"<(/?)h1>", r"<\1h2>", body[first + 5:])
    body = re.sub(r"<h([1-6])>(.*?)</h\1>", heading, body, flags=re.S)
    body = re.sub(r' style="text-align: (left|right|center);"', r' class="al-\1"', body)
    body = body.replace("<table>", '<div class="table-wrap"><table>').replace("</table>", "</table></div>")
    body = re.sub(r'<pre><code(?: class="language-([\w+#.-]+)")?>(.*?)</code></pre>', _code_block, body, flags=re.S)
    return body, toc, title


def _link(m: re.Match, rel: str, linker: Linker) -> str:
    href, rest, label = html.unescape(m.group(1)), m.group(2), m.group(3)
    target = linker.resolve(rel, href)
    if target is None:
        return f'<span class="unlinked">{label}</span>'
    return f'<a href="{html.escape(target)}"{rest}>{label}</a>'


def render(rel: str, linker: Linker) -> Doc:
    text = prepare(rel)
    before = len(linker.warnings)
    body, toc, title = to_html(text, rel, linker)
    twin = _md_links(read(rel), rel, linker)
    linker.warnings[before:] = sorted(set(linker.warnings[before:]))
    words = len(re.findall(r"\S+", re.sub(r"```.*?```", "", text, flags=re.S)))
    return Doc(rel, title, body, toc, twin, words)


def strip_targets(text: str) -> str:
    """Text with every link target blanked: what check.py compares between a twin and its source."""
    text = re.sub(r"(!?\[[^\]\n]*\])\(<?[^)\s>]+>?(\s+\"[^\"]*\")?\)", r"\1()", text)
    text = re.sub(r"(?m)^(\[[^\]]+\]:\s*)\S+", r"\1", text)
    text = re.sub(r'((?:href|src)=")[^"]+(")', r"\1\2", text)
    return re.sub(r"\[([^\]\n]*)\]\(\)", r"\1", text)
