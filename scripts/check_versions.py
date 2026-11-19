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


def check(root, allow_unreleased=False, preflight=False):
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
        if preflight:
            release = next((section for section in sections if section.lower() != "unreleased"), None)
            add("CHANGELOG.md:release", expected if expected in sections else release)
        else:
            unreleased = bool(sections and sections[0].lower() == "unreleased")
            if unreleased:
                add("CHANGELOG.md:top", sections.pop(0), accepted=allow_unreleased)
            if sections or not (allow_unreleased and unreleased):
                add("CHANGELOG.md:release", sections[0] if sections else None)
    except (OSError, UnicodeError) as exc:
        error("CHANGELOG.md", exc)

    try:
        cargo = tomllib.loads(read_text(root / "Cargo.toml"))
        workspace = cargo.get("workspace", {}).get("package", {})
        crates = sorted(root.glob("crates/*/Cargo.toml")) if preflight else []
        if "version" in workspace or not preflight or (not crates and "package" not in cargo):
            add("Cargo.toml:workspace.package.version", workspace.get("version"))
        if preflight and "package" in cargo:
            crates.insert(0, root / "Cargo.toml")
        for path in crates:
            source = f"{path.relative_to(root).as_posix()}:package.version"
            try:
                manifest = cargo if path == root / "Cargo.toml" else tomllib.loads(read_text(path))
                version = manifest.get("package", {}).get("version")
                add(source, workspace.get("version") if version == {"workspace": True} else version)
            except (OSError, UnicodeError, ValueError, AttributeError) as exc:
                error(source, exc)
    except (OSError, UnicodeError, ValueError, AttributeError) as exc:
        error("Cargo.toml:workspace.package.version", exc)

    # This repository's published npm packages live in packaging/npm; also honor
    # the packages/* layout, including every manifest added there in the future.
    manifests = sorted({*root.glob("packages/**/package.json"),
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
    for path in sorted(root.glob("packages/**/pyproject.toml")):
        source = f"{path.relative_to(root).as_posix()}:project.version"
        try:
            add(source, tomllib.loads(read_text(path)).get("project", {}).get("version"))
        except (OSError, UnicodeError, ValueError, AttributeError) as exc:
            error(source, exc)

    if preflight:
        return rows

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


# These selectors read the scalar declarations used by the repository's workflows.
# Keep the preflight dependency-free and never source a workflow or shell file.
ACTION_USE = re.compile(r"""^\s*(?:-\s*)?uses:\s*["']?([^\s"'#]+)""")
TOOLCHAIN_FIELD = re.compile(r"""^\s*(?:-\s*)?(?:toolchain|RUSTUP_TOOLCHAIN|DCTX_CI_RUST):\s*["']?([^\s"'#]+)""")
TOOLCHAIN_INSTALL = re.compile(r"""\brustup\s+toolchain\s+install\s+["']?([^\s"'#]+)""")
CHANNEL = re.compile(r"(?:stable|beta|nightly(?:-\d{4}-\d{2}-\d{2})?|\d+\.\d+(?:\.\d+)?)")
SHA = re.compile(r"[a-fA-F0-9]{40}")


def workflow_checks(root):
    toolchains = []
    actions = []
    read_errors = []
    workflows = sorted((root / ".github/workflows").glob("*.y*ml"))
    for path in workflows:
        try:
            lines = read_text(path).splitlines()
        except (OSError, UnicodeError) as exc:
            read_errors.append(f"{path.name}: {type(exc).__name__}")
            continue
        for number, line in enumerate(lines, 1):
            source = f".github/workflows/{path.name}:{number}"
            action = ACTION_USE.match(line)
            if action and not action[1].startswith(("./", "docker://")):
                actions.append((source, action[1]))
            # The matrix includes an MSRV lane, and release and benchmark jobs
            # deliberately install their own toolchains.
            if path.name == "performance.yml" or line.lstrip().startswith("#"):
                continue
            field = TOOLCHAIN_FIELD.match(line)
            install = TOOLCHAIN_INSTALL.search(line)
            match = field or install
            if match and (CHANNEL.fullmatch(match[1]) or match[1] in ("$DCTX_CI_RUST", "${DCTX_CI_RUST}")):
                toolchains.append((source, match[1]))
            if action and action[1].startswith("dtolnay/rust-toolchain@"):
                ref = action[1].split("@", 1)[1]
                if CHANNEL.fullmatch(ref):
                    toolchains.append((source, ref))

    errors = read_errors.copy()
    try:
        channel = tomllib.loads(read_text(root / "rust-toolchain.toml")).get("toolchain", {}).get("channel")
        if not isinstance(channel, str) or not channel.strip():
            errors.append("rust-toolchain.toml: missing toolchain.channel")
    except (OSError, UnicodeError, ValueError, AttributeError) as exc:
        channel = None
        errors.append(f"rust-toolchain.toml: {type(exc).__name__}")
    if not toolchains:
        errors.append("no default Rust toolchain found in .github/workflows")
    ci_pin = None
    if any(expected.startswith("$") for _, expected in toolchains):
        try:
            pin = re.search(r"""(?m)^DCTX_CI_RUST=["']?([^\s"'#]+)""",
                            read_text(root / "tools/release/toolchain.env"))
            if pin:
                ci_pin = pin[1]
        except (OSError, UnicodeError):
            pass
    for source, expected in toolchains:
        if expected.startswith("$"):
            expected = ci_pin
        if expected is None or (channel and channel != expected):
            errors.append(f"{source}: expected {expected or 'a DCTX_CI_RUST pin'}, found {channel!r}")
    yield "rust-toolchain", errors

    errors = read_errors.copy()
    if any(SHA.fullmatch(action.rpartition("@")[2]) for _, action in actions):
        errors.extend(f"{source}: {action} needs a full commit SHA"
                      for source, action in actions if not SHA.fullmatch(action.rpartition("@")[2]))
    yield "action-pins", errors


def run_preflight(root):
    rows = check(root, preflight=True)
    version = rows[0][1]
    checks = [
        ("versions", [f"{source}={value!r} (VERSION={version!r})" for source, value, ok in rows
                      if not ok and not source.startswith("CHANGELOG.md")]),
        ("changelog", [f"{source}={value!r} (VERSION={version!r})" for source, value, ok in rows
                       if not ok and source.startswith("CHANGELOG.md")]),
        *workflow_checks(root),
    ]
    for name, errors in checks:
        print(f"FAIL {name}: " + "; ".join(errors) if errors else f"PASS {name}")
    return int(any(errors for _, errors in checks))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1],
                        help="repository tree to check (default: this script's repository)")
    parser.add_argument("--allow-unreleased", action="store_true",
                        help="accept an Unreleased top section; still check the latest release if present")
    parser.add_argument("--preflight", action="store_true", help="run the offline release preflight")
    args = parser.parse_args(argv)
    if args.preflight:
        return run_preflight(args.root)
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
