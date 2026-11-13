#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Offline dependency baseline for the dctx facade and shipped dctx-cli binary.

Python 3.11+; no Cargo, registry cache, subprocesses or network. Member manifests
classify normal/dev/build edges; normal edges include optional dependencies and
all targets, so this is a conservative release envelope, not one linked binary.
Cargo.lock does NOT classify external edges by kind or feature. Their complete
locked closure is retained: an exact external normal-only graph cannot be proved
from these inputs. Workspace dev edges are ignored; workspace build edges and
their closure are printed separately and excluded from the normal allowlist.

Allowlist format: NAME VERSION # one-line reason (blank/comment lines allowed).
Exit 0: equal baseline, 1: drift or invalid input, 2: command-line usage error.
An equal nonempty baseline does not mean the zero-dependency promise is met.
"""

import argparse
from collections import deque
from pathlib import Path
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent
RELEASE_ROOTS = ("dctx", "dctx-cli")


def read_toml(path):
    with path.open("rb") as stream:
        return tomllib.load(stream)


def package_id(package):
    return package["name"], package["version"], package.get("source", "")


def label(key):
    return f"{key[0]} {key[1]}"


def workspace_manifests(root):
    top = read_toml(root / "Cargo.toml")
    workspace = top.get("workspace", {})
    excluded = {p.resolve() for pattern in workspace.get("exclude", [])
                for p in root.glob(pattern)}
    directories = {p for pattern in workspace.get("members", [])
                   for p in root.glob(pattern) if p.resolve() not in excluded}
    if "package" in top:
        directories.add(root)
    manifests = {}
    for directory in sorted(directories):
        path = directory / "Cargo.toml"
        if not path.is_file():
            raise ValueError("workspace member has no Cargo.toml")
        manifest = read_toml(path)
        package = manifest["package"]
        version = package["version"]
        if isinstance(version, dict) and version.get("workspace"):
            version = workspace["package"]["version"]
        key = package["name"], version, ""
        if key in manifests:
            raise ValueError(f"duplicate workspace member: {label(key)}")
        manifests[key] = manifest
    return workspace, manifests


def declarations(manifest, workspace, kind):
    tables = [manifest, *manifest.get("target", {}).values()]
    for table in tables:
        for alias, spec in table.get(kind, {}).items():
            spec = {"version": spec} if isinstance(spec, str) else spec
            if spec.get("workspace"):
                inherited = workspace.get("dependencies", {}).get(alias)
                if inherited is None:
                    raise ValueError(f"missing workspace dependency: {alias}")
                spec = {"version": inherited} if isinstance(inherited, str) else inherited
            yield spec.get("package", alias), spec


def version_matches(version, requirement):
    """Common Cargo requirements; fail closed on unsupported ambiguous cases."""
    if not requirement or requirement == "*":
        return True
    if requirement.startswith("="):
        return version == requirement[1:].strip()
    match = re.fullmatch(r"([~^]?)(\d+)(?:\.(\d+|\*))?(?:\.(\d+|\*))?", requirement)
    if not match:
        raise ValueError(f"cannot disambiguate version requirement {requirement!r}")
    operator, *parts = match.groups()
    numbers = [int(p) for p in parts if p is not None and p != "*"]
    current = tuple(int(p) for p in version.split("+")[0].split("."))
    lower = tuple(numbers + [0] * (3 - len(numbers)))
    if "*" in parts:
        return current[:len(numbers)] == tuple(numbers)
    if operator == "~":
        pivot = min(len(numbers) - 1, 1)
    else:
        pivot = next((i for i, n in enumerate(numbers) if n), len(numbers) - 1)
    upper = list(lower)
    upper[pivot] += 1
    upper[pivot + 1:] = [0] * (2 - pivot)
    return lower <= current < tuple(upper)


def dependency_graph(root):
    workspace, manifests = workspace_manifests(root)
    packages = read_toml(root / "Cargo.lock")["package"]
    locked = {package_id(p): p for p in packages}
    if len(locked) != len(packages):
        raise ValueError("duplicate Cargo.lock package identity")
    for key in manifests:
        if key not in locked:
            raise ValueError(f"workspace member missing from Cargo.lock: {label(key)}")
    by_name = {}
    for key in locked:
        by_name.setdefault(key[0], []).append(key)

    def resolve(entry):
        match = re.fullmatch(r"([^ ]+)(?: ([^ ()]+))?(?: \((.+)\))?", entry)
        if not match:
            raise ValueError(f"invalid locked dependency: {entry!r}")
        name, version, source = match.groups()
        candidates = [k for k in by_name.get(name, [])
                      if (version is None or k[1] == version)
                      and (source is None or k[2] == source)]
        if len(candidates) != 1:
            raise ValueError(f"unresolved or ambiguous locked dependency: {entry!r}")
        return candidates[0]

    normal, build = {}, {}
    for key, package in locked.items():
        edges = sorted({resolve(entry) for entry in package.get("dependencies", [])})
        if key not in manifests:
            normal[key], build[key] = edges, []
            continue
        for kind, graph in (("dependencies", normal), ("build-dependencies", build)):
            graph[key] = []
            for name, spec in declarations(manifests[key], workspace, kind):
                candidates = [edge for edge in edges if edge[0] == name]
                if "path" in spec:
                    candidates = [edge for edge in candidates if not edge[2]]
                elif "git" in spec:
                    candidates = [edge for edge in candidates if edge[2].startswith("git+")]
                else:
                    candidates = [edge for edge in candidates if edge[2]]
                if len(candidates) > 1 or spec.get("version", "").startswith("="):
                    candidates = [edge for edge in candidates
                                  if version_matches(edge[1], spec.get("version"))]
                if len(candidates) != 1:
                    raise ValueError(f"missing or ambiguous {kind}: {key[0]} -> {name}")
                graph[key].extend(candidates)
    roots = []
    for name in RELEASE_ROOTS:
        matches = [key for key in manifests if key[0] == name]
        if len(matches) != 1:
            raise ValueError(f"expected one workspace release root: {name}")
        roots.extend(matches)
    return manifests, normal, build, roots


def walk(roots, edges):
    """Breadth-first provenance, with cycle protection and stable ordering."""
    paths = {key: (key[0],) for key in roots}
    pending = deque(roots)
    while pending:
        key = pending.popleft()
        for child in sorted(edges[key]):
            if child not in paths:
                paths[child] = (*paths[key], child[0])
                pending.append(child)
    return paths


def inventory(root):
    members, normal, build, roots = dependency_graph(root)
    paths = walk(roots, normal)
    build_roots = sorted({child for key in paths for child in build[key]})
    combined = {key: sorted(set(normal[key] + build[key])) for key in normal}
    build_paths = walk(build_roots, combined)
    external = {key: path for key, path in paths.items() if key not in members}
    build_external = {key: path for key, path in build_paths.items() if key not in members}
    labels = [label(key) for key in external]
    if len(set(labels)) != len(labels):
        raise ValueError("same name/version from multiple sources cannot share an allowlist entry")
    return external, build_external


def read_allowlist(path):
    allowed = set()
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        entry, marker, reason = line.partition("#")
        if len(entry.split()) != 2 or not marker or not reason.strip():
            raise ValueError(f"allowlist line {number}: expected NAME VERSION # reason")
        entry = " ".join(entry.split())
        if entry in allowed:
            raise ValueError(f"duplicate allowlist entry: {entry}")
        allowed.add(entry)
    return allowed


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT, help="workspace directory")
    parser.add_argument("--allowlist", type=Path, help="default: ROOT/scripts/zero_dep_allow.txt")
    args = parser.parse_args(argv)
    try:
        external, build = inventory(args.root)
        allowed = read_allowlist(args.allowlist or args.root / "scripts/zero_dep_allow.txt")
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"zero_dep: invalid input: {error}", file=sys.stderr)
        return 1
    print("Scope: dctx (facade), dctx-cli (release binary); all optional/target normal declarations.")
    print("Workspace dev/build edges excluded; external lock edges are untyped and retained conservatively.")
    print(f"Non-workspace normal-rooted lock closure: {len(external)} crates")
    for key, path in sorted(external.items()):
        print(f"  {label(key)} # {' -> '.join(path)}")
    print(f"Workspace build dependencies (separate; not allowlisted): {len(build)} crates")
    for key in sorted(build):
        print(f"  build: {label(key)}")
    if external:
        print("Zero-dependency promise: NOT MET; dependencies listed above remain.")
    else:
        print("Zero-dependency promise: no external crates in the normal-rooted lock closure.")
    actual = {label(key) for key in external}
    for entry in sorted(actual - allowed):
        print(f"EXTRA: {entry}")
    for entry in sorted(allowed - actual):
        print(f"STALE: {entry}")
    equal = actual == allowed
    print(f"Allowlist: {'PASS' if equal else 'FAIL'} ({len(actual)} actual, {len(allowed)} allowed)")
    return 0 if equal else 1


if __name__ == "__main__":
    sys.exit(main())
