#!/usr/bin/env python3
"""Rebuild the website in isolation and check it. Offline. Exit 0 only with 0 errors.

    DCTX_BIN=target/release/dctx-cli DCTX_DENY_LIST=path/to/deny-list.txt \\
        tools/website/.venv/bin/python tools/website/check.py [--no-capture]

    # Deployment: check freshly built website/ instead of committed HEAD bytes.
    tools/website/.venv/bin/python tools/website/check.py --working-tree --no-capture

1. Rebuild from the tracked files into a temporary directory; every generated file must equal HEAD.
2. Capture drift: re-run capture.py with DCTX_BIN; data/demo.json must not change (and --version = VERSION).
3. Deny-list (DCTX_DENY_LIST, one Python bytes regex per line) over website/, tools/website/, tools/playground/.
4. Documents and links, JSON-LD, OG images, machine files, first-load budgets (requests, bytes, largest
   asset), accessibility, contrast, external links, hashed assets, docs twins against their sources,
   nginx.conf (CSP with the theme script hash), legacy and sales wording, dates.json against git.
5. The llms files: facts.json against VERSION, CHANGELOG.md and the tag; data/llms.json against the documents,
   git and (with DCTX_BIN) a re-capture; removed commands only in the removed list.
Needs the build dependencies (tools/website/requirements.txt).
"""
from __future__ import annotations

import base64
import datetime as dt
import hashlib
import html
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urljoin, urlsplit

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / "tools/website"
sys.path.insert(0, str(HERE))
from llms import LLMS_LIMIT
from glyphs import FORBIDDEN, plain as plain_glyphs

ORIGIN = "https://www.dctx.internet.technology"
HOST = urlsplit(ORIGIN).hostname
STATIC = {".gitignore", "assets/tejl-logo-off-black.svg"}
ALLOWED_EXTERNAL = ("https://github.com/Eris-Margeta", "https://github.com/sponsors/Eris-Margeta", "https://tejl.com",
                    "https://tejl.hr", "https://schema.org", "https://www.apache.org/", "https://keepachangelog.com",
                    "https://semver.org", "https://developercertificate.org", "https://docs.github.com/en/actions",
                    "https://code.claude.com/docs/en/mcp", "https://cursor.com/docs/mcp",
                    "https://developers.openai.com/codex/mcp/",
                    "mailto:info@tejl.com", "mailto:eris.margeta@gmail.com")
# First visit, cache empty. The reference sites measured 8 requests / 372 KB (rural) and 40 requests /
# 396 KB (internet.technology) on their home pages; these budgets match or beat both.
BUDGET = {"/": {"requests": 8, "bytes": 370_000}, "*": {"requests": 9, "bytes": 450_000}}
LARGEST = 200_000
HTML_LIMIT = {"docs/spec/index.html": 450_000, "cli/index.html": 300_000, "docs/benchmark-oct/index.html": 300_000,
              "changelog/index.html": 200_000, "examples/index.html": 300_000, "docs/benchmark/index.html": 200_000}
SALES = re.compile(r"(?i)free forever|(?<!not )enterprise|pricing|trusted by|world[- ]class|revolutionary|best[- ]in[- ]class")
LEGACY_RULES = re.compile(r"(?i)\.legacyignore|\.dctxomitcontent|config/dctx/omitcontent")
LEGACY_CALL = re.compile(r"dctx \.(?:\s+-[a-zA-Z]|[ \t]*$)", re.M)


def git(*args: str) -> bytes:
    return subprocess.run(["git", "-C", str(ROOT), *args], check=True, capture_output=True).stdout


def tracked() -> list[str]:
    return [p.decode() for p in git("ls-files", "-z", "--cached", "--others", "--exclude-standard").split(b"\0") if p]


def rebuild(dest: Path) -> Path:
    for rel in tracked():
        parts = Path(rel).parts
        if parts[0] == "website" and "/".join(parts[1:]) not in STATIC:
            continue
        if not (ROOT / rel).is_file():
            continue
        target = dest / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / rel, target)
    result = subprocess.run([sys.executable, str(dest / "tools/website/build.py")], cwd=dest, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f"build.py exited {result.returncode}:\n{result.stdout}{result.stderr}")
    return dest / "website"


def staleness(site: Path, errors: list[str], working_tree: bool = False) -> int:
    if working_tree:
        committed = {p.relative_to(ROOT / "website").as_posix() for p in (ROOT / "website").rglob("*") if p.is_file()}
    else:
        committed = {n.decode().removeprefix("website/") for n in git("ls-tree", "-r", "--name-only", "-z", "HEAD", "--", "website").split(b"\0") if n}
    reference = "website/" if working_tree else "the committed bytes"
    generated = {p.relative_to(site).as_posix() for p in site.rglob("*") if p.is_file()}
    for name in sorted(committed | generated):
        if name not in generated:
            errors.append(f"stale: website/{name} exists in {reference} but is no longer generated")
        elif name not in committed:
            errors.append(f"stale: website/{name} is generated but missing from {reference}")
        else:
            expected = (ROOT / "website" / name).read_bytes() if working_tree else git("show", f"HEAD:website/{name}")
            if (site / name).read_bytes() != expected:
                errors.append(f"stale: website/{name} differs from {reference}")
    return len(generated)


def capture_drift(errors: list[str], warnings: list[str], skip: bool) -> None:
    binary = os.environ.get("DCTX_BIN") or str(ROOT / "target/release/dctx-cli")
    if skip:
        warnings.append("capture drift NOT checked (--no-capture)")
        return
    if not Path(binary).is_file():
        errors.append(f"capture: no dctx binary at {binary} (set DCTX_BIN, or pass --no-capture to skip this check)")
        return
    version = (ROOT / "VERSION").read_text().strip()
    out = subprocess.run([binary, "--version"], capture_output=True, text=True).stdout
    if not out.startswith(f"dctx {version} "):
        errors.append(f"capture: {binary} --version is {out.strip()!r}, VERSION is {version}")
        return
    with tempfile.TemporaryDirectory(prefix="dctx-capture-check-") as tmp:
        target = Path(tmp) / "demo.json"
        r = subprocess.run([sys.executable, str(HERE / "capture.py"), "--out", str(target)], env={**os.environ, "DCTX_BIN": binary},
                           capture_output=True, text=True)
        if r.returncode:
            errors.append(f"capture: capture.py failed: {r.stdout}{r.stderr}")
            return
        new, old = target.read_bytes(), (HERE / "data/demo.json").read_bytes()
        if new != old:
            a, b = json.loads(old), json.loads(new)
            changed = [x["id"] for x, y in zip(a["records"], b["records"]) if x != y] or ["(record list)"]
            errors.append(f"capture drift: the binary's output differs from data/demo.json in {', '.join(changed[:8])}; run capture.py")


