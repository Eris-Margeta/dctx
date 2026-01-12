#!/bin/bash

# ==============================================================================
# DCTX BATTLE-TEST SUITE (ADR-007 COMPLIANT)
# ==============================================================================
# This script implements the Triple-Run Protocol to verify CLI stability.
# Usage: ./tools/test-integrity.sh
# ==============================================================================

set -e

# --- Colors ---
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

# --- Globals ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
TEST_ID="dctx-test-$(date +%s)"
TEST_ROOT="/tmp/$TEST_ID"
MOCK_HOME="$TEST_ROOT/home"
PROJECT_A="$TEST_ROOT/project-a"
LOG_FILE="$TEST_ROOT/dctx.log"
DCTX_BIN="$PROJECT_ROOT/target/release/dctx-cli"

# --- Helpers ---
log() { echo -e "${BLUE}[TEST]${NC} $1"; }
pass() { echo -e "${GREEN}[PASS]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
fail() {
  echo -e "${RED}[FAIL]${NC} $1"
  if [ -f "$LOG_FILE" ]; then
    echo ">>> LAST LOG OUTPUT <<<"
    tail -n 20 "$LOG_FILE"
    echo ">>> END LOG <<<"
  fi
  exit 1
}

run_dctx() {
  # Runs dctx and captures output to LOG_FILE
  # Usage: run_dctx [args...]
  echo "CMD: $DCTX_BIN $@" >>"$LOG_FILE"
  "$DCTX_BIN" "$@" >>"$LOG_FILE" 2>&1
  return $?
}

assert_file_exists() {
  if [ ! -f "$1" ]; then fail "File missing: $1"; fi
}

assert_file_missing() {
  if [ -f "$1" ]; then fail "File should NOT exist: $1"; fi
}

assert_version_in_file() {
  local file=$1
  local ver=$2
  if ! grep -q "SNAPSHOT_$ver]" "$file"; then
    fail "Version mismatch in $file. Expected $ver."
  fi
}

assert_log_contains() {
  local pattern=$1
  if ! grep -q "$pattern" "$LOG_FILE"; then
    fail "Log missing expected pattern: '$pattern'"
  fi
}

# ==============================================================================
# 1. BUILD PHASE
# ==============================================================================
log "Building dctx (Release Mode)..."
cd "$PROJECT_ROOT"
cargo build --release -p dctx-cli --quiet || fail "Build failed"

if [ ! -f "$DCTX_BIN" ]; then fail "Binary missing at $DCTX_BIN"; fi

# ==============================================================================
# 2. ENVIRONMENT SETUP
# ==============================================================================
log "Initializing Sandbox: $TEST_ROOT"
mkdir -p "$MOCK_HOME/.config/dctx"
mkdir -p "$MOCK_HOME/.cache/dctx"
mkdir -p "$PROJECT_A"

# Mock Environment
export XDG_CONFIG_HOME="$MOCK_HOME/.config"
export XDG_CACHE_HOME="$MOCK_HOME/.cache"
export HOME="$MOCK_HOME"

# Initial Project State
echo "def hello(): print('world')" >"$PROJECT_A/main.py"
echo "ignored_log" >"$PROJECT_A/debug.log"
echo "*.log" >"$PROJECT_A/.dctxignore"

# ==============================================================================
# TEST SUITE 1: FRESH START (V1)
# ==============================================================================
log ">>> SUITE 1: FRESH START (V1)"

# RUN 1: Trigger (Text Mode)
log "  Run 1: Trigger (Text Mode)"
run_dctx "$PROJECT_A" || fail "dctx crashed"
assert_file_exists "$PROJECT_A/.dctx/project-a.llmcontext.txt"
assert_version_in_file "$PROJECT_A/.dctx/project-a.llmcontext.txt" "V1"
assert_log_contains "No existing binary state found"

# RUN 2: Confirmation (Text Mode - No Changes)
log "  Run 2: Confirmation (No Changes)"
run_dctx "$PROJECT_A" || fail "dctx crashed"
assert_log_contains "No changes detected. Version remains V1"

# RUN 3: Cross-Check (Clipboard Mode)
log "  Run 3: Cross-Check (Clipboard Mode)"
run_dctx "$PROJECT_A" -c || fail "dctx crashed"
assert_log_contains "Copied to clipboard"
assert_log_contains "State Loaded: V1"

pass "Suite 1 Complete"

# ==============================================================================
# TEST SUITE 2: INCREMENTAL UPDATE (V1.1)
# ==============================================================================
log ">>> SUITE 2: INCREMENTAL UPDATE (V1 -> V1.1)"

# Modify state
echo "print('updated')" >>"$PROJECT_A/main.py"

# RUN 1: Trigger (Text Mode)
# Expectation: Version bump, but NO diff file (default config is false)
log "  Run 1: Trigger (Change Detected)"
run_dctx "$PROJECT_A" || fail "dctx crashed"
assert_version_in_file "$PROJECT_A/.dctx/project-a.llmcontext.txt" "V1.1"
assert_file_missing "$PROJECT_A/.dctx/project-a.llmcontext-V1.1-diff.txt"

# RUN 2: Confirmation (No Changes)
log "  Run 2: Confirmation"
run_dctx "$PROJECT_A" || fail "dctx crashed"
assert_log_contains "Version remains V1.1"

pass "Suite 2 Complete"

# ==============================================================================
# TEST SUITE 3: CONFIG PROPAGATION & DIFF GENERATION
# ==============================================================================
log ">>> SUITE 3: CONFIG ENABLE DIFFS & TREE MODE"

# Enable diff files in config
echo "text_output_mode_generates_diff_files = true" >"$XDG_CONFIG_HOME/dctx/config.toml"

# Modify state
sleep 1 # Ensure timestamp tick (though we fixed walker to use millis)
echo "New File" >"$PROJECT_A/README.md"

# RUN 1: Trigger (Tree-Only Mode)
# Expectation: V1.2, Diff file CREATED (due to config), Main file is Tree Only
log "  Run 1: Trigger (-t)"
run_dctx "$PROJECT_A" -t || fail "dctx crashed"

assert_version_in_file "$PROJECT_A/.dctx/project-a.llmcontext.txt" "V1.2"
assert_file_exists "$PROJECT_A/.dctx/project-a.llmcontext-V1.2-diff.txt"

# Verify Tree-Only content (No FILE_CONTENT_START)
if grep -q "FILE_CONTENT_START" "$PROJECT_A/.dctx/project-a.llmcontext.txt"; then
  fail "Tree-Only mode failed: Found content blocks"
fi

pass "Suite 3 Complete"

# ==============================================================================
# TEST SUITE 4: CLIPBOARD DIFF FORCING
# ==============================================================================
log ">>> SUITE 4: CLIPBOARD DIFF (-c -d)"

# Modify state
sleep 1
echo "Clipboard Change" >>"$PROJECT_A/README.md"

# RUN 1: Trigger (Clipboard + Diff)
# Expectation: Output should be the DIFF, not the full context.
# We can't easily check clipboard content from script, so we verify logs/logic flow
log "  Run 1: Trigger (-c -d)"
run_dctx "$PROJECT_A" -c -d || fail "dctx crashed"

# The log should show we detected changes and loaded state V1.2 -> V1.3
assert_log_contains "Version: V1.2 -> V1.3"
assert_log_contains "Copied to clipboard"

# RUN 2: Cross-Check (Text Mode)
# Since state was updated in clipboard mode, running text mode now should show NO changes
log "  Run 2: Confirmation (Text Mode)"
run_dctx "$PROJECT_A" || fail "dctx crashed"
assert_log_contains "No changes detected. Version remains V1.3"

pass "Suite 4 Complete"

# ==============================================================================
# TEST SUITE 5: MASSIVE CHANGE (V2 RESET)
# ==============================================================================
log ">>> SUITE 5: MASSIVE CHANGE (V2)"

# Nuke everything
rm "$PROJECT_A/"*
echo "Reborn" >"$PROJECT_A/genesis.txt"

# RUN 1: Trigger
log "  Run 1: Trigger (Massive Change)"
run_dctx "$PROJECT_A" || fail "dctx crashed"

assert_version_in_file "$PROJECT_A/.dctx/project-a.llmcontext.txt" "V2"
# Old diffs should be gone
assert_file_missing "$PROJECT_A/.dctx/project-a.llmcontext-V1.1-diff.txt"
assert_file_missing "$PROJECT_A/.dctx/project-a.llmcontext-V1.2-diff.txt"

pass "Suite 5 Complete"

log "========================================"
pass "ALL TRIPLE-RUN TESTS PASSED"
log "========================================"

# Cleanup
# rm -rf "$TEST_ROOT"
