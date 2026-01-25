#!/bin/bash

# ==============================================================================
# EXTRACT LATEST CHANGELOG ENTRY
# ==============================================================================
# Usage: ./tools/extract-changelog.sh [version]
#
# Extracts the changelog entry for the specified version (or latest if not specified)
# from PROJECT-INTERNAL/DOCUMENTS/CHANGELOG.md
# ==============================================================================

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
CHANGELOG_FILE="$PROJECT_ROOT/PROJECT-INTERNAL/DOCUMENTS/CHANGELOG.md"

if [ ! -f "$CHANGELOG_FILE" ]; then
  echo "❌ Error: CHANGELOG.md not found at $CHANGELOG_FILE"
  exit 1
fi

VERSION="$1"

if [ -z "$VERSION" ]; then
  # Extract latest version from CHANGELOG
  VERSION=$(grep -m 1 "^## \[" "$CHANGELOG_FILE" | sed 's/^## \[\(.*\)\].*/\1/')
fi

# Remove 'v' prefix if present
VERSION="${VERSION#v}"

# Extract the changelog section for this version
# Start from the version header, stop at the next version header or end of file
awk -v ver="$VERSION" '
  BEGIN { printing=0; found=0 }
  /^## \[/ {
    if (printing) exit
    if ($0 ~ "## \\[" ver "\\]") {
      printing=1
      found=1
      next
    }
  }
  printing { print }
  END { if (!found) exit 1 }
' "$CHANGELOG_FILE"

if [ $? -ne 0 ]; then
  echo "❌ Error: Version $VERSION not found in CHANGELOG.md" >&2
  exit 1
fi