def shallow() -> bool:
    try:
        return git("rev-parse", "--is-shallow-repository").strip() == b"true"
    except (subprocess.CalledProcessError, OSError):
        return True


def llms_drift(errors: list[str], warnings: list[str], skip: bool) -> None:
    """The llms files against their sources: document digests, the docs commit, and (with the binary) a re-capture."""
    import llms
    cap = json.loads((HERE / "data/llms.json").read_text())
    for rel in llms.stale_sources():
        errors.append(f"llms drift: {rel} changed since data/llms.json was captured; run capture.py --llms")
    from routes import llms_tiers
    wanted = sorted({f for *_, files in llms_tiers() for f in files})
    if sorted(cap["sources"]) != wanted:
        errors.append(f"llms drift: data/llms.json covers {len(cap['sources'])} documents, the tiers name {len(wanted)}; run capture.py --llms")
    if shallow():
        warnings.append("llms docs commit not checked: shallow clone or no git")
    else:
        real = git("log", "-1", "--format=%H %cI", "--", *cap["sources"]).decode().split()
        if real != [cap["docs_commit"], cap["docs_date"]]:
            errors.append(f"llms drift: data/llms.json names docs commit {cap['docs_commit'][:12]}, git says {real[0][:12]}; "
                          "commit the documents, then run capture.py --llms")
    binary = os.environ.get("DCTX_BIN") or str(ROOT / "target/release/dctx-cli")
    if skip or not Path(binary).is_file():
        warnings.append("llms capture drift NOT checked (no binary or --no-capture)")
        return
    with tempfile.TemporaryDirectory(prefix="dctx-llms-check-") as tmp:
        target = Path(tmp) / "llms.json"
        argv = [sys.executable, str(HERE / "capture.py"), "--llms", "--out", str(target), "--keep-git", str(HERE / "data/llms.json")]
        r = subprocess.run(argv, env={**os.environ, "DCTX_BIN": binary}, capture_output=True, text=True)
        if r.returncode:
            errors.append(f"llms capture: capture.py --llms failed: {r.stdout}{r.stderr}")
            return
        new = json.loads(target.read_text())
        changed = [b["file"] for a, b in zip(cap["tiers"], new["tiers"]) if a != b] or ["(tier list)"]
        if new != cap:
            errors.append(f"llms capture drift: the binary's output over the documents differs from data/llms.json in "
                          f"{', '.join(changed)}; run capture.py --llms")


def facts_fresh(errors: list[str], warnings: list[str]) -> None:
    """data/facts.json: version = VERSION; release date = CHANGELOG.md and the release tag (never the clock);
    the notice verbatim in README.md and docs/START.md (the canonical wording lives in those documents)."""
    facts = json.loads((HERE / "data/facts.json").read_text())
    version = (ROOT / "VERSION").read_text().strip()
    for doc in ("README.md", "docs/START.md"):
        if facts["notice"] not in (ROOT / doc).read_text():
            errors.append(f"facts.json: notice is not verbatim in {doc}")
    if facts["version"] != version:
        errors.append(f"facts.json: version {facts['version']} != VERSION {version}")
    if facts["release_tag"] != f"v{version}" or f"/releases/tag/v{version}" not in facts["verify_download"]["link"]:
        errors.append(f"facts.json: release_tag and verify_download.link must name v{version}")
    m = re.search(rf"^## \[{re.escape(version)}\] - (\d{{4}}-\d{{2}}-\d{{2}})", (ROOT / "CHANGELOG.md").read_text(), re.M)
    if not m or m.group(1) != facts["released"]:
        errors.append(f"facts.json: released {facts['released']} != CHANGELOG.md {m.group(1) if m else '(no entry)'} for {version}")
    try:
        tag_date = git("log", "-1", "--format=%cs", f"v{version}^{{commit}}", "--").decode().strip()
    except (subprocess.CalledProcessError, OSError):
        warnings.append(f"facts.json: release date not checked against git: no tag v{version}")
        return
    if tag_date != facts["released"]:
        errors.append(f"facts.json: released {facts['released']} != the commit date of tag v{version} ({tag_date})")


def removed_list_patterns() -> list[tuple[str, re.Pattern]]:
    facts = json.loads((HERE / "data/facts.json").read_text())
    return [(item["was"], re.compile(p)) for item in facts["removed"]["items"] for p in item["patterns"]]


def strip_removed_list(html_text: str) -> str:
    """A page may show the removed list inside one element marked data-facts="removed" (no same-tag nesting)."""
    return re.sub(r'<(\w+)[^>]*\bdata-facts="removed"[^>]*>.*?</\1>', "", html_text, flags=re.S)


def removed_outside_list(label: str, text: str, errors: list[str]) -> None:
    """A removed command may appear only in the facts' removed list, in a historical source inside a pack
    (llms.HISTORICAL), or on a line that itself calls it legacy or removed."""
    from llms import HISTORICAL
    from common import BLOB
    from routes import DOCS, POLICIES
    pats = removed_list_patterns()
    documents = {file for _, file, *_ in DOCS} | {file for _, file, *_ in POLICIES} | {"CHANGELOG.md"}
    source, in_list, rendered_doc = None, False, False
    for n, line in enumerate(text.split("\n"), 1):
        if source is None and line.startswith("url: "):
            rendered_doc = False
        if source is None and line.startswith("source: " + BLOB):
            # Like their HTML pages, rendered document additions are checked through
            # the source pack. Tag spacing can turn a prose link ending in "dctx" and
            # a full stop into an apparent implicit snapshot command.
            rendered_doc = line.removeprefix("source: " + BLOB) in documents
        m = re.match(r'<FILE_CONTENT_START ID="[^"]+" PATH="([^"]+)"', line)
        if m:
            source = m.group(1)
            continue
        if line.startswith("</FILE_CONTENT_END "):
            source = None
            continue
        if source is None and (line.startswith("#") or re.fullmatch(r"\*\*.+\*\*", line)):
            in_list = line.startswith(("### Removed in ", "**Removed in "))
        if rendered_doc or in_list or (source and source.startswith(HISTORICAL)) or re.search(r"(?i)(?<![-\w])legacy\b|\bremoved\b", line):
            continue
        for was, p in pats:
            if p.search(line):
                errors.append(f"{label}:{n}{' (' + source + ')' if source else ''}: removed command ({was}) outside the removed list: {line.strip()[:100]!r}")


