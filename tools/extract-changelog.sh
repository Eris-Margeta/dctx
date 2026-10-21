#!/bin/sh
# Usage: zsh tools/extract-changelog.sh [version|Unreleased]
# With no argument, extract the latest numbered release (skip Unreleased).
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CHANGELOG_FILE="$SCRIPT_DIR/../CHANGELOG.md"
if [ ! -f "$CHANGELOG_FILE" ]; then
  printf '%s\n' "Error: root CHANGELOG.md not found" >&2
  exit 1
fi
if [ "$#" -gt 1 ]; then
  printf '%s\n' "Usage: $0 [version|Unreleased]" >&2
  exit 2
fi

VERSION="${1:-}"
VERSION="${VERSION#v}"
if [ -z "$VERSION" ]; then
  VERSION=$(awk '/^## \[[0-9]/ { sub(/^## \[/, ""); sub(/\].*$/, ""); print; exit }' "$CHANGELOG_FILE")
fi

# Compare literal bracket contents: dots in a version are not regex wildcards.
if ! awk -v ver="$VERSION" '
  /^## / {
    if (printing) exit
    header = $0
    sub(/^## \[/, "", header)
    sub(/\].*$/, "", header)
    if (header == ver) { printing = 1; found = 1; next }
  }
  printing { print }
  END { if (!found) exit 1 }
' "$CHANGELOG_FILE"; then
  printf '%s\n' "Error: requested version not found in CHANGELOG.md" >&2
  exit 1
fi
