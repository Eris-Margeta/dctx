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
SOURCE_TEMPLATES="$PROJECT_ROOT/crates/dctx-core/templates"
if [ -d "$SOURCE_TEMPLATES" ]; then
  echo "🎨 Installing templates..."
  cp -r "$SOURCE_TEMPLATES/"* "$TEMPLATES_DIR/"
fi

# --- 9. Deploy Global Ignore/Omit Rules ---
# Deploy template files to config root where they're loaded at runtime
GLOBAL_IGNORE_FILE="$CONFIG_DIR/ignore"
GLOBAL_OMIT_FILE="$CONFIG_DIR/omitcontent"

# Deploy global-ignore
if [ -f "$TEMPLATES_DIR/global-ignore" ]; then
  if [ ! -f "$GLOBAL_IGNORE_FILE" ]; then
    echo "📋 Deploying global ignore rules..."
    cp "$TEMPLATES_DIR/global-ignore" "$GLOBAL_IGNORE_FILE"
  else
    # Check if template is newer (larger file = more comprehensive rules)
    TEMPLATE_SIZE=$(wc -c < "$TEMPLATES_DIR/global-ignore")
    CURRENT_SIZE=$(wc -c < "$GLOBAL_IGNORE_FILE")
    if [ "$TEMPLATE_SIZE" -gt "$CURRENT_SIZE" ]; then
      echo "🔄 Updating global ignore rules (new version has more patterns)..."
      cp "$TEMPLATES_DIR/global-ignore" "$GLOBAL_IGNORE_FILE.new"
      echo "   - Your current rules: $GLOBAL_IGNORE_FILE"
      echo "   - New comprehensive rules: $GLOBAL_IGNORE_FILE.new"
      echo "   - Review and replace manually if desired"
    fi
  fi
fi

# Deploy global-omitcontent
if [ -f "$TEMPLATES_DIR/global-omitcontent" ]; then
  if [ ! -f "$GLOBAL_OMIT_FILE" ]; then
    echo "📋 Deploying global omit rules..."
    cp "$TEMPLATES_DIR/global-omitcontent" "$GLOBAL_OMIT_FILE"
  else
    # Check if template is newer
    TEMPLATE_SIZE=$(wc -c < "$TEMPLATES_DIR/global-omitcontent")
    CURRENT_SIZE=$(wc -c < "$GLOBAL_OMIT_FILE")
    if [ "$TEMPLATE_SIZE" -gt "$CURRENT_SIZE" ]; then
      echo "🔄 Updating global omit rules (new version has more patterns)..."
      cp "$TEMPLATES_DIR/global-omitcontent" "$GLOBAL_OMIT_FILE.new"
      echo "   - Your current rules: $GLOBAL_OMIT_FILE"
      echo "   - New comprehensive rules: $GLOBAL_OMIT_FILE.new"
      echo "   - Review and replace manually if desired"
    fi
  fi
fi

echo ""
echo "✅ Installation Complete (v$DCTX_VERSION)!"
echo "   - Binary: $INSTALL_DEST"
echo "   - Config: $CONFIG_DIR"
echo "   - Global Ignore: $GLOBAL_IGNORE_FILE"
echo "   - Global Omit: $GLOBAL_OMIT_FILE"