def readme_drift(errors: list[str]) -> None:
    """Keep README examples tied to the same captures and compiled library snippet in CI."""
    result = subprocess.run([sys.executable, str(ROOT / "scripts/render_readme.py"), "--check"],
                            cwd=ROOT, capture_output=True, text=True)
    if result.returncode:
        errors.append(f"README drift: {result.stdout}{result.stderr}".strip())


def deny_list(errors: list[str], warnings: list[str]) -> int:
    path = os.environ.get("DCTX_DENY_LIST")
    if not path:
        warnings.append("DENY-LIST NOT CHECKED: set DCTX_DENY_LIST to the publish deny-list file")
        return -1
    patterns = [re.compile(line) for line in Path(path).read_bytes().split(b"\n") if line.strip() and not line.startswith(b"#")]
    hits = 0
    files = [r for r in tracked() if r.startswith(("website/", "tools/website/", "tools/playground/"))]
    files += [p.relative_to(ROOT).as_posix() for p in (ROOT / "website").rglob("*") if p.is_file()]
    for rel in sorted(set(files)):
        full = ROOT / rel
        if not full.is_file():
            continue
        data = full.read_bytes()
        for i, pattern in enumerate(patterns):
            if pattern.search(data):
                hits += 1
                errors.append(f"deny-list: {rel} matches pattern #{i + 1}")
    return hits


