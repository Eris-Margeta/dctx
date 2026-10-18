#!/bin/sh

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
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
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
INSTALL_DIR="${DCTX_INSTALL_DIR:-/usr/local/bin}"
INSTALL_DEST="$INSTALL_DIR/dctx"
if [ -n "${DCTX_INSTALL_DIR:-}" ]; then
  mkdir -p "$INSTALL_DIR"
fi

if [ ! -f "$BINARY_SOURCE" ]; then
  echo "❌ Build failed. Binary not found at $BINARY_SOURCE"
  exit 1
fi

echo "📦 Installing binary to $INSTALL_DEST..."
if [ -w "$INSTALL_DIR" ]; then
  cp "$BINARY_SOURCE" "$INSTALL_DEST"
else
  echo "🔐 Sudo required to write to $INSTALL_DIR"
  sudo cp "$BINARY_SOURCE" "$INSTALL_DEST"
fi

# --- 4. Setup Configuration Directory ---
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/dctx"
TEMPLATES_DIR="$CONFIG_DIR/templates"
echo "📂 Setting up config directory: $CONFIG_DIR"
mkdir -p "$TEMPLATES_DIR"

# --- 5. Default Configuration ---
CONFIG_FILE="$CONFIG_DIR/config.toml"

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

EOF
fi

echo "dctx never edits ignore files; add any Git ignore patterns yourself if you want them."

# --- 8. Install Templates ---
# Never overwrite an existing template or a staged <name>.new file.
SOURCE_TEMPLATES="$PROJECT_ROOT/crates/dctx-core/templates"
if [ -d "$SOURCE_TEMPLATES" ]; then
  echo "🎨 Installing templates..."
  for source in "$SOURCE_TEMPLATES"/*; do
    [ -f "$source" ] || continue
    name=${source##*/}
    dest="$TEMPLATES_DIR/$name"
    staged="$dest.new"
    if [ ! -e "$dest" ]; then
      cp "$source" "$dest"
      echo "   - Installed template $name"
    elif [ -e "$staged" ]; then
      echo "   - Keeping existing template $dest; skipped staged update $staged"
    elif cmp -s "$source" "$dest"; then
      echo "   - Template $name already installed; left unchanged"
    else
      cp "$source" "$staged"
      echo "   - Keeping existing template $dest; wrote new default beside it as $staged"
    fi
  done
fi

# --- 9. Deploy Global Ignore/Omit Rules ---
# Deploy template files to config root where they're loaded at runtime.
# An existing rules file is kept. A staged *.new file is never overwritten.
GLOBAL_IGNORE_FILE="$CONFIG_DIR/ignore"
GLOBAL_OMIT_FILE="$CONFIG_DIR/omitcontent"

stage_rules() {
  _src=$1
  _dest=$2
  _label=$3
  if [ ! -f "$_dest" ]; then
    echo "📋 Deploying $_label..."
    cp "$_src" "$_dest"
    return 0
  fi
  _template_size=$(wc -c < "$_src")
  _current_size=$(wc -c < "$_dest")
  if [ "$_template_size" -gt "$_current_size" ]; then
    _staged="$_dest.new"
    if [ -e "$_staged" ]; then
      echo "Keeping existing staged update $_staged; skipped"
      return 0
    fi
    echo "🔄 Updating $_label (new version has more patterns)..."
    cp "$_src" "$_staged"
    echo "   - Your current rules: $_dest"
    echo "   - New comprehensive rules: $_staged"
    echo "   - Review and replace manually if desired"
  fi
}

if [ -f "$TEMPLATES_DIR/global-ignore" ]; then
  stage_rules "$TEMPLATES_DIR/global-ignore" "$GLOBAL_IGNORE_FILE" "global ignore rules"
fi

if [ -f "$TEMPLATES_DIR/global-omitcontent" ]; then
  stage_rules "$TEMPLATES_DIR/global-omitcontent" "$GLOBAL_OMIT_FILE" "global omit rules"
fi

echo ""
echo "✅ Installation Complete (v$DCTX_VERSION)!"
echo "   - Binary: $INSTALL_DEST"
echo "   - Config: $CONFIG_DIR"
echo "   - Global Ignore: $GLOBAL_IGNORE_FILE"
echo "   - Global Omit: $GLOBAL_OMIT_FILE"
