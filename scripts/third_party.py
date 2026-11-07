#!/usr/bin/env python3
"""Write THIRD_PARTY.md: every non-workspace package in the dependency graph of the shipped binary (dctx-cli).

Modes:
  (default)    run `cargo metadata --format-version 1 --locked`, walk normal and build edges from dctx-cli, and fail
               (exit 1, file not written) on a package whose license is missing or not on the allow-list.
  --lock       best-effort list from Cargo.lock without cargo; licenses come from the local registry sources under
               $CARGO_HOME (default ~/.cargo) where present, else `see crate`. Fails only on a known, non-allowed license.
  --self-test  check the license-expression evaluator and the graph walk; prints `self-test ok`.
Options: --out PATH (default THIRD_PARTY.md at the repository root), --target TRIPLE (cargo metadata --filter-platform).

The listed set is a superset of what one build links: `cargo metadata` unifies features across all workspace members and,
without --target, keeps every platform's dependencies; Cargo.lock also keeps optional dependencies. Dev-dependencies of
workspace crates are excluded in both modes. Exit codes: 0 ok, 1 license problem, 2 usage or cargo error.
"""
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BINARY = "dctx-cli"
ALLOWED = ["Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSL-1.0", "MIT", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-3.0", "Unicode-DFS-2016", "Zlib", "MPL-2.0", "CC0-1.0"]
ALLOWED_FOLDED = {name.lower() for name in ALLOWED}
UNKNOWN = "see crate"


def tokens(expression):
    """Split an SPDX expression into parentheses, operators and identifiers; legacy `/` is read as OR."""
    return re.findall(r"\(|\)|[^\s()]+", expression.replace("/", " OR "))


def license_allowed(expression):
    """True when the SPDX expression can be satisfied with allow-listed licenses only.

    OR needs one allowed branch, AND needs every branch. `X WITH exception` counts as X: an exception only adds
    permissions. A trailing `+` (or later) counts as the base identifier. Empty or malformed expressions are not allowed.
    """
    if not expression or not expression.strip():
        return False
    items = tokens(expression)
    position = 0

    def peek():
        return items[position].upper() if position < len(items) else None

    def take():
        nonlocal position
        position += 1
        return items[position - 1]

    def atom():
        if peek() is None or peek() in ("AND", "OR", "WITH", ")"):
            raise ValueError("identifier expected")
        if peek() == "(":
            take()
            value = disjunction()
            if peek() != ")":
                raise ValueError("unbalanced parenthesis")
            take()
            return value
        identifier = take()
        if peek() == "WITH":
            take()
            if peek() is None or peek() in ("AND", "OR", "WITH", "(", ")"):
                raise ValueError("exception expected")
            take()
        return identifier.rstrip("+").lower() in ALLOWED_FOLDED

    def conjunction():
        value = atom()
        while peek() == "AND":
            take()
            value = atom() and value
        return value

    def disjunction():
        value = conjunction()
        while peek() == "OR":
            take()
            value = conjunction() or value
        return value

    try:
        value = disjunction()
    except ValueError:
        return False
    return value and position == len(items)


def walk(start, edges, workspace):
    """Ids reachable from `start` over `edges` (id -> [(dep id, kinds)]) through normal and build edges, minus `workspace`."""
    seen, stack = {start}, [start]
    while stack:
        for dep, kinds in edges.get(stack.pop(), []):
            if dep not in seen and any(kind in (None, "build") for kind in kinds):
                seen.add(dep)
                stack.append(dep)
    return seen - set(workspace)


def rows_from_metadata(metadata):
    """(name, version, license, repository) for every non-workspace package the shipped binary reaches in `metadata`."""
    packages = {package["id"]: package for package in metadata["packages"]}
    workspace = metadata["workspace_members"]
    start = [pid for pid in workspace if packages[pid]["name"] == BINARY]
    if len(start) != 1:
        raise SystemExit(f"third_party: workspace member {BINARY} not found in cargo metadata")
    edges = {node["id"]: [(dep["pkg"], [kind.get("kind") for kind in dep.get("dep_kinds", [])]) for dep in node.get("deps", [])] for node in metadata["resolve"]["nodes"]}
    rows = []
    for pid in walk(start[0], edges, workspace):
        package = packages[pid]
        rows.append((package["name"], package["version"], package.get("license") or "", package.get("repository") or ""))
    return rows


def manifest_dependencies(manifest, sections):
    """Package names declared in the given dependency `sections` of a manifest, including `[target.*]` tables."""
    tables = [manifest.get(section, {}) for section in sections]
    for target in manifest.get("target", {}).values():
        tables.extend(target.get(section, {}) for section in sections)
    return {(spec.get("package", key) if isinstance(spec, dict) else key) for table in tables for key, spec in table.items()}


def workspace_manifests(tomllib):
    """Package name -> parsed manifest for each workspace member of ROOT/Cargo.toml."""
    root = tomllib.loads((ROOT / "Cargo.toml").read_text())
    workspace = root.get("workspace", {})
    excluded = {(ROOT / path).resolve() for path in workspace.get("exclude", [])}
    manifests = {}
    for pattern in workspace.get("members", []):
        for directory in sorted(ROOT.glob(pattern)):
            path = directory / "Cargo.toml"
            if directory.resolve() not in excluded and path.is_file():
                manifest = tomllib.loads(path.read_text())
                manifests[manifest["package"]["name"]] = manifest
    return manifests


def registry_manifest(tomllib, name, version):
    """[package] of the crate's Cargo.toml in the local cargo registry sources, or {} when absent."""
    home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    for path in sorted(home.glob(f"registry/src/*/{name}-{version}/Cargo.toml")):
        try:
            return tomllib.loads(path.read_text()).get("package", {})
        except (OSError, ValueError):
            continue
    return {}


def rows_from_lock():
    """Best-effort rows from Cargo.lock: dev-only dependencies of workspace crates are skipped, licenses read locally."""
    import tomllib

    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
    ids = {(package["name"], package["version"]): package for package in lock}
    by_name = {}
    for package in lock:
        by_name.setdefault(package["name"], []).append(package)
    workspace = {(package["name"], package["version"]) for package in lock if "source" not in package}
    manifests = workspace_manifests(tomllib)
    dev_only = {name: manifest_dependencies(manifest, ["dev-dependencies"]) - manifest_dependencies(manifest, ["dependencies", "build-dependencies"]) for name, manifest in manifests.items()}
    edges = {}
    for package in lock:
        key = (package["name"], package["version"])
        edges[key] = []
        for entry in package.get("dependencies", []):
            parts = entry.split(" ")
            matches = [ids[(parts[0], parts[1])]] if len(parts) > 1 else by_name.get(parts[0], [])
            if len(matches) != 1:
                raise SystemExit(f"third_party: cannot resolve Cargo.lock dependency {entry!r} of {package['name']}")
            kinds = ["dev"] if key in workspace and parts[0] in dev_only.get(package["name"], set()) else [None]
            edges[key].append(((matches[0]["name"], matches[0]["version"]), kinds))
    start = [key for key in workspace if key[0] == BINARY]
    if len(start) != 1:
        raise SystemExit(f"third_party: workspace member {BINARY} not found in Cargo.lock")
    rows = []
    for name, version in walk(start[0], edges, workspace):
        package = registry_manifest(tomllib, name, version)
        rows.append((name, version, package.get("license") or UNKNOWN, package.get("repository") or UNKNOWN))
    return rows


def problems(rows, unknown_ok):
    """One message per row whose license is missing or not allowed; `see crate` passes when `unknown_ok`."""
    found = []
    for name, version, license_expression, _ in rows:
        if unknown_ok and license_expression == UNKNOWN:
            continue
        if not license_expression:
            found.append(f"{name} {version}: no license expression (license-file only or unset)")
        elif not license_allowed(license_expression):
            found.append(f"{name} {version}: license {license_expression!r} is not on the allow-list")
    return found


def render(rows, source):
    """THIRD_PARTY.md text for `rows`, sorted by name then version, with the source and allow-list stated."""
    cell = lambda text: (text or "-").replace("|", "\\|")
    lines = [
        "# Third-party dependencies",
        "",
        f"Packages outside this workspace in the dependency graph of the shipped binary `{BINARY}` (default features).",
        f"Generated by `scripts/third_party.py` from {source}; do not edit by hand.",
        "The list is a superset of one build: it may include platform-specific or optional crates that a given build does not link.",
        "Each package keeps its own license and notices; this file is an index, not the license texts.",
        "",
        "Allowed licenses (and OR/AND combinations of them): " + ", ".join(ALLOWED) + ".",
        "",
        f"{len(rows)} packages.",
        "",
        "| Crate | Version | License | Repository |",
        "|---|---|---|---|",
    ]
    for name, version, license_expression, repository in sorted(rows, key=lambda row: (row[0], row[1])):
        lines.append(f"| {cell(name)} | {cell(version)} | {cell(license_expression)} | {cell(repository)} |")
    return "\n".join(lines) + "\n"


def self_test():
    """Asserts the evaluator and the walk on fixed cases; returns 0."""
    allowed = ["MIT", "Apache-2.0", "BSL-1.0", "Apache-2.0 WITH LLVM-exception", "MIT OR Apache-2.0", "MIT/Apache-2.0", "(MIT OR Apache-2.0) AND Unicode-3.0", "Unicode-DFS-2016",
               "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT", "Unlicense OR MIT", "Zlib OR Apache-2.0 OR MIT", "BSD-2-Clause",
               "BSD-3-Clause", "ISC", "MPL-2.0", "CC0-1.0", "mit or apache-2.0", "MIT AND (GPL-3.0 OR ISC)", "Apache-2.0+"]
    denied = ["", "   ", None, "GPL-3.0", "Unlicense", "MIT AND GPL-3.0", "ISC AND MIT AND OpenSSL", "(MIT", "MIT)", "MIT OR",
              "OR MIT", "MIT Apache-2.0", "GPL-2.0 WITH Classpath-exception-2.0", "MIT WITH", "AND"]
    for expression in allowed:
        assert license_allowed(expression), f"should be allowed: {expression!r}"
    for expression in denied:
        assert not license_allowed(expression), f"should be denied: {expression!r}"
    edges = {"cli": [("a", [None]), ("b", ["dev"]), ("c", ["build"]), ("w", [None])], "a": [("d", [None, "dev"])], "b": [("e", [None])], "w": [("f", [None])]}
    assert walk("cli", edges, ["cli", "w"]) == {"a", "c", "d", "f"}, walk("cli", edges, ["cli", "w"])
    metadata = {
        "workspace_members": ["cli 1", "w 1"],
        "packages": [{"id": "cli 1", "name": BINARY, "version": "1"}, {"id": "w 1", "name": "w", "version": "1"},
                     {"id": "a 1", "name": "a", "version": "1", "license": "MIT", "repository": "https://example.invalid/a"},
                     {"id": "t 1", "name": "t", "version": "1", "license": None}, {"id": "g 1", "name": "g", "version": "1", "license": "GPL-3.0"}],
        "resolve": {"nodes": [{"id": "cli 1", "deps": [{"pkg": "w 1", "dep_kinds": [{"kind": None}]}, {"pkg": "t 1", "dep_kinds": [{"kind": "dev"}]}]},
                              {"id": "w 1", "deps": [{"pkg": "a 1", "dep_kinds": [{"kind": None, "target": None}]}, {"pkg": "g 1", "dep_kinds": [{"kind": "build"}]}]}]},
    }
    rows = rows_from_metadata(metadata)
    assert sorted(row[0] for row in rows) == ["a", "g"], rows
    assert problems(rows, False) == ["g 1: license 'GPL-3.0' is not on the allow-list"], problems(rows, False)
    assert problems([("t", "1", "", "")], False) and problems([("u", "1", UNKNOWN, "")], True) == []
    text = render(rows, "a fixture")
    assert "| Crate | Version | License | Repository |" in text and "| a | 1 | MIT | https://example.invalid/a |" in text and "2 packages." in text, text
    print("self-test ok")
    return 0


def main(argv):
    """Parses arguments, builds the rows, checks licenses and writes the file; returns the exit code."""
    out, target, mode, rest = ROOT / "THIRD_PARTY.md", None, "metadata", list(argv)
    while rest:
        flag = rest.pop(0)
        if flag in ("--lock", "--self-test"):
            mode = flag[2:]
        elif flag in ("--out", "--target") and rest:
            value = rest.pop(0)
            out, target = (Path(value), target) if flag == "--out" else (out, value)
        else:
            print(__doc__, file=sys.stderr)
            return 2
    if mode == "self-test":
        return self_test()
    if mode == "lock":
        rows, source = rows_from_lock(), "`Cargo.lock` (best effort: licenses from local registry sources, `see crate` where unknown)"
    else:
        command = ["cargo", "metadata", "--format-version", "1", "--locked", "--manifest-path", str(ROOT / "Cargo.toml")]
        if target:
            command += ["--filter-platform", target]
        try:
            result = subprocess.run(command, check=True, capture_output=True, text=True)
        except (OSError, subprocess.CalledProcessError) as error:
            print(f"third_party: {' '.join(command)} failed: {getattr(error, 'stderr', '') or error}", file=sys.stderr)
            return 2
        rows, source = rows_from_metadata(json.loads(result.stdout)), "`cargo metadata --locked`" + (f" for {target}" if target else " (all platforms)")
    found = problems(rows, mode == "lock")
    if found:
        print("third_party FAIL:\n  " + "\n  ".join(found), file=sys.stderr)
        return 1
    out.write_text(render(rows, source))
    print(f"third_party ok: {len(rows)} packages written to {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