class Doc(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.ids: set[str] = set()
        self.refs: list[tuple[int, str, str, str]] = []
        self.h1s = 0
        self.headings: list[int] = []
        self.titles: list[str] = []
        self.canonicals: list[str] = []
        self.json_ld: list[tuple[int, str]] = []
        self.inline_scripts: list[str] = []
        self.problems: list[str] = []
        self.meta: list[dict] = []
        self.in_head = False
        self.links: list[dict] = []
        self.scripts: list[str] = []
        self.images: list[dict] = []
        self.lang = None
        self.viewport = False
        self._buf: list[str] | None = None
        self._kind = ""
        self._line = 0
        self._a: list | None = None

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        line = self.getpos()[0]
        if a.get("id"):
            if a["id"] in self.ids:
                self.problems.append(f"{line}: duplicate id {a['id']!r}")
            self.ids.add(a["id"])
        for k, v in attrs:
            if k in ("href", "src", "srcset", "action", "data-index") and v is not None:
                self.refs.append((line, tag, k, v.split()[0] if k == "srcset" else v))
            if k.startswith("on"):
                self.problems.append(f"{line}: inline event handler {k}")
            if k == "style":
                self.problems.append(f"{line}: inline style attribute")
        if tag == "head":
            self.in_head = True
        if tag == "svg":
            for key, value in (("aria-hidden", "true"), ("focusable", "false")):
                if a.get(key) != value:
                    self.problems.append(f"{line}: svg needs {key}={value}")
            if not a.get("width") or not a.get("height"):
                self.problems.append(f"{line}: svg needs explicit width and height")
        if tag == "html":
            self.lang = a.get("lang")
        if tag == "meta":
            if self.in_head:
                self.meta.append(a)
            elif (a.get("property") or a.get("name") or "").startswith(("og:", "twitter:")):
                self.problems.append(f"{line}: social metadata must be in head")
            if a.get("name") == "viewport":
                self.viewport = True
        if tag == "link":
            self.links.append(a)
            if "canonical" in (a.get("rel") or "").split():
                self.canonicals.append(a.get("href") or "")
        if re.fullmatch(r"h[1-6]", tag):
            level = int(tag[1])
            self.h1s += level == 1
            if self.headings and level > self.headings[-1] + 1:
                self.problems.append(f"{line}: heading h{level} after h{self.headings[-1]}")
            if not self.headings and level != 1:
                self.problems.append(f"{line}: first heading is h{level}")
            self.headings.append(level)
        if tag == "img":
            self.images.append(a)
            if a.get("alt") is None:
                self.problems.append(f"{line}: img without alt")
            if not (a.get("width") and a.get("height")):
                self.problems.append(f"{line}: img without width and height (layout shift)")
        if tag == "button" and not a.get("type"):
            self.problems.append(f"{line}: button without type")
        if tag == "a":
            self._a = [line, a.get("aria-label"), ""]
        if tag == "script":
            if a.get("src"):
                self.scripts.append(a["src"])
            else:
                self._buf, self._kind, self._line = [], (a.get("type") or "").lower(), line
        if tag == "title":
            self._buf, self._kind, self._line = [], "title", line

    def handle_endtag(self, tag):
        if tag == "head":
            self.in_head = False
        if tag == "a" and self._a is not None:
            line, label, text = self._a
            if not (label or text.strip()):
                self.problems.append(f"{line}: link without text or aria-label")
            self._a = None
        if tag in ("script", "title") and self._buf is not None:
            text = "".join(self._buf)
            if self._kind == "title":
                self.titles.append(text)
            elif self._kind == "application/ld+json":
                self.json_ld.append((self._line, text))
            elif self._kind != "application/json":
                self.inline_scripts.append(text)
            self._buf = None

    def handle_data(self, data):
        if self._buf is not None:
            self._buf.append(data)
        if self._a is not None:
            self._a[2] += data


REQUIRED = {
    "Organization": ("name", "url", "founder", "sameAs", "logo"),
    "Person": ("name", "url", "sameAs", "jobTitle", "worksFor"),
    "WebSite": ("url", "name", "publisher"),
    "SoftwareApplication": ("name", "softwareVersion", "author", "publisher", "url", "license", "codeRepository", "releaseNotes"),
    "BlogPosting": ("headline", "datePublished", "author", "publisher", "mainEntityOfPage"),
    "WebPage": ("url", "name", "isPartOf"),
    "TechArticle": ("headline", "about", "author", "publisher", "mainEntityOfPage"),
    "Dataset": ("name", "description", "creator", "distribution", "measurementTechnique", "temporalCoverage"),
    "WebAPI": ("name", "documentation", "provider"),
    "SoftwareSourceCode": ("name", "programmingLanguage", "codeSampleType"),
    "BreadcrumbList": ("itemListElement",),
}
BINDING = {
    "Organization": {"name": "TEJL", "url": "https://tejl.com/", "sameAs": ["https://tejl.hr/"]},
    "Person": {"name": "Eris Margeta", "url": "https://tejl.com/", "sameAs": ["https://github.com/Eris-Margeta"], "jobTitle": "Founder"},
}


def check_ld(label: str, data, version: str, errors: list[str]) -> None:
    graph = data.get("@graph") if isinstance(data, dict) else None
    if not isinstance(graph, list):
        errors.append(f"{label}: JSON-LD has no @graph")
        return
    ids = {n.get("@id") for n in graph}
    counts: dict[str, int] = {}

    def refs(v):
        if isinstance(v, dict):
            if set(v) == {"@id"}:
                yield v["@id"]
            else:
                for x in v.values():
                    yield from refs(x)
        elif isinstance(v, list):
            for x in v:
                yield from refs(x)
    for node in graph:
        kind = node.get("@type")
        counts[kind] = counts.get(kind, 0) + 1
        for f in REQUIRED.get(kind, ()):
            if f not in node:
                errors.append(f"{label}: JSON-LD {kind} lacks {f}")
        for k, want in BINDING.get(kind, {}).items():
            if node.get(k) != want:
                errors.append(f"{label}: JSON-LD {kind}.{k} is {node.get(k)!r}, binding value {want!r}")
        if kind == "Person" and node.get("worksFor") != {"@id": ORIGIN + "/#organization"}:
            errors.append(f"{label}: Person.worksFor must be the Organization")
        if kind == "Organization" and node.get("founder") != {"@id": ORIGIN + "/#author"}:
            errors.append(f"{label}: Organization.founder must be the Person")
        if kind == "SoftwareApplication":
            if node.get("softwareVersion") != version:
                errors.append(f"{label}: softwareVersion {node.get('softwareVersion')} != VERSION {version}")
            if node.get("author") != {"@id": ORIGIN + "/#author"}:
                errors.append(f"{label}: SoftwareApplication.author must be the Person")
            if "downloadUrl" in node:
                errors.append(f"{label}: SoftwareApplication has downloadUrl, but there are no binaries")
        for t in refs({k: v for k, v in node.items() if k != "@id"}):
            if t not in ids:
                errors.append(f"{label}: JSON-LD @id {t} does not resolve")
        for k in ("@id", "url"):
            v = node.get(k)
            if isinstance(v, str) and "dctx.internet.technology" in v and not v.startswith(ORIGIN):
                errors.append(f"{label}: JSON-LD {kind}.{k} {v} is not on {ORIGIN}")
    for kind in ("Organization", "Person", "SoftwareApplication", "WebSite"):
        if counts.get(kind) != 1:
            errors.append(f"{label}: {counts.get(kind, 0)} {kind} nodes, expected exactly 1")


def contrast(css: str, errors: list[str]) -> list[str]:
    def block(sel: str) -> dict:
        m = re.search(re.escape(sel) + r"\s*\{([^}]*)\}", css)
        return dict(re.findall(r"--([a-z0-9-]+):\s*(#[0-9a-f]{6})", m.group(1))) if m else {}
    light, dark = block(":root"), block(':root[data-theme="dark"]')

    def lum(hexv: str) -> float:
        rgb = [int(hexv[i:i + 2], 16) / 255 for i in (1, 3, 5)]
        lin = [c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4 for c in rgb]
        return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2]
    def ratio_of(x: str, y: str) -> float:
        a, b = sorted((lum(x), lum(y)), reverse=True)
        return (a + 0.05) / (b + 0.05)
    out = []
    # Body text and links in it at least 7:1 (WCAG AAA); secondary text at least 4.5:1 (design review R3, section 7).
    minimum = {"ink": 7.0, "brand-text": 7.0, "muted": 4.5}
    for name, theme in (("light", light), ("dark", dark)):
        for fg, bg in (("ink", "bg"), ("muted", "bg"), ("brand-text", "bg"), ("ink", "surface"), ("ink", "surface-2"),
                       ("muted", "surface"), ("muted", "surface-2")):
            if fg not in theme or bg not in theme:
                errors.append(f"contrast: --{fg} or --{bg} missing in the {name} theme")
                continue
            a, b = sorted((lum(theme[fg]), lum(theme[bg])), reverse=True)
            ratio = (a + 0.05) / (b + 0.05)
            out.append(f"{name} {fg}/{bg} {ratio:.2f}")
            if ratio < minimum[fg]:
                errors.append(f"contrast: {name} --{fg} on --{bg} is {ratio:.2f}:1 (< {minimum[fg]})")
        # Syntax colours (highlight.py, the .t-* classes) at least 4.5:1 on both code backgrounds.
        hl = sorted(k for k in theme if k.startswith("hl-"))
        if len(hl) < 9:
            errors.append(f"contrast: {name} theme defines {len(hl)} --hl-* syntax colours, expected 9")
        for fg in hl:
            worst = min(ratio_of(theme[fg], theme.get(bg, "#000000")) for bg in ("surface", "surface-2", "bg"))
            if worst < 4.5:
                errors.append(f"contrast: {name} --{fg} is {worst:.2f}:1 on a code background (< 4.5)")
        if hl:
            out.append(f"{name} {len(hl)} syntax colours >= 4.5")
    return out


def page_label(path: str) -> str:
    return path.lstrip("/") + ("index.html" if path.endswith("/") else "")


