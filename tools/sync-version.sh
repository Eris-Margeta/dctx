#!/bin/bash

# ==============================================================================
# DCTX VERSION SYNCHRONIZER
# ==============================================================================
# Usage: ./tools/sync-version.sh
#
# This script reads the version from the root 'VERSION' file and updates:
# 1. project-meta.yaml
# 2. crates/dctx-core/Cargo.toml
# 3. crates/dctx-cli/Cargo.toml
# 4. README.md (version badges and references)
#
# It ensures all components are strictly aligned to the Unified Version.
# ==============================================================================

set -e

# --- 1. Setup Environment ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

VERSION_FILE="$PROJECT_ROOT/VERSION"
META_FILE="$PROJECT_ROOT/project-meta.yaml"
CORE_TOML="$PROJECT_ROOT/crates/dctx-core/Cargo.toml"
CLI_TOML="$PROJECT_ROOT/crates/dctx-cli/Cargo.toml"
README_FILE="$PROJECT_ROOT/README.md"

# --- 2. Read Source of Truth ---
if [ ! -f "$VERSION_FILE" ]; then
  echo "❌ Error: VERSION file not found at $VERSION_FILE"
  exit 1
fi

NEW_VERSION=$(cat "$VERSION_FILE" | tr -d '[:space:]')
echo "🚀 Syncing version to: $NEW_VERSION"

# --- 3. Helper Function for Cross-Platform Sed ---
# macOS sed requires an empty string argument for -i, Linux does not.
run_sed() {
  local pattern=$1
  local file=$2

  if [[ "$(uname)" == "Darwin" ]]; then
    sed -i '' "$pattern" "$file"
  else
    sed -i "$pattern" "$file"
  fi
}

# --- 4. Update project-meta.yaml ---
# Looks for: version: "X.Y.Z"
echo "📄 Updating project-meta.yaml..."
run_sed "s/^version: \".*\"/version: \"$NEW_VERSION\"/" "$META_FILE"

# --- 5. Update Cargo.toml files ---
# Looks for: version = "X.Y.Z" (only the first occurrence in [package] section usually)
# We match specifically the line `version = "..."` at the start of the line to avoid dependency versions.

echo "📦 Updating dctx-core/Cargo.toml..."
run_sed "s/^version = \".*\"/version = \"$NEW_VERSION\"/" "$CORE_TOML"

echo "📦 Updating dctx-cli/Cargo.toml..."
run_sed "s/^version = \".*\"/version = \"$NEW_VERSION\"/" "$CLI_TOML"

# --- 6. Update README.md Version Badge ---
# The badge URL format: https://img.shields.io/badge/version-X.Y.Z-8b5cf6
echo "📖 Updating README.md version references..."
if [ -f "$README_FILE" ]; then
  # Update version badge if it exists
  run_sed "s|https://img.shields.io/badge/version-[^-]*-|https://img.shields.io/badge/version-$NEW_VERSION-|g" "$README_FILE"
  # Note: GitHub release badge auto-updates from releases, no change needed
fi

# --- 7. Verification ---
echo ""
echo "✅ Version Sync Complete!"
echo "   - Source: $NEW_VERSION"
echo "   - Updated: project-meta.yaml, Cargo.toml files, README.md"
echo "   - Please verify changes with 'git diff' before committing."
