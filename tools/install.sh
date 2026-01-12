#!/bin/bash

# ==============================================================================
# DCTX INSTALLER
# ==============================================================================
# This script builds the Rust binary and installs it to the system.
# It also sets up the necessary configuration directories and default files.
# ==============================================================================

set -e # Exit immediately if a command exits with a non-zero status.

# --- 1. Detect Environment ---
OS="$(uname -s)"
ARCH="$(uname -m)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Read Version from file
VERSION_FILE="$PROJECT_ROOT/VERSION"
if [ -f "$VERSION_FILE" ]; then
  DCTX_VERSION=$(cat "$VERSION_FILE" | tr -d '[:space:]')
else
  echo "⚠️  VERSION file not found. Defaulting to 0.1.0"
  DCTX_VERSION="0.1.0"
fi

echo "🚀 Starting dctx installation v$DCTX_VERSION..."
echo "📍 Project Root: $PROJECT_ROOT"
echo "🖥️  System: $OS ($ARCH)"

# --- 2. Build Release Binary ---
echo "🛠️  Building dctx-cli (Release mode)..."
cd "$PROJECT_ROOT"
# Ensure we clean previous builds if version changed to force recompilation of env! constants
cargo build --release -p dctx-cli

# --- 3. Install Binary ---
BINARY_SOURCE="$PROJECT_ROOT/target/release/dctx-cli"
INSTALL_DEST="/usr/local/bin/dctx"

if [ ! -f "$BINARY_SOURCE" ]; then
  echo "❌ Build failed. Binary not found at $BINARY_SOURCE"
  exit 1
fi

echo "📦 Installing binary to $INSTALL_DEST..."
if [ -w "/usr/local/bin" ]; then
  cp "$BINARY_SOURCE" "$INSTALL_DEST"
else
  echo "🔐 Sudo required to write to /usr/local/bin"
  sudo cp "$BINARY_SOURCE" "$INSTALL_DEST"
fi

# --- 4. Setup Configuration Directory ---
CONFIG_DIR="$HOME/.config/dctx"
TEMPLATES_DIR="$CONFIG_DIR/templates"
echo "📂 Setting up config directory: $CONFIG_DIR"
mkdir -p "$TEMPLATES_DIR"

# --- 5. Config File Management (Merge-Append Strategy) ---
CONFIG_FILE="$CONFIG_DIR/config.toml"

# Define default configuration keys
CFG_KEY_GIT_SAFETY="update_local_git_repo_gitignore_rules"

if [ ! -f "$CONFIG_FILE" ]; then
  echo "📝 Creating default config.toml..."
  cat >"$CONFIG_FILE" <<EOF
# dctx Global Configuration
# Config Version: $DCTX_VERSION

# Output Mode: "both", "text", "binary"
output_mode = "both"

# Limit for diff files before forcing a full V-bump cleanup
max_diff_files = 999

# Default template to use if not specified
default_template = "default"

# Git Safety: Automatically add dctx patterns to local .gitignore
$CFG_KEY_GIT_SAFETY = true
EOF
else
  echo "🔄 Checking config.toml for missing keys..."

  # Check for Git Safety Key
  if ! grep -q "$CFG_KEY_GIT_SAFETY" "$CONFIG_FILE"; then
    echo "   -> Appending missing key: $CFG_KEY_GIT_SAFETY"
    cat >>"$CONFIG_FILE" <<EOF

# [Added in v$DCTX_VERSION]
# Git Safety: Automatically add dctx patterns to local .gitignore
$CFG_KEY_GIT_SAFETY = true
EOF
  fi

  # Update version comment (if we want to track it)
  # For now, we just append missing keys.
fi

# --- 6. Global Git Safety (Versioned Block Replacement) ---
echo "🌍 Configuring Global Git Safety..."

# Determine Global Ignore file path
GIT_GLOBAL_IGNORE=$(git config --global core.excludesfile || echo "")