def llms_fresh(site: Path, errors: list[str], routes=None) -> None:
    """Every built HTML page must remain discoverable and its extracted text readable.

    Check the output, not a second rendering of llms-full: stale or missing text must
    fail even when the generator itself stops adding a page. Verbatim document text
    can live in the pack; every other extracted line must be present as well.
    """
    import llms
    from routes import build_routes
    from pages.news_content import NEWS
    routes = build_routes(NEWS) if routes is None else routes
    texts = {name: (site / name).read_text() for name in LLMS_LIMIT}
    full = llms.normalize(texts["llms-full.txt"])
    paths = {r.path for r in routes}
    for page in sorted(site.rglob("*.html")):
        rel = page.relative_to(site).as_posix()
        path = "/" + rel.removesuffix("index.html") if page.name == "index.html" else "/" + rel
        url = ORIGIN + path
        if path not in paths:
            errors.append(f"llms freshness: {path} is missing from the route table")
        for name in ("llms.txt", "llms-full.txt"):
            if url not in texts[name]:
                errors.append(f"llms freshness: {name} does not mention {url}")
        extracted = llms.extract_page(page.read_text())
        missing = [line for line in extracted["lines"] if line not in full]
        if missing:
            errors.append(f"llms freshness: llms-full.txt is missing extracted text from {path}: {missing[0][:100]!r} "
                          f"({len(missing)} lines)")
    positions = []
    for r in routes:
        page = site / (r.path.lstrip("/") + ("index.html" if r.path.endswith("/") else ""))
        if not page.is_file():
            errors.append(f"llms freshness: route {r.path} has no HTML page")
            continue
        url = ORIGIN + r.path
        header = f"url: {url}\ntitle: {r.title}\n"
        if header not in texts["llms-full.txt"]:
            errors.append(f"llms freshness: llms-full.txt has no page header for {url}")
        else:
            positions.append(texts["llms-full.txt"].index(header))
        desc = llms.extract_page(page.read_text())["description"]
        if not desc or f"- [{r.title}]({url}): {desc}\n" not in texts["llms.txt"]:
            errors.append(f"llms freshness: llms.txt needs {url} with its meta description")
        if r.path.count("/") <= 2 and f"]({url})" not in texts["llms-small.txt"]:
            errors.append(f"llms freshness: llms-small.txt does not link to top-level page {url}")
    if positions != sorted(positions):
        errors.append("llms freshness: llms-full.txt pages are not in route-table order")
    # facts_fresh separately checks facts.json against VERSION, CHANGELOG.md and git.
    for name, text in texts.items():
        if llms.facts_for_file(name) not in text:
            errors.append(f"llms freshness: {name} facts differ from data/facts.json (version, release tag or date)")


def forbidden_glyphs(site: Path, errors: list[str]) -> None:
    """Scan the served text, including entity/JS/CSS escaped icons, before browsers decode it."""
    for path in sorted(site.rglob("*")):
        if path.suffix not in (".html", ".css", ".js", ".svg", ".json", ".md", ".txt", ".xml", ".webmanifest"):
            continue
        text = path.read_text(encoding="utf-8")
        if path.suffix in (".html", ".svg"):
            text = html.unescape(text)
        elif path.suffix == ".js":
            text = re.sub(r"\\u\{([0-9a-fA-F]{1,6})\}|\\u([0-9a-fA-F]{4})|\\x([0-9a-fA-F]{2})",
                          lambda m: chr(int(next(g for g in m.groups() if g), 16)), text)
            # JS can construct supplementary characters from UTF-16 surrogate pairs.
            text = text.encode("utf-16", "surrogatepass").decode("utf-16", "surrogatepass")
        elif path.suffix == ".css":
            text = re.sub(r"\\([0-9a-fA-F]{1,6})\s?", lambda m: chr(int(m[1], 16)), text)
        hits = sorted({ord(c) for c in FORBIDDEN.findall(text)})
        if hits:
            errors.append(f"{path.relative_to(site)}: forbidden glyphs " + ", ".join(f"U+{c:04X}" for c in hits))


def check_social_head(label: str, meta: list[dict], site: Path, errors: list[str]) -> None:
    """Keep each image's structured OG properties attached to that image, in order."""
    images = []
    for m in meta:
        prop = m.get("property", "")
        if prop == "og:image":
            images.append({"url": m.get("content", "")})
        elif prop.startswith("og:image:") and images:
            key = prop.removeprefix("og:image:")
            if key in images[-1]:
                errors.append(f"{label}: duplicate {prop}")
            images[-1][key] = m.get("content", "")
    if len(images) != 2:
        errors.append(f"{label}: need exactly two og:image groups (GIF, PNG)")
    for group, mime, ext in zip(images, ("image/gif", "image/png"), (".gif", ".png")):
        if (group.get("type"), group.get("width"), group.get("height")) != (mime, "1200", "630") or not group.get("alt"):
            errors.append(f"{label}: OG {mime} needs type, 1200x630 and alt")
        if not urlsplit(group["url"]).path.endswith(ext):
            errors.append(f"{label}: og:image must list GIF first, PNG second")
    names = {}
    for m in meta:
        if m.get("name", "").startswith("twitter:"):
            if m["name"] in names:
                errors.append(f"{label}: duplicate {m['name']}")
            names[m["name"]] = m.get("content", "")
    if names.get("twitter:card") != "summary_large_image" or not names.get("twitter:image:alt"):
        errors.append(f"{label}: Twitter needs summary_large_image and image alt")
    twitter = names.get("twitter:image", "")
    if not urlsplit(twitter).path.endswith(".twitter.png") or twitter in [g["url"] for g in images]:
        errors.append(f"{label}: Twitter needs its own static PNG composition")
    if len(images) == 2:
        stem = urlsplit(images[0]["url"]).path.removesuffix(".gif")
        if urlsplit(images[1]["url"]).path != stem + ".png" or urlsplit(twitter).path != stem + ".twitter.png":
            errors.append(f"{label}: GIF, fallback and Twitter must belong to the same page card")
    for url in [g["url"] for g in images] + [twitter]:
        parsed = urlsplit(url)
        path = site / parsed.path.lstrip("/")
        if parsed.scheme != "https" or parsed.netloc != HOST or not path.is_file():
            errors.append(f"{label}: missing or noncanonical social asset {url}")
        elif parsed.query != "v=" + hashlib.sha256(path.read_bytes()).hexdigest()[:12]:
            errors.append(f"{label}: social image must be cache-busted by its bytes: {url}")


