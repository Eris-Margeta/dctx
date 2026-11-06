#!/usr/bin/env python3
"""Rebuild in isolation and check the committed website (Python stdlib only).

The builder owns website/ except assets/ and .gitignore. Seed those static
inputs in a temporary copy of tracked repository files, then run build.py's
entry point. Never write to the real website or trust old generated pages.
"""
from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urljoin, urlsplit

ROOT = Path(__file__).resolve().parents[2]
ORIGIN = "https://www.dctx.internet.technology"


def git(*args: str) -> bytes:
    return subprocess.run(
        ["git", "-C", str(ROOT), *args], check=True, capture_output=True
    ).stdout


def rebuild(destination: Path) -> Path:
    """Copy tracked working-tree inputs; exclude all previous generated output."""
    for raw in git("ls-files", "-z").split(b"\0"):
        if not raw:
            continue
        relative = Path(raw.decode("utf-8"))
        if relative.parts[0] == "website" and not (
            relative.parts[1] in {"assets", ".gitignore"}
        ):
            continue
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / relative, target)
    result = subprocess.run(
        [sys.executable, str(destination / "tools/website/build.py")],
        cwd=destination, capture_output=True, text=True,
    )
    if result.returncode:
        raise RuntimeError(
            f"build.py exited {result.returncode}:\n{result.stdout}{result.stderr}"
        )
    return destination / "website"


def check_staleness(site: Path, errors: list[str]) -> int:
    committed = {
        name.decode("utf-8").removeprefix("website/")
        for name in git("ls-tree", "-r", "--name-only", "-z", "HEAD", "--", "website").split(b"\0")
        if name
    }
    generated = {p.relative_to(site).as_posix() for p in site.rglob("*") if p.is_file()}
    for name in sorted(committed | generated):
        if name not in generated:
            errors.append(f"stale: website/{name} is committed but no longer generated")
        elif name not in committed:
            errors.append(f"stale: website/{name} is generated but not committed")
        elif (site / name).read_bytes() != git("show", f"HEAD:website/{name}"):
            errors.append(f"stale: website/{name} differs from the committed bytes")
    return len(generated)


