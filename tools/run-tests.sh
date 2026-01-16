#!/bin/bash

# ==============================================================================
# DCTX UNIT TEST RUNNER
# ==============================================================================
# Runs all unit tests for dctx-core and dctx-cli crates.
# usage: ./tools/run-tests.sh [OPTIONS]
#
# Options:
#   --verbose, -v    Show full test output (not just summary)
#   --lib            Run only library tests (dctx-core)
#   --bin            Run only binary tests (dctx-cli)
#   --module <name>  Run tests for a specific module (e.g., config, diff, reader)
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

    log "Running: $description"

    if [ -n "$filter" ]; then
        if cargo test -p "$crate" "$filter" $CARGO_ARGS 2>&1; then
            pass "$description"
            ((TESTS_PASSED++))
        else
            fail "$description"
            ((TESTS_FAILED++))
        fi
    else
        if cargo test -p "$crate" $CARGO_ARGS 2>&1; then
            pass "$description"
            ((TESTS_PASSED++))
        else
            fail "$description"
            ((TESTS_FAILED++))
        fi
    fi
    echo ""
}

# Run tests based on options
if [ -n "$TARGET_MODULE" ]; then
    # Run specific module tests
    log "Target Module: $TARGET_MODULE"
    echo ""
    run_test_suite "dctx-core" "dctx-core::$TARGET_MODULE" "$TARGET_MODULE::"
else
    # Run all tests
    if [ "$TARGET_LIB" = true ]; then
        log "Testing dctx-core library modules..."
        echo ""

        # Test each module individually for better reporting
        MODULES=(
            "config"
            "datatypes"
            "ignore"
            "diff"
            "version"
            "registry"
            "template"
            "writer"
            "reader"
            "llm_formatter"
        )

        for module in "${MODULES[@]}"; do
            run_test_suite "dctx-core" "dctx-core::$module" "$module::"
        done
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