def check_social_images(site: Path, docs: dict, errors: list[str], report: dict) -> None:
    from PIL import Image, ImageChops, ImageStat
    checked = set()
    pngs = sorted(site.glob("assets/og/*.png"))
    gifs = sorted(site.glob("assets/og/*.gif"))
    for path in pngs:
        with Image.open(path) as image:
            size = (1200, 600) if path.name.endswith(".twitter.png") else (1200, 630)
            if image.format != "PNG" or image.size != size:
                errors.append(f"{path.name}: must be PNG {size}, got {image.format} {image.size}")
        if path.stat().st_size > 200_000:
            errors.append(f"{path.name}: PNG exceeds 200 KB")
    for doc in docs.values():
        for m in doc.meta:
            if m.get("property") != "og:image":
                continue
            path = site / urlsplit(m.get("content", "")).path.lstrip("/")
            if path.suffix != ".gif" or path in checked or not path.is_file():
                continue
            checked.add(path)
            if path.stat().st_size >= 5_000_000:
                errors.append(f"{path.name}: GIF must be under 5 MB")
            with Image.open(path) as image:
                if image.format != "GIF" or image.size != (1200, 630) or image.n_frames < 2 or image.info.get("loop") != 0:
                    errors.append(f"{path.name}: need animated 1200x630 GIF with infinite loop")
                frames, duration = [], 0
                for i in range(image.n_frames):
                    image.seek(i)
                    duration += image.info.get("duration", 0)
                    frames.append(image.convert("RGB"))
                if not 2000 <= duration <= 4000:
                    errors.append(f"{path.name}: loop is {duration} ms; expected 2-4 seconds")
                fallback = path.with_suffix(".png")
                if not fallback.is_file():
                    errors.append(f"{path.name}: no PNG fallback")
                else:
                    with Image.open(fallback) as still:
                        if frames[0].tobytes() != still.convert("RGB").tobytes():
                            errors.append(f"{path.name}: frame 0 differs from PNG fallback")
                differences = [sum(ImageStat.Stat(ImageChops.difference(a, b)).mean)
                               for a, b in zip(frames, frames[1:] + frames[:1])]
                if not any(differences):
                    errors.append(f"{path.name}: animation has no motion")
                elif differences[-1] > max(differences[:-1]) * 2:
                    errors.append(f"{path.name}: discontinuity at the loop seam")
    if checked != set(gifs):
        errors.append("social GIFs must all be referenced by a page")
    report["og"] = {"png": len(pngs), "gif": len(gifs), "largest_gif": max((f.stat().st_size for f in gifs), default=0),
                    "largest_png": max((f.stat().st_size for f in pngs), default=0)}


