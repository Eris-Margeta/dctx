#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Fast, dependency-free wrapper import gate. See wrapper-api.toml for policy.

Rust use trees are expanded before matching (including aliases and glob imports).
Fully qualified references and Cargo dependency aliases are checked too. This is
a conservative source lint, not a substitute for Rust visibility or code review.
"""
import argparse
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE = {".rs", ".py", ".js", ".mjs", ".cjs", ".ts", ".tsx", ".go", ".java", ".cs", ".c", ".h", ".cpp"}
SKIP = {"target", "node_modules", "__pycache__", ".venv", "dist", ".git"}
RUST_REFERENCE = re.compile(r"\b(?:dctx(?:_[A-Za-z0-9_]+)?|crate|super)\s*::\s*(?:[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*(?:[A-Za-z_][A-Za-z0-9_]*|\*)")


def rust_code(source):
    # Keep offsets/line numbers; strip comments (including nested block comments),
    # strings and raw strings so documentation/examples cannot grant exemptions.
    pattern = re.compile(
        r'''//[^\n]*|/\*|r(#+)?"|'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'|"(?:\\.|[^"\\])*"''',
        re.S,
    )
    chars = list(source)
    pos = 0
    while match := pattern.search(source, pos):
        end = match.end()
        if match.group() == "/*":
            depth = 1
            while depth and end < len(source):
                token = re.search(r"/\*|\*/", source[end:])
                if token is None:
                    end = len(source)
                    break
                end += token.end()
                depth += 1 if token.group() == "/*" else -1
        elif match.group().startswith('r'):
            terminator = '"' + (match.group(1) or "")
            found = source.find(terminator, end)
            end = len(source) if found < 0 else found + len(terminator)
        for index in range(match.start(), end):
            if chars[index] != "\n":
                chars[index] = " "
        pos = end
    return "".join(chars)


def use_paths(tree, prefix=""):
    """Expand a use tree without losing the parent of grouped child imports."""
    depth = 0
    start = 0
    for index, char in enumerate(tree + ","):
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
        elif char == "," and depth == 0:
            item = tree[start:index].strip()
            start = index + 1
            if "{" in item:
                parent, children = item.split("{", 1)
                yield from use_paths(children.rsplit("}", 1)[0], prefix + parent.strip())
            elif item:
                item = re.split(r"\bas\b", item)[0].strip()
                yield prefix.rstrip(":") if item == "self" else prefix + item


def references(source):
    code = rust_code(source)
    code = re.sub(r"\br#(?=[A-Za-z_])", "", code)
    refs = []
    for match in re.finditer(r"\buse\s+([^;]+);", code):
        refs.extend((match.start(), path) for path in use_paths(re.sub(r"\s+", " ", match[1])))
    code = re.sub(r"\buse\s+[^;]+;", lambda m: re.sub(r"[^\n]", " ", m.group()), code)
    refs.extend((match.start(), match.group()) for match in RUST_REFERENCE.finditer(code))
    # Bare extern crate imports must not bypass fully-qualified path checks.
    refs.extend((m.start(), m[1]) for m in re.finditer(r"\bextern\s+crate\s+(\w+)", code))
    return [(pos, re.sub(r"\s", "", path).removeprefix("::")) for pos, path in refs]


def allowed(path, api):
    return any(path == entry or path.startswith(entry + "::") for entry in api)


def dependency_aliases(path):
    for parent in path.parents:
        manifest = parent / "Cargo.toml"
        if manifest.is_file():
            document = tomllib.loads(manifest.read_text())
            aliases = {}
            for table in [document, *document.get("target", {}).values()]:
                for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
                    for name, spec in table.get(kind, {}).items():
                        package = spec.get("package", name) if isinstance(spec, dict) else name
                        if package == "dctx" or package.startswith("dctx-"):
                            aliases[name.replace("-", "_")] = package.replace("-", "_")
            return aliases
    return {}


def check_file(path, relative, api):
    source = path.read_text(encoding="utf-8")
    errors = []
    if path.suffix == ".rs":
        aliases = dependency_aliases(path)
        code = rust_code(source)
        for alias, package in aliases.items():
            if alias != package:
                code = re.sub(rf"\b{re.escape(alias)}\b", package, code)
        for pos, ref in references(code):
            root = ref.split("::")[0]
            if (root == "crate" or root == "dctx" or root.startswith("dctx_")) and not allowed(ref, api):
                errors.append(f"{relative}:{source.count(chr(10), 0, pos) + 1}: forbidden engine reference {ref}")
            # Relative imports must not escape a wrapper into its crate's
            # composition modules. Inline unit tests may import their parent.
            production = re.split(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]", code, maxsplit=1)[0]
            if ref.startswith("super::super::") or (
                root == "super" and path.name == "mod.rs" and pos < len(production)
            ):
                errors.append(f"{relative}: parent import escapes wrapper boundary: {ref}; use an allowlisted crate path")
        if re.search(r'\binclude\s*!|#\s*\[\s*path\s*=', code):
            errors.append(f"{relative}: external source inclusion requires boundary review")
    elif path.name == "Cargo.toml":
        manifest = tomllib.loads(source)
        tables = [manifest, *manifest.get("target", {}).values()]
        for table in tables:
            for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
                for name, spec in table.get(kind, {}).items():
                    package = spec.get("package", name) if isinstance(spec, dict) else name
                    if package == "dctx" or package.startswith("dctx-"):
                        errors.append(f"{relative}: forbidden linked engine dependency {name} ({package})")
    else:
        # Other languages must use the binary protocol. Catch crate, module and
        # package spellings, including strings in require()/dynamic import().
        for match in re.finditer(r"\bdctx[-_](?:adapter|core|hash|lex|map|pack|ports|template|plugin)[A-Za-z0-9_/-]*", source):
            errors.append(f"{relative}:{source.count(chr(10), 0, match.start()) + 1}: forbidden engine import {match.group()}")
    return errors


def check(root, policy):
    errors = []
    count = 0
    if policy["version"] != 1:
        raise ValueError("unsupported allowlist version")
    for surface in policy["surface"]:
        paths = sorted({path for pattern in surface["paths"] for path in root.glob(pattern)})
        if surface["required"] and not paths:
            errors.append(f"{surface['name']}: required wrapper directory missing")
        for base in paths:
            for path in sorted(base.rglob("*")):
                relative = path.relative_to(root)
                if any(part in SKIP for part in relative.parts) or not path.is_file():
                    continue
                if path.suffix not in SOURCE and path.name != "Cargo.toml":
                    continue
                count += 1
                errors.extend(check_file(path, relative, surface["rust_api"]))
    return count, sorted(set(errors))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args()
    policy = tomllib.loads((ROOT / "tools/engine-contract/wrapper-api.toml").read_text())
    count, errors = check(args.root, policy)
    if errors:
        print("Wrapper boundary FAIL (allowlist: tools/engine-contract/wrapper-api.toml)", file=sys.stderr)
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"Wrapper boundary PASS: {count} files; only allowlisted engine APIs")
    return 0


if __name__ == "__main__":
    sys.exit(main())
