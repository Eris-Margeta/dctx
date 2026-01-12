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

echo "🚀 Starting dctx installation..."
echo "📍 Project Root: $PROJECT_ROOT"
echo "🖥️  System: $OS ($ARCH)"

# --- 2. Build Release Binary ---
echo "🛠️  Building dctx-cli (Release mode)..."
cd "$PROJECT_ROOT"
cargo build --release -p dctx-cli

# --- 3. Install Binary ---
BINARY_SOURCE="$PROJECT_ROOT/target/release/dctx-cli"
INSTALL_DEST="/usr/local/bin/dctx"

if [ ! -f "$BINARY_SOURCE" ]; then
  echo "❌ Build failed. Binary not found at $BINARY_SOURCE"
  exit 1
fi

echo "📦 Installing binary to $INSTALL_DEST..."
# Use sudo if we don't have write access to /usr/local/bin
if [ -w "/usr/local/bin" ]; then
  cp "$BINARY_SOURCE" "$INSTALL_DEST"
else
  echo "🔐 Sudo required to write to /usr/local/bin"
  sudo cp "$BINARY_SOURCE" "$INSTALL_DEST"
fi

# --- 4. Setup Configuration Directory ---
# Follows XDG standard on Linux, simple .config on macOS for consistency
CONFIG_DIR="$HOME/.config/dctx"
TEMPLATES_DIR="$CONFIG_DIR/templates"

echo "📂 Setting up config directory: $CONFIG_DIR"
mkdir -p "$TEMPLATES_DIR"

# --- 5. Install Default Config File ---
CONFIG_FILE="$CONFIG_DIR/config.toml"
if [ -f "$CONFIG_FILE" ]; then
  echo "ℹ️  Config file already exists. Skipping overwrite."
else
  echo "📝 Creating default config.toml..."
  cat >"$CONFIG_FILE" <<EOF
# dctx Global Configuration

# Output Mode: "both", "text", "binary"
output_mode = "both"

# Limit for diff files before forcing a full V-bump cleanup
max_diff_files = 999

# Default template to use if not specified
default_template = "default"

# Git Safety: Automatically add dctx patterns to local .gitignore
update_local_git_repo_gitignore_rules = true
EOF
fi

# --- 6. Install Global Ignore File ---
IGNORE_FILE="$CONFIG_DIR/ignore"
if [ -f "$IGNORE_FILE" ]; then
  echo "ℹ️  Global ignore file already exists. Skipping."
else
  echo "📝 Creating default global ignore file..."
  cat >"$IGNORE_FILE" <<EOF
# Global dctx ignore rules
.DS_Store
Thumbs.db
.git/
node_modules/
target/
.dctxignore
*.dctx
*.llmcontext.txt
EOF
fi

# --- 7. Install Templates ---
# We assume the templates are in crates/dctx-core/templates
SOURCE_TEMPLATES="$PROJECT_ROOT/crates/dctx-core/templates"
if [ -d "$SOURCE_TEMPLATES" ]; then
  echo "🎨 Installing templates..."
  cp -r "$SOURCE_TEMPLATES/"* "$TEMPLATES_DIR/"
else
  echo "⚠️  Warning: Template source directory not found at $SOURCE_TEMPLATES"
fi

# --- 8. GLOBAL GIT SAFETY (ADR-005) ---
echo "🌍 Configuring Global Git Safety..."

# Determine Global Ignore file path
GIT_GLOBAL_IGNORE=$(git config --global core.excludesfile || echo "")

# If not set, try default XDG path or MacOS default
if [ -z "$GIT_GLOBAL_IGNORE" ]; then
  if [ -d "$HOME/.config/git" ]; then
    GIT_GLOBAL_IGNORE="$HOME/.config/git/ignore"
  else
    GIT_GLOBAL_IGNORE="$HOME/.gitignore_global"
  fi
  echo "ℹ️  Git global ignore not explicitly set. Targeting: $GIT_GLOBAL_IGNORE"
fi

# Ensure directory exists if we are writing to ~/.config/git/
IGNORE_DIR=$(dirname "$GIT_GLOBAL_IGNORE")
mkdir -p "$IGNORE_DIR"

# Check if markers exist
if [ -f "$GIT_GLOBAL_IGNORE" ] && grep -q "# \[dctx\] Automated Ignore Rules" "$GIT_GLOBAL_IGNORE"; then
  echo "✅ Global gitignore already contains dctx rules. Skipping."
else
  echo "📝 Adding dctx rules to global gitignore ($GIT_GLOBAL_IGNORE)..."
  cat >>"$GIT_GLOBAL_IGNORE" <<EOF

# [dctx] Automated Ignore Rules - github.com/Eris-Margeta/dctx
.dctx
.dctx/
*.llmcontext.txt
*.llmcontext-*-diff.txt
EOF
  echo "✅ Updated global gitignore."
fi

echo ""
echo "✅ Installation Complete!"
echo "   - Binary: $INSTALL_DEST"
echo "   - Config: $CONFIG_DIR"
echo "   - Global Git Ignore: $GIT_GLOBAL_IGNORE"
echo "   - Run 'dctx' for interactive menu"
echo "   - Run 'dctx .' for instant snapshot"