def check_site(site: Path, errors: list[str], report: dict) -> None:
    from pages.news_content import NEWS
    from routes import DOCS, POLICIES, build_routes
    version = (ROOT / "VERSION").read_text().strip()
    routes = build_routes(NEWS)
    forbidden_glyphs(site, errors)
    kinds = {page_label(r.path): r.kind for r in routes}
    docs: dict[Path, Doc] = {}
    for path in sorted(site.rglob("*.html")):
        d = Doc()
        d.feed(path.read_text(encoding="utf-8"))
        d.close()
        docs[path] = d
    referenced: set[str] = set()
    css = next(site.glob("assets/site.*.css")).read_text()
    theme_hash = None
    for path, d in docs.items():
        label = path.relative_to(site).as_posix()
        url = ORIGIN + "/" + label.removesuffix("index.html")
        if d.h1s != 1:
            errors.append(f"{label}: {d.h1s} h1 elements")
        if d.lang != "en" or not d.viewport:
            errors.append(f"{label}: missing lang or viewport")
        if not any(t.strip() for t in d.titles):
            errors.append(f"{label}: empty title")
        if len(d.canonicals) != 1 or not d.canonicals[0].startswith(ORIGIN + "/"):
            errors.append(f"{label}: canonical must be one link under {ORIGIN}/")
        errors.extend(f"{label}:{p}" for p in d.problems)
        if len(d.inline_scripts) != 1:
            errors.append(f"{label}: {len(d.inline_scripts)} inline scripts; only the theme script is allowed")
        for s in d.inline_scripts:
            h = "sha256-" + base64.b64encode(hashlib.sha256(s.encode()).digest()).decode()
            theme_hash = theme_hash or h
            if h != theme_hash:
                errors.append(f"{label}: inline script differs from the theme script")
        if not d.json_ld:
            errors.append(f"{label}: no JSON-LD")
        for line, text in d.json_ld:
            try:
                check_ld(f"{label}:{line}", json.loads(text), version, errors)
            except ValueError as error:
                errors.append(f"{label}:{line}: invalid JSON-LD: {error}")
        metas = {(m.get("property") or m.get("name")): m.get("content") for m in d.meta}
        og_images = [m.get("content") for m in d.meta if m.get("property") == "og:image"]
        check_social_head(label, d.meta, site, errors)
        for key in ("og:title", "og:description", "og:url", "og:image:alt", "twitter:card", "twitter:image", "twitter:image:alt", "description"):
            if not metas.get(key):
                errors.append(f"{label}: no {key}")
        if metas.get("og:url") != d.canonicals[0] if d.canonicals else True:
            errors.append(f"{label}: og:url differs from the canonical link")
        if (metas.get("twitter:image") or "").endswith(".gif"):
            errors.append(f"{label}: twitter:image must be static")
        for img in og_images + [metas.get("twitter:image") or ""]:
            if not (site / urlsplit(img).path.lstrip("/")).is_file():
                errors.append(f"{label}: OG image {img} missing")
        for line, tag, attr, value in d.refs:
            loc = f"{label}:{line}: {attr}={value!r}"
            if value.startswith("mailto:"):
                if not value.startswith(ALLOWED_EXTERNAL):
                    errors.append(f"{loc}: mail address not allowed")
                continue
            target = urlsplit(urljoin(url, value))
            if target.scheme in ("http", "https") and target.hostname != HOST:
                if not (value.startswith(ALLOWED_EXTERNAL) and tag in ("a", "link")):
                    errors.append(f"{loc}: external {'link' if tag == 'a' else 'resource'} not on the allowlist")
                continue
            if target.scheme not in ("http", "https"):
                continue
            pathpart = unquote(target.path)
            if pathpart.startswith("/api/"):
                continue
            referenced.add(pathpart)
            file = site / pathpart.lstrip("/")
            if file.is_dir():
                file = file / "index.html"
            if not file.is_file():
                errors.append(f"{loc}: missing {pathpart}")
            elif target.fragment and (file not in docs or unquote(target.fragment) not in docs[file].ids):
                errors.append(f"{loc}: missing fragment #{target.fragment}")
        # first-load requests, bytes and the largest asset (an empty cache; lazy images excluded)
        loaded = [l["href"] for l in d.links if (l.get("rel") or "") in ("stylesheet", "preload")] + d.scripts
        loaded += [i["src"] for i in d.images if i.get("loading") != "lazy"]
        size, largest, requests = path.stat().st_size, (path.stat().st_size, label), 1
        for res in loaded:
            f = site / urlsplit(res).path.lstrip("/")
            if f.is_file():
                requests += 1
                size += f.stat().st_size
                largest = max(largest, (f.stat().st_size, res))
        budget = BUDGET.get("/" + label.removesuffix("index.html"), BUDGET["*"])
        if requests > budget["requests"]:
            errors.append(f"{label}: {requests} requests on first load (budget {budget['requests']})")
        if size > budget["bytes"]:
            errors.append(f"{label}: {size:,} bytes on first load (budget {budget['bytes']:,})")
        if largest[0] > LARGEST and largest[1] != label:
            errors.append(f"{label}: largest first-load asset {largest[1]} is {largest[0]:,} bytes (> {LARGEST:,})")
        limit = HTML_LIMIT.get(label, 120_000)
        if path.stat().st_size > limit:
            errors.append(f"{label}: HTML is {path.stat().st_size:,} bytes (limit {limit:,})")
        report.setdefault("pages", {})[label] = (requests, size, largest)
        text = strip_removed_list(path.read_text(encoding="utf-8"))
        plain = re.sub(r"<[^>]+>", " ", text)
        if LEGACY_RULES.search(text):
            errors.append(f"{label}: mentions a legacy rule file")
        handwritten = kinds.get(label) != "doc" and label != "cli/index.html"
        if handwritten and LEGACY_CALL.search(re.sub(r"<[^>]+>", "", text)):
            errors.append(f"{label}: a hand-written page shows a legacy `dctx .` command")
        if kinds.get(label) not in ("doc", "changelog", "post"):
            removed_outside_list(label, html.unescape(re.sub(r"<[^>]+>", "", text)), errors)
        if kinds.get(label) != "doc" and SALES.search(plain):
            errors.append(f"{label}: sales wording: {SALES.search(plain).group(0)!r}")
    report["pages_checked"] = len(docs)
    nginx = (ROOT / "tools/playground/nginx.conf").read_text()
    csp = re.search(r'add_header Content-Security-Policy "([^"]+)"', nginx)
    if not csp or "default-src 'none'" not in csp.group(1) or "frame-ancestors 'none'" not in csp.group(1):
        errors.append("nginx.conf: no Content-Security-Policy with default-src 'none'")
    elif theme_hash and f"'{theme_hash}'" not in csp.group(1):
        errors.append(f"nginx.conf: CSP script-src lacks the theme script hash '{theme_hash}'")
    for header in ("Strict-Transport-Security", "X-Content-Type-Options", "Referrer-Policy", "Permissions-Policy", "immutable", "gzip on"):
        if header not in nginx:
            errors.append(f"nginx.conf: no {header}")
    report["theme_hash"] = theme_hash
    total_css = sum(f.stat().st_size for f in site.glob("assets/*.css"))
    js_sources = sum(f.stat().st_size for f in (HERE / "src").glob("*.js"))
    biggest_bundle = max(f.stat().st_size for f in site.glob("assets/*.js"))
    hero, term = (HERE / "src/hero.js").stat().st_size, (HERE / "src/term.js").stat().st_size
    if total_css > 48_000:
        errors.append(f"CSS total {total_css:,} bytes > 48 KB")
    if js_sources > 60_000 or biggest_bundle > 60_000:
        errors.append(f"JS: {js_sources:,} bytes of sources, largest bundle {biggest_bundle:,} (limit 60 KB)")
    if hero > 18_432 or term > 6_144:
        errors.append(f"hero.js {hero:,} bytes (limit 18 KB) or term.js {term:,} (limit 6 KB)")
    report["sizes"] = {"css": total_css, "js_sources": js_sources, "largest_bundle": biggest_bundle, "hero_js": hero, "term_js": term}
    css_text = "".join(f.read_text() for f in site.glob("assets/*.css"))
    for f in site.glob("assets/*.*"):
        if f.suffix == ".woff2" and f.stat().st_size > 40_000:
            errors.append(f"font {f.name} is {f.stat().st_size:,} bytes > 40 KB")
        if f.suffix in (".css", ".js", ".woff2"):
            if not re.search(r"\.[0-9a-f]{8}\.(css|js|woff2)$", f.name):
                errors.append(f"asset {f.name} is not content-hashed")
            elif f"/assets/{f.name}" not in referenced and f.name not in css_text:
                errors.append(f"asset {f.name} is not referenced (orphan)")
    for ref in referenced:
        if re.match(r"^/assets/[^/]+\.(css|js|woff2)$", ref) and not re.search(r"\.[0-9a-f]{8}\.", ref):
            errors.append(f"un-hashed asset referenced: {ref}")
    check_social_images(site, docs, errors, report)
    llms_texts = {name: (site / name).read_text() for name in LLMS_LIMIT}
    llms_fresh(site, errors, routes)
    for name, text in llms_texts.items():
        if len(text.encode()) > LLMS_LIMIT[name]:
            errors.append(f"{name} is {len(text.encode()):,} bytes > {LLMS_LIMIT[name]:,}")
        head = re.split(r"^## (?:Documents|Outline of the start documents)$", text, maxsplit=1, flags=re.M)[0]
        markers = ("**Facts**", "## Overview") if name == "llms.txt" else ("## Facts", "## Pages")
        if not head.startswith("# dctx\n\n> ") or any(marker not in head for marker in markers):
            errors.append(f"{name}: does not start with the title, the facts block and the route table")
        for u in sorted({x.rstrip(".,;:") for x in re.findall(r"https?://[^\s)`]+", head)}):
            p = urlsplit(u)
            if p.hostname != HOST:
                if not u.startswith(ALLOWED_EXTERNAL):
                    errors.append(f"{name}: external link {u} not on the allowlist")
                continue
            f = site / unquote(p.path).lstrip("/")
            page = f / "index.html" if f.is_dir() else f
            if not page.is_file():
                errors.append(f"{name}: {u} does not resolve to a built file")
            elif p.fragment and (page not in docs or unquote(p.fragment) not in docs[page].ids):
                errors.append(f"{name}: {u}: missing fragment")
        removed_outside_list(name, text, errors)
    llms = llms_texts["llms.txt"]
    full = llms_texts["llms-full.txt"]
    report["llms_full_bytes"] = len(full.encode())
    report["llms_bytes"] = len(llms.encode())
    from routes import doc_path
    twins = [(site / doc_path(slug).strip("/") / "index.md", file) for slug, file, *_ in DOCS]
    twins += [(site / path.strip("/") / "index.md", file) for path, file, *_ in POLICIES]
    for twin, source in twins:
        check_twin(site, twin, source, full, errors)
    report["twins"] = len(twins)
    indexable = {ORIGIN + r.path for r in routes if r.indexable}
    try:
        locs = {e.text for e in ET.parse(site / "sitemap.xml").getroot().iter("{http://www.sitemaps.org/schemas/sitemap/0.9}loc")}
        if locs != indexable:
            errors.append(f"sitemap.xml differs from the indexable pages: {sorted(locs ^ indexable)[:4]}")
        for feed in ("news/feed.xml", "changelog/feed.xml"):
            if not ET.parse(site / feed).getroot().findall("{http://www.w3.org/2005/Atom}entry"):
                errors.append(f"{feed}: no entries")
        ET.parse(site / "opensearch.xml")
    except ET.ParseError as error:
        errors.append(f"XML does not parse: {error}")
    sec = (site / ".well-known/security.txt").read_text()
    m = re.search(r"^Expires: (\S+)$", sec, re.M)
    now = dt.datetime.now(dt.timezone.utc)
    expires = dt.datetime.fromisoformat(m.group(1).replace("Z", "+00:00")) if m else None
    if not expires or not (now < expires <= now + dt.timedelta(days=366)):
        errors.append(f"security.txt: Expires {m.group(1) if m else None} must be in the future and at most a year ahead")
    robots = (site / "robots.txt").read_text()
    if f"Sitemap: {ORIGIN}/sitemap.xml" not in robots or "Disallow: /" in robots:
        errors.append("robots.txt: wrong Sitemap line or a Disallow")
    for f in ("humans.txt", "build.json", "manifest.webmanifest", "opensearch.xml", "favicon.svg", "favicon-32.png",
              "apple-touch-icon.png", "assets/search-index.json", "assets/openapi.json", "llms-small.txt", "llms-full.txt"):
        if not (site / f).is_file():
            errors.append(f"missing {f}")
    json.loads((site / "build.json").read_text())
    json.loads((site / "manifest.webmanifest").read_text())
    if (site / "assets/search-index.json").stat().st_size > 300_000:
        errors.append("search-index.json > 300 KB")
    for f in sorted(site.rglob("*")):
        if not f.is_file() or f.suffix not in (".txt", ".md", ".json", ".xml", ".html"):
            continue
        data = f.read_bytes()
        if b"example-not-a-secret" in data:
            errors.append(f"{f.relative_to(site)}: contains the sample's .env value")
        if re.search(rb"/srv/|/home/[a-z]", data):
            errors.append(f"{f.relative_to(site)}: absolute host path")
        if re.search(rb"https?://dctx\.internet\.technology", data) and f.suffix != ".md" and f.name != "llms-full.txt":
            errors.append(f"{f.relative_to(site)}: bare-host URL; the canonical host is {HOST}")
    report["contrast"] = contrast(css, errors)


