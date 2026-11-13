# SPDX-License-Identifier: Apache-2.0
"""Check public status claims. Exit 0 clean, 1 violations, 2 invalid input.

Scan README.md, website/**/*.html / *.md / *.markdown, and docs/*.md.
Before 2.0, README and website/index.html must visibly say "not yet stable"
or "NOT STABLE", the two existing status forms. Quotes, Markdown blockquotes
and code, and HTML blockquote/q/code/pre are examples rather than claims.
HTML attributes are checked too; their syntax quotes do not exempt claims.
Only enterprise-ready has the explicit "not" exception.

Allowlist syntax (one exact exception per line):
relative/path:line:canonical phrase | nonempty reason
Use missing-status as the phrase for a required status notice exception.
Blank lines and # comments are ignored. No glob or whole-file exemptions.
"""

import argparse
from dataclasses import dataclass
from html import unescape
from html.parser import HTMLParser
from pathlib import Path, PurePosixPath
import re
import sys


PHRASES = (
    "production-ready", "enterprise-ready", "stable release", "battle-tested",
    "guaranteed savings", "saves tokens by default",
)
CLAIMS = [(phrase, re.compile(r"\b" + r"\s+".join(map(re.escape, phrase.split()))
                             + r"\b", re.I)) for phrase in PHRASES]
STATUS = re.compile(r"\bnot\s+(?:yet\s+)?stable\b", re.I)
QUOTES = re.compile(r'''"[^"\n]*"|(?<!\w)'[^'\n]*'(?!\w)|“[^”]*”|‘[^’]*’''')
ATTR = re.compile(r'''[^\s=/>]+\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))''')
IGNORED_TAGS = {"blockquote", "q", "code", "pre", "script", "style", "template"}


def blank(text):
    """Preserve newlines so diagnostics refer to the original file."""
    return re.sub(r"[^\n]", " ", text)


def markdown_examples(source):
    lines = source.splitlines(keepends=True)
    fence = None
    for index, line in enumerate(lines):
        marker = re.match(r" {0,3}(`{3,}|~{3,})", line)
        if fence:
            lines[index] = blank(line)
            if re.fullmatch(r" {0,3}" + re.escape(fence[0]) + "{" + str(len(fence))
                            + r",}\s*", line):
                fence = None
        elif marker:
            fence = marker[1]
            lines[index] = blank(line)
        elif re.match(r" {0,3}>", line):
            lines[index] = blank(line)
    source = "".join(lines)
    return re.sub(r"(`+)(?!`)([\s\S]*?)(?<!`)\1(?!`)",
                  lambda match: blank(match[0]), source)


class PublicText(HTMLParser):
    """Keep source positions while removing markup and quoted HTML contexts."""

    def __init__(self, source):
        super().__init__(convert_charrefs=False)
        self.source = source
        self.body = list(source)
        self.attributes = list(blank(source))
        self.starts = [0] + [match.end() for match in re.finditer("\n", source)]
        self.ignored = []
        self.feed(source)
        self.close()

    def source_offset(self):
        line, column = self.getpos()
        return self.starts[line - 1] + column

    def mask(self, start, end):
        self.body[start:end] = blank(self.source[start:end])

    def handle_starttag(self, tag, attrs):
        start = self.source_offset()
        raw = self.get_starttag_text()
        self.mask(start, start + len(raw))
        if not self.ignored and tag not in IGNORED_TAGS:
            for match in ATTR.finditer(raw):
                group = next(i for i in (1, 2, 3) if match[i] is not None)
                left, right = match.span(group)
                self.attributes[start + left:start + right] = raw[left:right]
        if tag in IGNORED_TAGS:
            self.ignored.append(tag)

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        if tag in IGNORED_TAGS:
            self.ignored.pop()

    def handle_endtag(self, tag):
        start = self.source_offset()
        end = self.source.find(">", start)
        self.mask(start, len(self.source) if end < 0 else end + 1)
        if tag in self.ignored:
            self.ignored = self.ignored[:len(self.ignored) - self.ignored[::-1].index(tag) - 1]

    def handle_data(self, data):
        if self.ignored:
            self.mask(self.source_offset(), self.source_offset() + len(data))

    def handle_comment(self, data):
        start = self.source_offset()
        end = self.source.find("-->", start)
        self.mask(start, len(self.source) if end < 0 else end + 3)

    def handle_decl(self, decl):
        self.mask(self.source_offset(), self.source_offset() + len(decl) + 3)


