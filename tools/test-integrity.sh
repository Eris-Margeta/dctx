#!/bin/bash

# ==============================================================================
# DCTX INTEGRITY & PERSISTENCE TEST (UPDATED)
# ==============================================================================
# This script verifies the implementation of ADR-001 (Centralized State) and
# ADR-002 (Versioning). It runs in an isolated environment by overriding
# XDG_CONFIG_HOME and XDG_CACHE_HOME.
# ==============================================================================

set -e

# --- Colors ---
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log() { echo -e "${BLUE}[TEST]${NC} $1"; }
pass() { echo -e "${GREEN}[PASS]${NC} $1"; }
fail() {
  echo -e "${RED}[FAIL]${NC} $1"
  if [ -f "dctx.log" ]; then
    echo ">>> DUMPING LOGS <<<"
    cat dctx.log
  fi
  exit 1
}

# --- 1. Setup Paths ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
TEST_ENV_ROOT="/tmp/dctx-integrity-test-$(date +%s)"
MOCK_HOME="$TEST_ENV_ROOT/home"
TARGET_PROJECT="$TEST_ENV_ROOT/my-project"
DCTX_BIN="$PROJECT_ROOT/target/release/dctx-cli"
LOG_FILE="$TEST_ENV_ROOT/dctx.log"

# --- 2. Build Binary (Run with REAL environment) ---
log "Building release binary..."
cd "$PROJECT_ROOT"
# We DO NOT mock HOME yet, so cargo can find rustup
cargo build --release -p dctx-cli --quiet

if [ ! -f "$DCTX_BIN" ]; then
  fail "Binary not found at $DCTX_BIN"
fi

# --- 3. Prepare Mock Environment ---
log "Setting up mock environment at $TEST_ENV_ROOT"
mkdir -p "$MOCK_HOME"
mkdir -p "$TARGET_PROJECT"
# We create these so we can export them safely
mkdir -p "$MOCK_HOME/.config/dctx"
mkdir -p "$MOCK_HOME/.cache/dctx"

# Create some initial files
echo "Initial Content" >"$TARGET_PROJECT/main.txt"
echo "Ignore me" >"$TARGET_PROJECT/temp.log"

# Setup ignore rules
echo "*.log" >"$TARGET_PROJECT/.dctxignore"

# --- 4. Activate Isolation ---
# NOW we override the environment for the rest of the script (the actual testing)
export XDG_CONFIG_HOME="$MOCK_HOME/.config"
export XDG_CACHE_HOME="$MOCK_HOME/.cache"
export HOME="$MOCK_HOME"

log "Environment isolated. HOME is now $HOME"

# --- TEST CASE 1: Fresh Snapshot (V1) ---
log ">>> TEST CASE 1: Fresh Snapshot (V1)"
"$DCTX_BIN" "$TARGET_PROJECT" >"$LOG_FILE" 2>&1 || fail "dctx crashed"

# A. Check if .llmcontext.txt was created locally
if [ -f "$TARGET_PROJECT/.dctx/my-project.llmcontext.txt" ]; then
  pass "Local .llmcontext.txt created"
else
  fail "Local .llmcontext.txt missing"
fi

# B. Check Registry (MOVED TO CACHE)
REGISTRY_FILE="$XDG_CACHE_HOME/dctx/projects.db"
if [ -f "$REGISTRY_FILE" ]; then
  pass "Registry file created in cache"
else
  fail "Registry file missing at $REGISTRY_FILE"
fi

# C. Check Binary Cache
# Read hash from registry to find binary
HASH_ID=$(cat "$REGISTRY_FILE" | grep "$TARGET_PROJECT" | awk '{print $1}')
BINARY_FILE="$XDG_CACHE_HOME/dctx/$HASH_ID.dctx"

if [ -f "$BINARY_FILE" ]; then
  pass "Binary state file found in cache: $HASH_ID.dctx"
else
  fail "Binary state file missing from cache"
fi

# D. Check Version Content
if grep -q "SNAPSHOT_V1]" "$TARGET_PROJECT/.dctx/my-project.llmcontext.txt"; then
  pass "Version Header confirms V1"
else
  fail "Version Header mismatch. Expected V1."
fi

# --- TEST CASE 2: Incremental Update (V1.1) ---
log ">>> TEST CASE 2: Incremental Update (V1.1)"
sleep 1 # Ensure timestamp difference
echo "New Line" >>"$TARGET_PROJECT/main.txt"

"$DCTX_BIN" "$TARGET_PROJECT" >"$LOG_FILE" 2>&1 || fail "dctx crashed"

# A. Check Version
if grep -q "SNAPSHOT_V1.1]" "$TARGET_PROJECT/.dctx/my-project.llmcontext.txt"; then
  pass "Version bumped to V1.1"
else
  fail "Version bump failed."
fi

# B. Check Diff File Existence (Default config is text_output_mode_generates_diff_files = false)
# Wait! In Phase 5.2 we set default to FALSE. So diff file should NOT exist by default.
DIFF_FILE="$TARGET_PROJECT/.dctx/my-project.llmcontext-V1.1-diff.txt"
if [ ! -f "$DIFF_FILE" ]; then
  pass "Diff file NOT created (Default config behavior correct)"
else
  fail "Diff file created despite default config=false"
fi

# --- TEST CASE 3: Enable Diffs & Tree-Only Mode ---
log ">>> TEST CASE 3: Config Override & Tree-Only"

# Enable diffs in config
echo "text_output_mode_generates_diff_files = true" >"$XDG_CONFIG_HOME/dctx/config.toml"

sleep 1
echo "Another Change" >>"$TARGET_PROJECT/main.txt"

# Run with -t (Tree Only)
"$DCTX_BIN" "$TARGET_PROJECT" -t >"$LOG_FILE" 2>&1 || fail "dctx crashed"

# A. Check if Diff created now
DIFF_FILE_1_2="$TARGET_PROJECT/.dctx/my-project.llmcontext-V1.2-diff.txt"
if [ -f "$DIFF_FILE_1_2" ]; then
  pass "Diff file created after config update"
else
  fail "Diff file missing after config update"
fi

# B. Check Tree-Only Content (Should NOT contain FILE_CONTENT_START)
if ! grep -q "FILE_CONTENT_START" "$TARGET_PROJECT/.dctx/my-project.llmcontext.txt"; then
  pass "Tree-Only mode respected (No content blocks found)"
else
  fail "Tree-Only mode failed (Content blocks found)"
fi

# --- TEST CASE 4: Massive Change Trigger (V2) ---
log ">>> TEST CASE 4: Massive Change (V2 Reset)"
sleep 1
rm "$TARGET_PROJECT/main.txt"
echo "Complete Rewrite" >"$TARGET_PROJECT/new_start.txt"

"$DCTX_BIN" "$TARGET_PROJECT" >"$LOG_FILE" 2>&1 || fail "dctx crashed"

# A. Check Version (Should be V2, not V1.3)
if grep -q "SNAPSHOT_V2]" "$TARGET_PROJECT/.dctx/my-project.llmcontext.txt"; then
  pass "Massive change triggered Major Bump (V2)"
else
  fail "Major bump failed. Version state unclear."
fi

# B. Check Cleanup (Old diffs should be gone)
if [ ! -f "$DIFF_FILE_1_2" ]; then
  pass "Old diff files cleaned up"
else
  fail "Old diff files persist after Major Bump"
fi

log "========================================"
pass "ALL INTEGRITY TESTS PASSED"
log "========================================"

# Cleanup
rm -rf "$TEST_ENV_ROOT"
