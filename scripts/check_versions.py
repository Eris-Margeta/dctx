#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Read-only release version gate (Python 3.11+).

Versions are compared as exact strings, including prerelease/build suffixes.
The default root is this script's repository; --root selects a fixture tree.
Website selectors follow tools/website/common.py, build.py and sync-version.sh:
check their version fields, not arbitrary historical release mentions. Selectors
do not depend on the current VERSION value, so stale values remain detectable.
Exit codes: 0 agreement, 1 drift or unreadable/malformed sources, 2 usage.
"""

import argparse
import json
from pathlib import Path
import re
import tomllib


WEBSITE_JSON = (
    "tools/website/data/facts.json",
    "tools/website/data/demo.json",
    "tools/website/data/llms.json",
    "website/build.json",
)
WEBSITE_HTML = (
    ("softwareVersion", r'"softwareVersion"\s*:\s*"([^"\n]*)"'),
    ("brand", r'<span\b[^>]*\bclass=[\"\']ver[\"\'][^>]*>([^<]*)</span>'),
    ("footer", r'<p class="mono small">dctx ([^\s<]+)'),
)


def read_text(path):
    return path.read_text(encoding="utf-8")


def headings(text):
    """Yield level-two section labels, excluding fenced examples and links."""
    fence = None
    for line in text.splitlines():
        marker = re.match(r"^\s{0,3}(`{3,}|~{3,})", line)
        if marker:
            token = marker[1]
            if fence is None:
                fence = token
            elif token[0] == fence[0] and len(token) >= len(fence):
                fence = None
            continue
        if fence:
            continue
        match = re.match(r"^##[ \t]+(?:\[([^\]]+)\]|([^\s]+))", line)
        if match:
            yield match[1] or match[2]


def check(root, allow_unreleased=False):
    rows = []
    expected = None

    def add(source, value, accepted=False):
        valid = isinstance(value, str) and bool(value.strip())
        rows.append((source, value if valid else "<missing>",
                     valid and (accepted or (expected is not None and value == expected))))

    def error(source, exc):
        # Avoid absolute fixture paths in diagnostics; identify the relative source.
        rows.append((source, f"<unreadable or invalid: {type(exc).__name__}>", False))

    try:
        expected = read_text(root / "VERSION").strip() or None
        add("VERSION", expected)
    except (OSError, UnicodeError) as exc:
        error("VERSION", exc)

    try:
        sections = list(headings(read_text(root / "CHANGELOG.md")))
        if sections and sections[0].lower() == "unreleased":
            add("CHANGELOG.md:top", sections.pop(0), accepted=allow_unreleased)
        if sections:
            add("CHANGELOG.md:release", sections[0])
        elif not allow_unreleased or not rows or rows[-1][0] != "CHANGELOG.md:top":
            add("CHANGELOG.md:release", None)
    except (OSError, UnicodeError) as exc:
        error("CHANGELOG.md", exc)

    try:
        cargo = tomllib.loads(read_text(root / "Cargo.toml"))
        add("Cargo.toml:workspace.package.version",
            cargo.get("workspace", {}).get("package", {}).get("version"))
    except (OSError, UnicodeError, ValueError, AttributeError) as exc:
        error("Cargo.toml:workspace.package.version", exc)

    # This repository's published npm packages live in packaging/npm; also honor
    # the packages/* layout, including every manifest added there in the future.
    manifests = sorted({*root.glob("packages/*/package.json"),
                        *root.glob("packaging/npm/*/package.json")})

    def json_versions(path, fields):
        relative = path.relative_to(root).as_posix()
        try:
            data = json.loads(read_text(path))
            for field in fields:
                value = data.get(field)
                if field == "release_tag" and isinstance(value, str):
                    value = value.removeprefix("v")
                add(f"{relative}:{field}", value)
        except (OSError, UnicodeError, ValueError, AttributeError) as exc:
            error(relative, exc)

    for path in manifests:
        json_versions(path, ("version",))
    for relative in WEBSITE_JSON:
        path = root / relative
        if path.exists():
            fields = ("version", "release_tag") if relative.endswith("facts.json") else ("version",)
            json_versions(path, fields)

    for path in sorted((root / "website").rglob("*.html")):
        relative = path.relative_to(root).as_posix()
        try:
            text = read_text(path)
            for label, pattern in WEBSITE_HTML:
                for index, match in enumerate(re.finditer(pattern, text), 1):
                    add(f"{relative}:{label}[{index}]", match[1])
        except (OSError, UnicodeError) as exc:
            error(relative, exc)

    for path in sorted((root / "website").glob("llms*.txt")):
        relative = path.relative_to(root).as_posix()
        try:
            text = read_text(path)
            for match in re.finditer(r"(?m)^- (?:Current version: |Engine: dctx )([^\s,]+)", text):
                add(f"{relative}:{text.count(chr(10), 0, match.start()) + 1}", match[1].rstrip("."))
        except (OSError, UnicodeError) as exc:
            error(relative, exc)
    return rows


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1],
                        help="repository tree to check (default: this script's repository)")
    parser.add_argument("--allow-unreleased", action="store_true",
                        help="accept an Unreleased top section; still check the latest release if present")
    args = parser.parse_args(argv)
    rows = check(args.root, args.allow_unreleased)
    print("source\tvalue\tok")
    for source, value, ok in rows:
        print(f"{source}\t{json.dumps(value, ensure_ascii=False)}\t{'yes' if ok else 'NO'}")
    failures = [source for source, _, ok in rows if not ok]
    if failures:
        print("Disagreeing or invalid sources: " + ", ".join(failures))
    return int(bool(failures))


if __name__ == "__main__":
    raise SystemExit(main())