def public_text(source):
    parsed = PublicText(markdown_examples(source))
    body = QUOTES.sub(lambda match: blank(match[0]), "".join(parsed.body))
    # Emphasis can separate "not" from a negated claim in Markdown, just as
    # inline HTML tags can. Keep word-internal underscores intact.
    body = re.sub(r"(?<!\w)[*_]+|[*_]+(?!\w)", lambda match: blank(match[0]), body)
    # Decode entities after identifying quotation syntax. Entity decoding never
    # changes source line accounting (including numeric newline entities).
    def decode(text):
        return re.sub(r"&(?:#\d+|#x[\da-fA-F]+|\w+);",
                      lambda match: unescape(match[0]).replace("\n", " "), text)
    return decode(body), decode("".join(parsed.attributes))


@dataclass(frozen=True)
class Violation:
    path: str
    line: int
    phrase: str

    def __str__(self):
        detail = ("requires 'NOT STABLE' or 'not yet stable' before 2.0"
                  if self.phrase == "missing-status" else "forbidden claim: " + self.phrase)
        return f"{self.path}:{self.line}: {detail}"


def read_allowlist(path):
    allowed = set()
    for line, entry in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not entry.strip() or entry.lstrip().startswith("#"):
            continue
        match = re.fullmatch(r"([^:]+):([1-9]\d*):([^|]+)\|\s*(\S.*)", entry)
        if match:
            name, number, phrase, reason = match.groups()
            name, phrase = name.strip(), phrase.strip()
            relative = PurePosixPath(name)
            if (name == relative.as_posix() and not relative.is_absolute()
                    and ".." not in relative.parts and "\\" not in name
                    and not any(char in name for char in "*?[]")
                    and phrase in (*PHRASES, "missing-status")):
                allowed.add(Violation(name, int(number), phrase))
                continue
        raise ValueError(f"scripts/status_claims_allow.txt:{line}: invalid exception; "
                         "expected path:line:phrase | reason")
    return allowed


def check(root, allowlist=None):
    version = (root / "VERSION").read_text(encoding="utf-8").strip()
    match = re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
                         r"(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", version)
    if not match:
        raise ValueError("VERSION:1: expected a semantic version such as 1.3.1")
    allowed_path = allowlist if allowlist is not None else root / "scripts/status_claims_allow.txt"
    allowed = read_allowlist(allowed_path) if allowlist is not None or allowed_path.exists() else set()
    paths = {root / "README.md", root / "website/index.html"}
    paths.update((root / "docs").glob("*.md"))
    paths.update(path for path in (root / "website").rglob("*")
                 if path.suffix.lower() in {".html", ".htm", ".md", ".markdown"}
                 and path.is_file())
    violations = set()
    for path in sorted(paths):
        name = path.relative_to(root).as_posix()
        body, attributes = public_text(path.read_text(encoding="utf-8")) if path.exists() else ("", "")
        if (int(match[1]) < 2 and name in {"README.md", "website/index.html"}
                and not STATUS.search(body)):
            violations.add(Violation(name, 1, "missing-status"))
        for text in (body, attributes):
            for phrase, pattern in CLAIMS:
                for claim in pattern.finditer(text):
                    if phrase == "enterprise-ready" and re.search(r"\bnot\s+$", text[:claim.start()], re.I):
                        continue
                    violations.add(Violation(name, text.count("\n", 0, claim.start()) + 1, phrase))
    return sorted(violations - allowed, key=lambda item: (item.path, item.line, item.phrase))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1],
                        help="repository root (default: repository containing this script)")
    parser.add_argument("--allowlist", type=Path, help="override the repository allowlist")
    args = parser.parse_args(argv)
    try:
        violations = check(args.root, args.allowlist)
    except (OSError, UnicodeError, ValueError) as error:
        print(f"status claims: {error}", file=sys.stderr)
        return 2
    for violation in violations:
        print(violation)
    return int(bool(violations))


if __name__ == "__main__":
    sys.exit(main())
