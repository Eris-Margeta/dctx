#!/bin/bash

# ==============================================================================
# DCTX LOCAL WORKFLOW TEST SUITE
# ==============================================================================
# usage: ./tools/test-integrity.sh
# ==============================================================================

set -e

# --- Colors ---
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# --- Globals ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
# CHANGED: Local workspace instead of /tmp
TEST_ROOT="$SCRIPT_DIR/test_workspace"
MOCK_HOME="$TEST_ROOT/home"

# Generate 5-char random suffix for project uniqueness
RAND_SUFFIX=$(LC_ALL=C tr -dc 'a-zA-Z0-9' </dev/urandom | head -c 5)
PROJECT_A="$TEST_ROOT/project-a-$RAND_SUFFIX"

LOG_FILE="$TEST_ROOT/dctx.log"
DCTX_BIN="$PROJECT_ROOT/target/release/dctx-cli"

# --- Helpers ---
log() { echo -e "${BLUE}[TEST]${NC} $1"; }
pass() { echo -e "${GREEN}[PASS]${NC} $1"; }
fail() {
  echo -e "${RED}[FAIL]${NC} $1"
  echo "Check logs at: $LOG_FILE"
  exit 1
}

run_dctx() {
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
# 1. CLEANUP & BUILD
# ==============================================================================
log "Cleaning previous workspace..."
rm -rf "$TEST_ROOT"

log "Building dctx (Release Mode)..."
cd "$PROJECT_ROOT"
cargo build --release -p dctx-cli --quiet || fail "Build failed"

if [ ! -f "$DCTX_BIN" ]; then fail "Binary missing at $DCTX_BIN"; fi

# ==============================================================================
# 2. ENVIRONMENT SETUP
# ==============================================================================
log "Initializing Workspace at: $TEST_ROOT"
log "Target Project: $PROJECT_A"
mkdir -p "$MOCK_HOME/.config/dctx"
mkdir -p "$MOCK_HOME/.cache/dctx"
mkdir -p "$PROJECT_A"

# Mock Environment (Points dctx to our local folder)
export XDG_CONFIG_HOME="$MOCK_HOME/.config"
export XDG_CACHE_HOME="$MOCK_HOME/.cache"
export HOME="$MOCK_HOME"

# Initial Project State
echo "def hello(): print('world')" >"$PROJECT_A/main.py"
# ADDED: Stable file to prevent Massive Change trigger (V1->V2) on partial update
echo "I am constant" >"$PROJECT_A/stable.txt"
echo "ignored_log" >"$PROJECT_A/debug.log"
echo "*.log" >"$PROJECT_A/.dctxignore"

# Determine expected filenames based on random directory name
PROJECT_NAME="project-a-$RAND_SUFFIX"
MAIN_TXT="$PROJECT_A/.dctx/$PROJECT_NAME.llmcontext.txt"

# ==============================================================================
# TEST SUITE 1: FRESH START (V1)
# ==============================================================================
log ">>> SUITE 1: FRESH START (V1)"

# RUN 1: Trigger (Text Mode)
run_dctx "$PROJECT_A" || fail "dctx crashed"
assert_file_exists "$MAIN_TXT"
assert_version_in_file "$MAIN_TXT" "V1"
assert_log_contains "No existing binary state found"

# RUN 2: Confirmation (Text Mode - No Changes)
run_dctx "$PROJECT_A" || fail "dctx crashed"
assert_log_contains "No changes detected. Version remains V1"

# RUN 3: Cross-Check (Clipboard Mode)
run_dctx "$PROJECT_A" -c || fail "dctx crashed"
assert_log_contains "Copied to clipboard"
assert_log_contains "State Loaded: V1"

pass "Suite 1 Complete"

# ==============================================================================
# TEST SUITE 2: INCREMENTAL UPDATE (V1 -> V1.1)
# ==============================================================================
log ">>> SUITE 2: INCREMENTAL UPDATE (V1 -> V1.1)"

# Modify state (Only main.py changes, stable.txt remains)
sleep 2 # Ensure timestamp diff
echo "print('updated')" >>"$PROJECT_A/main.py"

# RUN 1: Trigger (Change Detected)
# Expectation: V1.1 because files_unchanged > 0
run_dctx "$PROJECT_A" || fail "dctx crashed"
assert_version_in_file "$MAIN_TXT" "V1.1"
assert_file_missing "$PROJECT_A/.dctx/$PROJECT_NAME.llmcontext-V1.1-diff.txt" # Default config=false

# RUN 2: Confirmation (No Changes)
run_dctx "$PROJECT_A" || fail "dctx crashed"
assert_log_contains "Version remains V1.1"

pass "Suite 2 Complete"

# ==============================================================================
# TEST SUITE 3: CONFIG PROPAGATION & DIFF GENERATION
# ==============================================================================
log ">>> SUITE 3: CONFIG ENABLE DIFFS & TREE MODE (V1.2)"

# Enable diff files in config
echo "text_output_mode_generates_diff_files = true" >"$XDG_CONFIG_HOME/dctx/config.toml"

# Modify state
sleep 2
echo "New File" >"$PROJECT_A/README.md"

# RUN 1: Trigger (-t Tree Only)
run_dctx "$PROJECT_A" -t || fail "dctx crashed"

assert_version_in_file "$MAIN_TXT" "V1.2"
assert_file_exists "$PROJECT_A/.dctx/$PROJECT_NAME.llmcontext-V1.2-diff.txt"

# Verify Tree-Only content (Use specific ID tag to avoid matching instructions)
if grep -q "FILE_CONTENT_START ID=\"F" "$MAIN_TXT"; then
  fail "Tree-Only mode failed: Found content blocks"
fi

pass "Suite 3 Complete"

# ==============================================================================
# TEST SUITE 4: CLIPBOARD DIFF FORCING (V1.3)
# ==============================================================================
log ">>> SUITE 4: CLIPBOARD DIFF (-c -d)"

# Modify state
sleep 2
echo "Clipboard Change" >>"$PROJECT_A/README.md"

# RUN 1: Trigger (Clipboard + Diff)
run_dctx "$PROJECT_A" -c -d || fail "dctx crashed"

# The log should show we detected changes and bumped version
assert_log_contains "Version: V1.2 -> V1.3"
assert_log_contains "Copied to clipboard"

# RUN 2: Cross-Check (Text Mode)
# Since state was updated in clipboard mode, running text mode now should show NO changes
run_dctx "$PROJECT_A" || fail "dctx crashed"
assert_log_contains "No changes detected. Version remains V1.3"

pass "Suite 4 Complete"

# ==============================================================================
# TEST SUITE 5: MASSIVE CHANGE (V2 RESET)
# ==============================================================================
log ">>> SUITE 5: MASSIVE CHANGE (V2)"

# Nuke everything (Including stable.txt) -> 0% Unchanged
rm "$PROJECT_A/"*
echo "Reborn" >"$PROJECT_A/genesis.txt"

# RUN 1: Trigger (Massive Change)
run_dctx "$PROJECT_A" || fail "dctx crashed"

assert_version_in_file "$MAIN_TXT" "V2"
# Old diffs should be gone
assert_file_missing "$PROJECT_A/.dctx/$PROJECT_NAME.llmcontext-V1.2-diff.txt"

pass "Suite 5 Complete"

# ==============================================================================
# TEST SUITE 6: PRIVACY VERIFICATION
# ==============================================================================
log ">>> SUITE 6: PRIVACY VERIFICATION"

# --- Test 6.1: Sensitive File in Ignored Folder Never Appears ---
log "Test 6.1: Sensitive file in ignored folder"

# Create .env file in .git/ folder
mkdir -p "$PROJECT_A/.git"
echo "AWS_SECRET_KEY=super-secret-123" > "$PROJECT_A/.git/.env"
echo ".git/" > "$PROJECT_A/.dctxignore"

run_dctx "$PROJECT_A" --tree || fail "dctx crashed"
CONTEXT_FILE="$PROJECT_A/.dctx/$PROJECT_NAME.llmcontext.txt"

# Assert .env never appears in output
if grep -q ".env" "$CONTEXT_FILE"; then
    fail "Test 6.1: .env file appears in output despite being ignored"
fi

# Assert secret never appears
if grep -q "AWS_SECRET_KEY" "$CONTEXT_FILE"; then
    fail "Test 6.1: Secret appears in output despite being ignored"
fi

pass "Test 6.1: Ignored sensitive file never appears"

# --- Test 6.2: Debug Mode Shows Paths But Not Content ---
log "Test 6.2: Debug mode shows [IGNORED] but no content"

# Create .env in root
echo "SECRET_DATA=classified" > "$PROJECT_A/.env"
echo ".env" >> "$PROJECT_A/.dctxignore"

run_dctx "$PROJECT_A" --debug-ignored --tree || fail "dctx crashed"

# Assert .env shown with [IGNORED] marker
if ! grep -q "\.env.*\[IGNORED\]" "$CONTEXT_FILE"; then
    fail "Test 6.2: .env not shown with [IGNORED] marker in debug mode"
fi

# Assert content never appears
if grep -q "SECRET_DATA" "$CONTEXT_FILE"; then
    fail "Test 6.2: Secret content appears in debug mode"
fi

pass "Test 6.2: Debug mode shows marker but no content"

# --- Test 6.3: OMIT vs IGNORE Distinction ---
log "Test 6.3: OMIT vs IGNORE distinction"

# Clean up .dctxignore and create omit test
rm "$PROJECT_A/.dctxignore"
rm "$PROJECT_A/.env"
rm -rf "$PROJECT_A/.git"

echo "lockfile content here" > "$PROJECT_A/package-lock.json"
echo "package-lock.json" > "$PROJECT_A/.dctxomitcontent"

run_dctx "$PROJECT_A" || fail "dctx crashed"

# Assert file appears in tree
if ! grep -q "package-lock.json" "$CONTEXT_FILE"; then
    fail "Test 6.3: Omitted file doesn't appear in tree"
fi

# Assert content is omitted
if ! grep -q "\[Content omitted by pattern rules\]" "$CONTEXT_FILE"; then
    fail "Test 6.3: Content not omitted for matched pattern"
fi

# Assert actual content doesn't appear
if grep -q "lockfile content here" "$CONTEXT_FILE"; then
    fail "Test 6.3: Omitted file content appears in output"
fi

pass "Test 6.3: OMIT shows in tree, content omitted"

# --- Test 6.4: Multiple Sensitive Files in Deep Hierarchy ---
log "Test 6.4: Deeply nested ignored files"

# Clean up omit test
rm "$PROJECT_A/.dctxomitcontent"
rm "$PROJECT_A/package-lock.json"

# Create deeply nested structure
mkdir -p "$PROJECT_A/.git/hooks"
mkdir -p "$PROJECT_A/secrets/prod"
echo "HOOK_SECRET=abc123" > "$PROJECT_A/.git/hooks/.env"
echo "PROD_KEY=xyz789" > "$PROJECT_A/secrets/prod/.env"

echo ".git/" > "$PROJECT_A/.dctxignore"
echo "secrets/" >> "$PROJECT_A/.dctxignore"

run_dctx "$PROJECT_A" --tree || fail "dctx crashed"

# Assert no secrets appear
if grep -q "HOOK_SECRET\|PROD_KEY" "$CONTEXT_FILE"; then
    fail "Test 6.4: Secrets from deeply nested ignored files appear"
fi

pass "Test 6.4: Deeply nested ignored files never read"

pass "Suite 6 Complete (4/4 privacy tests passed)"

log "========================================"
pass "ALL TESTS PASSED (6 Suites). Artifacts available in: $TEST_ROOT"
log "========================================"