if [ -z "$GIT_GLOBAL_IGNORE" ]; then
  if [ -d "$HOME/.config/git" ]; then
    GIT_GLOBAL_IGNORE="$HOME/.config/git/ignore"
  else
    GIT_GLOBAL_IGNORE="$HOME/.gitignore_global"
  fi
  echo "ℹ️  Git global ignore not explicitly set. Targeting: $GIT_GLOBAL_IGNORE"
fi

IGNORE_DIR=$(dirname "$GIT_GLOBAL_IGNORE")
mkdir -p "$IGNORE_DIR"
touch "$GIT_GLOBAL_IGNORE"

# Define Block Markers
BLOCK_START="# >>> dctx-ignore-v"
BLOCK_END="# <<< dctx-ignore-v"
NEW_BLOCK_START="# >>> dctx-ignore-v$DCTX_VERSION >>>"
NEW_BLOCK_END="# <<< dctx-ignore-v$DCTX_VERSION <<<"

# Define the content block
RULES_CONTENT=".dctx
.dctx/
*.llmcontext.txt
*.llmcontext-*-diff.txt"

# Check if CURRENT version block exists
if grep -Fq "$NEW_BLOCK_START" "$GIT_GLOBAL_IGNORE"; then
  echo "✅ Global gitignore is up to date (v$DCTX_VERSION)."
else
  echo "🔄 Updating global gitignore rules to v$DCTX_VERSION..."

  # Check if ANY dctx block exists (Old version)
  if grep -Fq "$BLOCK_START" "$GIT_GLOBAL_IGNORE"; then
    # Remove old block (using sed range delete)
    # We assume standard markers. This deletes anything between >>> dctx and <<< dctx
    if [[ "$OS" == "Darwin" ]]; then
      sed -i '' "/$BLOCK_START/,/$BLOCK_END/d" "$GIT_GLOBAL_IGNORE"
    else
      sed -i "/$BLOCK_START/,/$BLOCK_END/d" "$GIT_GLOBAL_IGNORE"
    fi
    echo "   -> Removed old dctx rules."
  fi

  # Append new block
  cat >>"$GIT_GLOBAL_IGNORE" <<EOF
$NEW_BLOCK_START
# Automated rules. Do not edit this block manually.
$RULES_CONTENT
$NEW_BLOCK_END
EOF
  echo "   -> Added rules for v$DCTX_VERSION."
fi

# --- 7. Local Repo Git Safety (Self-Correction) ---
# We update the .gitignore of the repo we are currently installing from
CURRENT_REPO_GITIGNORE="$PROJECT_ROOT/.gitignore"
if [ -f "$CURRENT_REPO_GITIGNORE" ]; then
  # Check for dctx marker
  if ! grep -q ".dctx/" "$CURRENT_REPO_GITIGNORE"; then
    echo "🛡️  Updating project root .gitignore..."
    echo "" >>"$CURRENT_REPO_GITIGNORE"
    echo "# [dctx] Installer Auto-Add" >>"$CURRENT_REPO_GITIGNORE"
    echo ".dctx/" >>"$CURRENT_REPO_GITIGNORE"
    echo "*.llmcontext.txt" >>"$CURRENT_REPO_GITIGNORE"
  fi
fi

# --- 8. Install Templates ---
SOURCE_TEMPLATES="$PROJECT_ROOT/crates/dctx-core/templates"
if [ -d "$SOURCE_TEMPLATES" ]; then
  echo "🎨 Installing templates..."
  cp -r "$SOURCE_TEMPLATES/"* "$TEMPLATES_DIR/"
fi

echo ""
echo "✅ Installation Complete (v$DCTX_VERSION)!"
echo "   - Binary: $INSTALL_DEST"
echo "   - Config: $CONFIG_DIR"
echo "   - Global Git Ignore: $GIT_GLOBAL_IGNORE"