class Document(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.ids: set[str] = set()
        self.references: list[tuple[int, str, str, str]] = []
        self.h1s = 0
        self.titles: list[list[str]] = []
        self.canonicals: list[str] = []
        self.json_ld: list[tuple[int, str]] = []
        self.base: str | None = None
        self._title = False
        self._json: list[str] | None = None
        self._json_line = 0

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        attributes = dict(attrs)
        if attributes.get("id"):
            self.ids.add(attributes["id"])
        if tag == "a" and attributes.get("name"):
            self.ids.add(attributes["name"])
        for attribute, value in attrs:
            if attribute in {"href", "src"} and value is not None:
                self.references.append((self.getpos()[0], tag, attribute, value))
        if tag == "base" and self.base is None:
            self.base = attributes.get("href")
        if tag == "h1":
            self.h1s += 1
        if tag == "title":
            self.titles.append([])
            self._title = True
        if tag == "link" and "canonical" in (attributes.get("rel") or "").lower().split():
            self.canonicals.append(attributes.get("href") or "")
        if tag == "script" and (attributes.get("type") or "").strip().lower() == "application/ld+json":
            self._json = []
            self._json_line = self.getpos()[0]

    def handle_endtag(self, tag: str) -> None:
        if tag == "title":
            self._title = False
        if tag == "script" and self._json is not None:
            self.json_ld.append((self._json_line, "".join(self._json)))
            self._json = None

    def handle_data(self, data: str) -> None:
        if self._title:
            self.titles[-1].append(data)
        if self._json is not None:
            self._json.append(data)


REQUIRED = {
    "Organization": ("name", "url", "founder"),
    "Person": ("name", "url", "sameAs", "jobTitle", "worksFor"),
    "WebSite": ("url", "name", "publisher"),
    "SoftwareApplication": ("name", "softwareVersion", "author", "url", "license"),
    "BlogPosting": ("headline", "datePublished", "author", "publisher", "mainEntityOfPage"),
    "WebPage": ("url", "name"),
}


def check_json_ld(label: str, line: int, data: object, errors: list[str]) -> None:
    """Offline structured-data check: @graph ids resolve, required fields are present."""
    graph = data.get("@graph") if isinstance(data, dict) else None
    if not isinstance(graph, list):
        errors.append(f"{label}:{line}: JSON-LD has no @graph list")
        return
    ids = {n.get("@id") for n in graph if isinstance(n, dict)}

    def refs(value: object):
        if isinstance(value, dict):
            if set(value) == {"@id"}:
                yield value["@id"]
            else:
                for inner in value.values():
                    yield from refs(inner)
        elif isinstance(value, list):
            for inner in value:
                yield from refs(inner)

    for node in graph:
        if not isinstance(node, dict):
            errors.append(f"{label}:{line}: JSON-LD graph node is not an object")
            continue
        kind = node.get("@type")
        for field in REQUIRED.get(kind, ()):
            if field not in node:
                errors.append(f"{label}:{line}: JSON-LD {kind} {node.get('@id')} lacks {field!r}")
        for target in refs({k: v for k, v in node.items() if k != "@id"}):
            if target not in ids:
                errors.append(f"{label}:{line}: JSON-LD @id {target} does not resolve in this page's graph")


def check_documents(site: Path, errors: list[str]) -> tuple[int, int, int, int]:
    documents: dict[Path, Document] = {}
    pages = references = fragments = blocks = 0
    for path in sorted(site.rglob("*")):
        if path.suffix.lower() not in {".html", ".htm", ".svg", ".xml"}:
            continue
        document = Document()
        document.feed(path.read_text(encoding="utf-8"))
        document.close()
        documents[path] = document
        label = path.relative_to(site).as_posix()
        if path.suffix.lower() in {".html", ".htm"}:
            pages += 1
            if document.h1s != 1:
                errors.append(f"{label}: expected exactly one <h1>, found {document.h1s}")
            if not any("".join(title).strip() for title in document.titles):
                errors.append(f"{label}: missing or empty <title>")
            if len(document.canonicals) != 1 or not document.canonicals[0].startswith(ORIGIN + "/"):
                errors.append(f"{label}: expected one canonical link under {ORIGIN}/")
        if document._json is not None:
            errors.append(f"{label}:{document._json_line}: unclosed JSON-LD script")
        for line, data in document.json_ld:
            blocks += 1
            try:
                check_json_ld(label, line, json.loads(data), errors)
            except ValueError as error:
                errors.append(f"{label}:{line}: invalid JSON-LD: {error}")

    if not pages:
        errors.append("website: no HTML pages generated")
    for path, document in documents.items():
        label = path.relative_to(site).as_posix()
        page_url = ORIGIN + "/" + label.removesuffix("index.html")
        base_url = urljoin(page_url, document.base) if document.base is not None else page_url
        for line, tag, attribute, value in document.references:
            location = f"{label}:{line}: {attribute}={value!r}"
            try:
                url = urlsplit(urljoin(page_url if tag == "base" else base_url, value))
                if url.scheme not in {"http", "https"} or url.hostname != urlsplit(ORIGIN).hostname:
                    continue
                references += 1
                target = (site / unquote(url.path).lstrip("/")).resolve()
                if not target.is_relative_to(site.resolve()):
                    errors.append(f"{location}: target escapes website/")
                    continue
                if target.is_dir():
                    target /= "index.html"
                if not target.is_file():
                    errors.append(f"{location}: missing file {target.relative_to(site)}")
                    continue
                fragment = unquote(url.fragment)
                if fragment:
                    fragments += 1
                    if target not in documents or fragment not in documents[target].ids:
                        errors.append(f"{location}: missing fragment #{fragment} in {target.relative_to(site)}")
            except ValueError as error:
                errors.append(f"{location}: invalid URL: {error}")
    return pages, references, fragments, blocks


def main() -> int:
    errors: list[str] = []
    try:
        with tempfile.TemporaryDirectory(prefix="dctx-sitecheck-") as temporary:
            site = rebuild(Path(temporary))
            files = check_staleness(site, errors)
            pages, references, fragments, blocks = check_documents(site, errors)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        errors.append(f"check could not complete: {error}")
    else:
        print(f"Checked {files} files, {pages} HTML pages, {references} internal href/src references, "
              f"{fragments} fragments, {blocks} JSON-LD blocks.")
    if errors:
        print(f"FAIL: {len(errors)} failure(s):", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1
    print("PASS: generated website matches HEAD and all site checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
