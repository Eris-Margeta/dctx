#!/bin/sh
# Usage: zsh tools/sync-version.sh [--check|--write]
# VERSION governs CLI/core, project metadata and the README badge only.
# Other crates, desktop versions and Cargo.lock have separate release steps.
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
for crate in ("dctx-core", "dctx-cli"):
    # Stay inside [package]; never rewrite dependency version requirements.
    patterns[f"crates/{crate}/Cargo.toml"] = (
        r'(?ms)(^\[package\]\n(?:(?!^\[).)*?^version = ")[^"]+("[ \t]*$)',
        rf'\g<1>{version}\2',
    )

# Validate every expected field before writing any file.
updates = []
for relative, (pattern, replacement) in patterns.items():
    path = root / relative
    before = path.read_text()
    after, count = re.subn(pattern, replacement, before)
    if count != 1:
        sys.exit(f"Expected exactly one version field in {relative}; found {count}")
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
print(f"CLI/core version fields match VERSION ({version}); Cargo.lock needs a separate locked check.")
PYTHON
