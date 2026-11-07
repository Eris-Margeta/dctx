#!/bin/sh
# Usage: zsh tools/sync-version.sh [--check|--write]
# VERSION governs every crates/* and plugins/* package, their Cargo.lock entries,
# project metadata and the README badge. The desktop app keeps its own version line.
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
if [ "$#" -gt 1 ]; then
  printf '%s\n' "Usage: $0 [--check|--write]" >&2
  exit 2
fi
MODE="${1:---check}"
if [ "$MODE" != --check ] && [ "$MODE" != --write ]; then
  printf '%s\n' "Usage: $0 [--check|--write]" >&2
  exit 2
fi

python3 - "$SCRIPT_DIR/.." "$MODE" <<'PYTHON'
import re
import sys
import tomllib
from pathlib import Path

root = Path(sys.argv[1]).resolve()
mode = sys.argv[2]
version = (root / "VERSION").read_text().strip()
if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
    sys.exit("VERSION must contain a stable X.Y.Z release version")

patterns = {
    "project-meta.yaml": (r'(?m)^(version: ")[^"]+("[ \t]*)$', rf'\g<1>{version}\2'),
    "README.md": (r'(https://img.shields.io/badge/version-)\d+\.\d+\.\d+', rf'\g<1>{version}'),
}
lock = []
for manifest in sorted([*root.glob("crates/*/Cargo.toml"), *root.glob("plugins/*/Cargo.toml")]):
    # Stay inside [package]; never rewrite dependency version requirements.
    patterns[manifest.relative_to(root).as_posix()] = (
        r'(?ms)(^\[package\]\n(?:(?!^\[).)*?^version = ")[^"]+("[ \t]*$)',
        rf'\g<1>{version}\2',
    )
    name = re.search(r'(?ms)^\[package\]\n(?:(?!^\[).)*?^name = "([^"]+)"', manifest.read_text())
    if not name:
        sys.exit(f"No package name in {manifest.relative_to(root)}")
    lock.append(name.group(1))
# Workspace package names only; no registry crate shares a name with a workspace package.
lock_pattern = r'(?m)(^\[\[package\]\]\nname = "(?:' + "|".join(map(re.escape, lock)) + r')"\nversion = ")[^"]+(")'

# Registry requirements must follow the package versions for the next release.
# Versionless dev-only path dependencies are intentionally omitted by Cargo when
# packaging; adding registry versions there would create publication cycles.
manifests = sorted([*root.glob("crates/*/Cargo.toml"), *root.glob("plugins/*/Cargo.toml")])
local_dirs = {p.parent.resolve() for p in manifests}

# Validate every expected field before writing any file.
updates = []
lock_path = root / "Cargo.lock"
before = lock_path.read_text()
after, count = re.subn(lock_pattern, rf'\g<1>{version}\2', before)
if count != len(lock):
    sys.exit(f"Expected {len(lock)} workspace package entries in Cargo.lock; found {count}")
if before != after:
    updates.append((lock_path, "Cargo.lock", after))
for relative, (pattern, replacement) in patterns.items():
    path = root / relative
    before = path.read_text()
    after, count = re.subn(pattern, replacement, before)
    if count != 1:
        sys.exit(f"Expected exactly one version field in {relative}; found {count}")
    if path in manifests:
        section = ""
        lines = []
        for line in after.splitlines(keepends=True):
            if line.startswith("["):
                section = line.strip()
            if "dependencies]" in section and "dev-dependencies]" not in section and "path = " in line:
                dep = tomllib.loads(line)
                spec = next(iter(dep.values()))
                if (path.parent / spec["path"]).resolve() in local_dirs:
                    line = re.sub(r'version = "[^"]+"', f'version = "={version}"', line)
            lines.append(line)
        after = "".join(lines)
    if before != after:
        updates.append((path, relative, after))

for path, relative, after in updates:
    if mode == "--write":
        path.write_text(after)
        print(f"Updated {relative}")
    else:
        print(f"Version mismatch: {relative}", file=sys.stderr)
if mode == "--check" and updates:
    sys.exit(1)
print(f"{len(lock)} crate versions, their Cargo.lock entries and the metadata match VERSION ({version}).")
PYTHON
