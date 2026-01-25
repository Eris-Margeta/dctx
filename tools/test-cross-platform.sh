#!/bin/bash
# Cross-platform verification script for dctx
# Tests installation, paths, and core functionality

set -e

echo "=================================="
echo "dctx Cross-Platform Test Suite"
echo "=================================="
echo ""

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Track results
PASSED=0
FAILED=0

# Test function
run_test() {
    local test_name="$1"
    local test_command="$2"

    echo -n "Testing: $test_name ... "
    if eval "$test_command" > /dev/null 2>&1; then
        echo -e "${GREEN}OK${NC}"
        PASSED=$((PASSED + 1))
        return 0
    else
        echo -e "${RED}FAILED${NC}"
        FAILED=$((FAILED + 1))
        return 1
    fi
}

# Display system information
echo -e "${BLUE}System Information:${NC}"
echo "  OS: $(uname -s)"
echo "  Kernel: $(uname -r)"
echo "  Architecture: $(uname -m)"
echo "  Rust: $(rustc --version)"
echo "  Cargo: $(cargo --version)"
echo ""

# Build and install
echo -e "${BLUE}Step 1: Building dctx${NC}"
cargo build --release
# Create dctx symlink for easier testing
if [ ! -f target/release/dctx ] && [ -f target/release/dctx-cli ]; then
    ln -sf dctx-cli target/release/dctx
fi
echo ""

echo -e "${BLUE}Step 2: Path Verification${NC}"
run_test "Home directory exists" "test -d $HOME"
run_test "Can create cache directory" "mkdir -p $HOME/.cache/dctx && test -d $HOME/.cache/dctx"
run_test "Can create config directory" "mkdir -p $HOME/.config/dctx && test -d $HOME/.config/dctx"
run_test "Can write to cache directory" "touch $HOME/.cache/dctx/test.tmp && rm $HOME/.cache/dctx/test.tmp"
run_test "Can write to config directory" "touch $HOME/.config/dctx/test.tmp && rm $HOME/.config/dctx/test.tmp"
echo ""

echo -e "${BLUE}Step 3: Binary Execution${NC}"
run_test "Binary exists" "test -f target/release/dctx"
run_test "Binary is executable" "test -x target/release/dctx"
run_test "Binary runs (help)" "./target/release/dctx --help"
echo ""

echo -e "${BLUE}Step 4: Functional Tests${NC}"
# Create a test directory
mkdir -p /tmp/dctx-test
cd /tmp/dctx-test
echo "test content" > test.txt
mkdir -p subdir
echo "nested content" > subdir/nested.txt

run_test "Generate context from directory" "$OLDPWD/target/release/dctx ."
run_test "Binary state file created" "test -f $HOME/.cache/dctx/*.dctx"
run_test "Can run dctx twice (state loading)" "$OLDPWD/target/release/dctx ."
run_test "Tree-only mode works" "$OLDPWD/target/release/dctx -t ."
run_test "Reset flag works" "$OLDPWD/target/release/dctx -r ."

# Cleanup test directory
cd "$OLDPWD"
rm -rf /tmp/dctx-test
echo ""

echo -e "${BLUE}Step 5: File Classification${NC}"
# Create test files
mkdir -p /tmp/dctx-binary-test
cd /tmp/dctx-binary-test
echo "text file" > test.txt
# Create a fake binary file (with null bytes)
printf '\x00\x01\x02\x03' > test.bin
touch test.png

run_test "Processes directory with binary files" "$OLDPWD/target/release/dctx ."

# Cleanup
cd "$OLDPWD"
rm -rf /tmp/dctx-binary-test
echo ""

# Summary
echo "=================================="
echo -e "${BLUE}Test Summary:${NC}"
echo -e "  ${GREEN}Passed: $PASSED${NC}"
if [ $FAILED -gt 0 ]; then
    echo -e "  ${RED}Failed: $FAILED${NC}"
else
    echo -e "  Failed: $FAILED"
fi
echo "=================================="

if [ $FAILED -gt 0 ]; then
    exit 1
fi

exit 0
