"""Shared constants and helpers of the website builder.

Every repository file the build reads goes through read() or read_bytes(), so build.json can
carry one digest of all inputs. Nothing here touches the network or the clock.
"""
from __future__ import annotations

import hashlib
import html
import json
import math
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
ORIGIN = "https://www.dctx.internet.technology"  # canonical; the bare host 301-redirects here
HOST = "www.dctx.internet.technology"
ALIASES = ("https://www.dctx.internet.technology", "https://www.dctx.internet.technology")
REPO = "https://github.com/Eris-Margeta/dctx"
BLOB = REPO + "/blob/main/"
TREE = REPO + "/tree/main/"
CONTACT = "info@tejl.com"
ORG_ID = ORIGIN + "/#organization"
PERSON_ID = ORIGIN + "/#author"
SITE_ID = ORIGIN + "/#website"
APP_ID = ORIGIN + "/#software"
LICENSE_URL = "https://www.apache.org/licenses/LICENSE-2.0"

e = html.escape
READS: dict[str, str] = {}


def read_bytes(rel: str) -> bytes:
    data = (ROOT / rel).read_bytes()
    READS[rel] = hashlib.sha256(data).hexdigest()
    return data


def read(rel: str) -> str:
    return read_bytes(rel).decode("utf-8")


VERSION = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
READS["VERSION"] = hashlib.sha256((ROOT / "VERSION").read_bytes()).hexdigest()


def est_tokens(nbytes: int) -> int:
    """The dctx estimate: ceil(2 * UTF-8 bytes / 7)."""
    return math.ceil(2 * nbytes / 7)


_demo: dict | None = None


def demo() -> dict:
    global _demo
    if _demo is None:
        _demo = json.loads(read("tools/website/data/demo.json"))
        if _demo["version"] != VERSION:
            raise SystemExit(f"demo.json was captured from dctx {_demo['version']}, VERSION is {VERSION}; run capture.py")
    return _demo


def rec(rid: str) -> dict:
    for record in demo()["records"]:
        if record["id"] == rid:
            return record
    raise KeyError(rid)


_dates: dict | None = None


def date_of(rel: str) -> str | None:
    """Last commit date (YYYY-MM-DD) of a source file, from data/dates.json."""
    global _dates
    if _dates is None:
        _dates = json.loads(read("tools/website/data/dates.json"))
    value = _dates.get(rel)
    return value[:10] if value else None


def iso_of(rel: str) -> str | None:
    global _dates
    if _dates is None:
        date_of(rel)
    return _dates.get(rel)


def src(path: str, label: str | None = None, line: int | None = None) -> str:
    """A footnote-style source link to a repository file."""
    anchor = f"#L{line}" if line else ""
    text = label or path
    return f'<a class="src" href="{BLOB}{path}{anchor}" title="Source: {e(path)}">{e(text)}</a>'


def line_of(rel: str, needle: str) -> int:
    """1-based line of the first occurrence of needle in a repository file (for source links)."""
    for number, text in enumerate(read(rel).splitlines(), 1):
        if needle in text:
            return number
    raise ValueError(f"{needle!r} not in {rel}")


def slugify(text: str) -> str:
    """GitHub-style heading slug: lower case, drop punctuation, spaces to hyphens."""
    text = re.sub(r"<[^>]+>", "", text)
    text = html.unescape(text).strip().lower()
    text = re.sub(r"[^\w\- ]", "", text)
    return text.replace(" ", "-")


def human_date(date: str) -> str:
    months = "January February March April May June July August September October November December".split()
    y, m, d = date[:10].split("-")
    return f"{int(d)} {months[int(m) - 1]} {y}"


def fmt(n: int) -> str:
    return f"{n:,}"


ICON = ('<svg viewBox="0 0 32 32" aria-hidden="true" focusable="false"><rect width="32" height="32" rx="8" fill="#14101f"/>'
        '<path d="M11.5 9H8.5v14h3M20.5 9h3v14h-3" fill="none" stroke="#fff" stroke-width="2.2" stroke-linecap="square"/>'
        '<circle cx="16" cy="16" r="2.6" fill="#8b5cf6"/></svg>')