def check_twin(site: Path, twin: Path, source: str, full: str, errors: list[str]) -> None:
    import md
    label = twin.relative_to(site).as_posix()
    if not twin.is_file():
        errors.append(f"missing Markdown twin {label}")
        return
    text = twin.read_text()
    if md.strip_targets(text) != md.strip_targets(plain_glyphs((ROOT / source).read_text())):
        errors.append(f"{label}: drifts from {source} beyond link targets")
    import llms
    tier = llms.tier("llms-full.txt")
    if source not in tier["dropped"] + tier.get("altered", []) and plain_glyphs((ROOT / source).read_text()).rstrip("\n") not in full:
        errors.append(f"llms-full.txt does not contain {source} (the dctx pack of the documents)")
    page = (twin.parent / "index.html").read_text()
    if f"/blob/main/{source}" not in page or f"/edit/main/{source}" not in page:
        errors.append(f"{twin.parent.relative_to(site)}/: no View source / Edit on GitHub link to {source}")


def dates_fresh(errors: list[str], warnings: list[str]) -> None:
    try:
        shallow = git("rev-parse", "--is-shallow-repository").strip() == b"true"
    except (subprocess.CalledProcessError, OSError):
        warnings.append("dates.json not checked: no git")
        return
    if shallow:
        warnings.append("dates.json not checked: shallow clone")
        return
    for rel, iso in json.loads((HERE / "data/dates.json").read_text()).items():
        real = git("log", "-1", "--format=%cI", "--", rel).decode().strip()
        if real != iso:
            errors.append(f"dates.json: {rel} is {iso}, git says {real}; run build.py --refresh-dates")


def main(argv: list[str]) -> int:
    errors: list[str] = []
    warnings: list[str] = []
    report: dict = {}
    files, hits = 0, -1
    try:
        with tempfile.TemporaryDirectory(prefix="dctx-sitecheck-") as tmp:
            site = rebuild(Path(tmp))
            files = staleness(site, errors, "--working-tree" in argv)
            check_site(site, errors, report)
        capture_drift(errors, warnings, "--no-capture" in argv)
        llms_drift(errors, warnings, "--no-capture" in argv)
        facts_fresh(errors, warnings)
        readme_drift(errors)
        hits = deny_list(errors, warnings)
        dates_fresh(errors, warnings)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError, StopIteration) as error:
        errors.append(f"check could not complete: {error}")
    for w in warnings:
        print(f"WARN: {w}")
    if report:
        home = report.get("pages", {}).get("index.html", (0, 0, (0, "")))
        print(f"Checked {files} files, {report.get('pages_checked')} HTML pages, {report.get('twins')} Markdown twins against their sources.")
        print(f"Home first load: {home[0]} requests, {home[1]:,} bytes, largest {home[2][1]} ({home[2][0]:,} bytes).")
        print(f"Sizes: {report.get('sizes')}. OG: {report.get('og')}. llms.txt {report.get('llms_bytes')} bytes, llms-full.txt {report.get('llms_full_bytes', 0):,} bytes.")
        print(f"Contrast: {', '.join(report.get('contrast', []))}.")
    print(f"deny-list hits: {hits if hits >= 0 else 'not checked'}")
    if errors:
        for e in errors:
            print(f"- {e}", file=sys.stderr)
        print(f"FAIL: {len(errors)} errors")
        return 1
    reference = "website/" if "--working-tree" in argv else "HEAD"
    print(f"PASS: 0 errors; the generated website matches {reference} and every check passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
