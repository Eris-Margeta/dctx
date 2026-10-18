#!/bin/bash

# ==============================================================================
# DCTX UNIT TEST RUNNER
# ==============================================================================
# Runs retained library compatibility tests and dctx-cli tests (D3-06).
# usage: ./tools/run-tests.sh [OPTIONS]
#
# Options:
#   --verbose, -v    Show full test output (not just summary)
#   --lib            Run only retained library tests
#   --bin            Run only binary tests (dctx-cli)
#   --module <name>  Run a retained integration target (e.g., selection_legacy)
#   --help, -h       Show this help message
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
VERBOSE=false
TARGET_LIB=true
TARGET_BIN=true
TARGET_MODULE=""

# --- Helpers ---
log() { echo -e "${BLUE}[TEST]${NC} $1"; }
pass() { echo -e "${GREEN}[PASS]${NC} $1"; }
fail() { echo -e "${RED}[FAIL]${NC} $1"; exit 1; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }

show_help() {
    head -n 15 "$0" | tail -n 13 | sed 's/^# //' | sed 's/^#//'
    exit 0
}

# --- Parse Arguments ---
while [[ $# -gt 0 ]]; do
    case $1 in
        --verbose|-v)
            VERBOSE=true
            shift
            ;;
        --lib)
            TARGET_LIB=true
            TARGET_BIN=false
            shift
            ;;
        --bin)
            TARGET_LIB=false
            TARGET_BIN=true
            shift
            ;;
        --module)
            TARGET_MODULE="$2"
            shift 2
            ;;
        --help|-h)
            show_help
            ;;
        *)
            warn "Unknown option: $1"
            shift
            ;;
    esac
done

# --- Main ---
cd "$PROJECT_ROOT"

# Preserve toolchain discovery while isolating every application home path.
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' EXIT
export HOME="$TEST_ROOT/home"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_CACHE_HOME="$HOME/.cache"
export XDG_DATA_HOME="$HOME/.local/share"
export XDG_STATE_HOME="$HOME/.local/state"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME"

echo ""
echo "========================================"
log "DCTX Unit Test Suite"
echo "========================================"
echo ""

# Build test args
CARGO_ARGS=""
if [ "$VERBOSE" = true ]; then
    CARGO_ARGS="-- --nocapture"
fi

# Track results
TESTS_PASSED=0
TESTS_FAILED=0

run_test_suite() {
    local crate=$1
    local description=$2
    local filter=$3
    local scope=${4:-}

    log "Running: $description"

    if [ -n "$filter" ]; then
        if cargo test -p "$crate" --test "$filter" $CARGO_ARGS 2>&1; then
            pass "$description"
            TESTS_PASSED=$((TESTS_PASSED + 1))
        else
            fail "$description"
            TESTS_FAILED=$((TESTS_FAILED + 1))
        fi
    else
        if cargo test -p "$crate" $scope $CARGO_ARGS 2>&1; then
            pass "$description"
            TESTS_PASSED=$((TESTS_PASSED + 1))
        else
            fail "$description"
            TESTS_FAILED=$((TESTS_FAILED + 1))
        fi
    fi
    echo ""
}

# Run tests based on options
if [ -n "$TARGET_MODULE" ]; then
    # Run specific module tests
    log "Target Module: $TARGET_MODULE"
    echo ""
    case "$TARGET_MODULE" in
        selection_legacy|selection_omit_legacy) crate=dctx-core ;;
        compatibility) crate=dctx-pack ;;
        legacy_envelopes) crate=dctx-template ;;
        *) fail "Unknown retained test target: $TARGET_MODULE" ;;
    esac
    run_test_suite "$crate" "$crate::$TARGET_MODULE" "$TARGET_MODULE"
else
    # Run all tests
    if [ "$TARGET_LIB" = true ]; then
        log "Testing retained library compatibility targets..."
        echo ""

        run_test_suite "dctx-core" "dctx-core library" "" "--lib"
        for module in selection_legacy selection_omit_legacy; do
            run_test_suite "dctx-core" "dctx-core::$module" "$module"
        done
        run_test_suite "dctx-pack" "dctx-pack::compatibility" "compatibility"
        run_test_suite "dctx-template" "dctx-template::legacy_envelopes" "legacy_envelopes"
    fi

    if [ "$TARGET_BIN" = true ]; then
        log "Testing dctx-cli binary..."
        echo ""
        run_test_suite "dctx-cli" "dctx-cli" ""
    fi
fi

# --- Summary ---
echo "========================================"
if [ $TESTS_FAILED -eq 0 ]; then
    pass "ALL TEST SUITES PASSED ($TESTS_PASSED suites)"
else
    fail "SOME TESTS FAILED ($TESTS_FAILED failed, $TESTS_PASSED passed)"
fi
echo "========================================"
echo ""
