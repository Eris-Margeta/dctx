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

# --- 4.1. Create Global Ignore File ---
SOURCE_GLOBAL_IGNORE="$PROJECT_ROOT/crates/dctx-core/templates/global-ignore"
GLOBAL_IGNORE_FILE="$CONFIG_DIR/ignore"
if [ ! -f "$GLOBAL_IGNORE_FILE" ]; then
  if [ -f "$SOURCE_GLOBAL_IGNORE" ]; then
    echo "📝 Installing global ignore file from template..."
    cp "$SOURCE_GLOBAL_IGNORE" "$GLOBAL_IGNORE_FILE"
  else
    echo "⚠️  Template not found: $SOURCE_GLOBAL_IGNORE"
  fi
fi

# --- 4.2. Create Global Omit Content File ---
SOURCE_GLOBAL_OMIT="$PROJECT_ROOT/crates/dctx-core/templates/global-omitcontent"
GLOBAL_OMIT_FILE="$CONFIG_DIR/omitcontent"
if [ ! -f "$GLOBAL_OMIT_FILE" ]; then
  if [ -f "$SOURCE_GLOBAL_OMIT" ]; then
    echo "📝 Installing global omitcontent file from template..."
    cp "$SOURCE_GLOBAL_OMIT" "$GLOBAL_OMIT_FILE"
  else
    echo "⚠️  Template not found: $SOURCE_GLOBAL_OMIT"
  fi
fi

# --- 5. Config File Management (Merge-Append Strategy) ---
CONFIG_FILE="$CONFIG_DIR/config.toml"
CFG_KEY_GIT_SAFETY="update_local_git_repo_gitignore_rules"

if [ ! -f "$CONFIG_FILE" ]; then
  echo "📝 Creating default config.toml..."
  cat >"$CONFIG_FILE" <<EOF
# dctx Global Configuration
# Config Version: $DCTX_VERSION

# Text Output: Enable/disable writing .txt files to .dctx/ directory
# Binary state is ALWAYS written to system cache regardless of this setting.
text_output_enabled = true

# Limit for diff files before forcing a full V-bump cleanup
max_diff_files = 999

# Default template to use if not specified
default_template = "default"

# Git Safety: Automatically add dctx patterns to local .gitignore
$CFG_KEY_GIT_SAFETY = true
EOF
else
  echo "🔄 Checking config.toml for missing keys..."
  if ! grep -q "$CFG_KEY_GIT_SAFETY" "$CONFIG_FILE"; then
    echo "   -> Appending missing key: $CFG_KEY_GIT_SAFETY"
    cat >>"$CONFIG_FILE" <<EOF

# [Added in v$DCTX_VERSION]
# Git Safety: Automatically add dctx patterns to local .gitignore
$CFG_KEY_GIT_SAFETY = true
EOF
  fi
fi

# --- 6. Global Git Safety (Strict Fail-Safe & Recursion Fix) ---
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

# --- STEP 6.1: Cleanup Legacy Blocks (The Bug Fix) ---
# We use perl to match the specific legacy header and delete it + the next 4 lines.
# This cleans up the "litter" from previous versions.
if grep -q "# \[dctx\] Automated Ignore Rules" "$GIT_GLOBAL_IGNORE"; then
  echo "🧹 Cleaning up legacy unversioned dctx rules..."
  perl -i -ne 'if (/# \[dctx\] Automated Ignore Rules/) { $skip=5; } print unless $skip-- > 0;' "$GIT_GLOBAL_IGNORE"
fi

# --- STEP 6.2: Cleanup Any Versioned Blocks ---
# We remove ANY block matching the version pattern to get a clean state for checking.
# Regex: Matches start tag `>>> dctx-ignore`... up to end tag `<<< dctx-ignore`...
if grep -q ">>> dctx-ignore-v" "$GIT_GLOBAL_IGNORE"; then
  echo "🔄 removing existing dctx version block to re-verify..."
  perl -i -0777 -pe 's/# >>> dctx-ignore-v.*?# <<< dctx-ignore-v.*?\n//gs' "$GIT_GLOBAL_IGNORE"
fi

# --- STEP 6.3: Fail-Safe Check (Do we still need to add rules?) ---
# Now that our auto-blocks are gone, does the file STILL contain dctx rules?
# If yes, the user added them manually (e.g., **/.dctx/). We MUST NOT interfere.
if grep -q "\.dctx" "$GIT_GLOBAL_IGNORE" || grep -q "llmcontext\.txt" "$GIT_GLOBAL_IGNORE"; then
  echo "✅ Manual dctx rules detected in global gitignore. Skipping auto-add."
else
  # --- STEP 6.4: Add New Block (With Explicit Recursion) ---
  echo "📝 Adding dctx rules to global gitignore..."

  NEW_BLOCK_START="# >>> dctx-ignore-v$DCTX_VERSION >>>"
  NEW_BLOCK_END="# <<< dctx-ignore-v$DCTX_VERSION <<<"

  # We use double asterisks for explicit global recursion as requested
  RULES_CONTENT="**/.dctx
**/.dctx/
**/*.llmcontext.txt
**/*.llmcontext-*-diff.txt"

  cat >>"$GIT_GLOBAL_IGNORE" <<EOF
$NEW_BLOCK_START
# Automated rules. Do not edit this block manually.
$RULES_CONTENT
$NEW_BLOCK_END
EOF
  echo "✅ Updated global gitignore with explicit recursion rules."
fi

# --- 7. Local Repo Git Safety (Self-Correction) ---
CURRENT_REPO_GITIGNORE="$PROJECT_ROOT/.gitignore"
if [ -f "$CURRENT_REPO_GITIGNORE" ]; then
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
echo "   - Global Ignore: $GLOBAL_IGNORE_FILE"
echo "   - Global Omit: $GLOBAL_OMIT_FILE"
echo "   - Global Git Ignore: $GIT_GLOBAL_IGNORE"
