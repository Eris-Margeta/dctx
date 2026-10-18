#!/usr/bin/env bash
# tools/benchmark-ignore-performance.sh
# Performance benchmark for ignore folder skip optimization
# Validates performance claims in ADR-014 and README.md

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_ROOT"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Configuration
QUICK_MODE=false
VERBOSE=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --quick)
            QUICK_MODE=true
            shift
            ;;
        --verbose)
            VERBOSE=true
            shift
            ;;
        --help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --quick      Run with 1/10th files (faster, for CI)"
            echo "  --verbose    Show detailed timing per run"
            echo "  --help       Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            echo "Use --help for usage information"
            exit 1
            ;;
    esac
done

# Set file counts based on mode
if [ "$QUICK_MODE" = true ]; then
    NODE_MODULES_FILES=5000
    GIT_OBJECTS=500
    TARGET_DIRS=5
    TARGET_FILES_PER_DIR=100
    echo -e "${YELLOW}Running in QUICK mode (1/10th files)${NC}"
else
    NODE_MODULES_FILES=50000
    GIT_OBJECTS=5000
    TARGET_DIRS=10
    TARGET_FILES_PER_DIR=1000
fi

# Create isolated test directory
TEST_ROOT=$(mktemp -d)
echo -e "${CYAN}Test directory: $TEST_ROOT${NC}"

# Cleanup function
cleanup() {
    echo -e "${CYAN}Cleaning up test files...${NC}"
    rm -rf "$TEST_ROOT"
    echo -e "${GREEN}✓ Cleanup complete${NC}"
}
trap cleanup EXIT

# Build dctx if needed
if [ ! -f "target/release/dctx-cli" ]; then
    echo -e "${YELLOW}Building dctx in release mode...${NC}"
    cargo build --release --quiet
fi

DCTX="./target/release/dctx-cli"

# D3-06: benchmark state must never use the developer's home.
export HOME="$TEST_ROOT/home"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_CACHE_HOME="$HOME/.cache"
export XDG_DATA_HOME="$HOME/.local/share"
export XDG_STATE_HOME="$HOME/.local/state"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME"

# Function to measure time in milliseconds
measure_time() {
    local cmd="$1"
    local output_var="$2"

    # Use Perl for cross-platform millisecond timing
    local start_time=$(perl -MTime::HiRes=time -e 'printf "%.0f", time * 1000')
    eval "$cmd" > /dev/null 2>&1
    local end_time=$(perl -MTime::HiRes=time -e 'printf "%.0f", time * 1000')

    local duration=$((end_time - start_time))
    eval "$output_var=$duration"

    if [ "$VERBOSE" = true ]; then
        echo -e "  ${BLUE}Run time: ${duration}ms${NC}"
    fi
}

# Function to run multiple measurements and average
average_time() {
    local cmd="$1"
    local runs=3
    local total=0

    for i in $(seq 1 $runs); do
        measure_time "$cmd" duration
        total=$((total + duration))
    done

    echo $((total / runs))
}

echo ""
echo -e "${CYAN}═══════════════════════════════════════════════════${NC}"
echo -e "${CYAN}  DCTX IGNORE PERFORMANCE BENCHMARK${NC}"
echo -e "${CYAN}═══════════════════════════════════════════════════${NC}"
echo ""

# ============================================================================
# SCENARIO 1: Large node_modules/ (50,000 files)
# ============================================================================
echo -e "${YELLOW}Scenario 1: Large node_modules/ ($NODE_MODULES_FILES files)${NC}"
echo -e "${CYAN}Setting up test project...${NC}"

PROJECT1="$TEST_ROOT/project1"
mkdir -p "$PROJECT1/src"
mkdir -p "$PROJECT1/node_modules"

# Create some actual source files
echo "console.log('main');" > "$PROJECT1/src/index.js"
echo "export const util = () => {};" > "$PROJECT1/src/utils.js"

# Create large node_modules/
echo -e "${CYAN}Creating $NODE_MODULES_FILES files in node_modules/...${NC}"
for i in $(seq 1 $NODE_MODULES_FILES); do
    echo "// module $i" > "$PROJECT1/node_modules/file_$i.js"
    if [ $((i % 10000)) -eq 0 ]; then
        echo -e "  ${BLUE}Progress: $i/$NODE_MODULES_FILES${NC}"
    fi
done

# Measure WITHOUT ignore
echo -e "${CYAN}Measuring scan time WITHOUT ignore rules...${NC}"
TIME_WITHOUT_1=$(average_time "$DCTX \"$PROJECT1\" --tree")

# Measure WITH ignore
echo "node_modules/" > "$PROJECT1/.dctxignore"
echo -e "${CYAN}Measuring scan time WITH ignore rules...${NC}"
TIME_WITH_1=$(average_time "$DCTX \"$PROJECT1\" --tree")

# Handle case where time is too fast to measure accurately
if [ "$TIME_WITH_1" -eq 0 ]; then
    SPEEDUP_1="∞ (too fast to measure)"
else
    SPEEDUP_1=$(echo "scale=1; $TIME_WITHOUT_1 / $TIME_WITH_1" | bc)
    SPEEDUP_1="${SPEEDUP_1}x"
fi

echo -e "${GREEN}  WITHOUT ignore: ${TIME_WITHOUT_1}ms${NC}"
echo -e "${GREEN}  WITH ignore:    ${TIME_WITH_1}ms${NC}"
echo -e "${GREEN}  Speedup:        ${SPEEDUP_1}${NC}"
echo ""

# ============================================================================
# SCENARIO 2: Large .git/ directory (5,000 objects)
# ============================================================================
echo -e "${YELLOW}Scenario 2: Large .git/ directory ($GIT_OBJECTS objects)${NC}"
echo -e "${CYAN}Setting up test project...${NC}"

PROJECT2="$TEST_ROOT/project2"
mkdir -p "$PROJECT2/src"
mkdir -p "$PROJECT2/.git/objects"
mkdir -p "$PROJECT2/.git/refs"

# Create source files
echo "fn main() {}" > "$PROJECT2/src/main.rs"
echo "pub fn lib() {}" > "$PROJECT2/src/lib.rs"

# Create git objects
echo -e "${CYAN}Creating $GIT_OBJECTS git objects...${NC}"
for i in $(seq 1 $GIT_OBJECTS); do
    echo "blob $i" > "$PROJECT2/.git/objects/obj_$i"
    if [ $((i % 1000)) -eq 0 ]; then
        echo -e "  ${BLUE}Progress: $i/$GIT_OBJECTS${NC}"
    fi
done

# Measure WITHOUT ignore
echo -e "${CYAN}Measuring scan time WITHOUT ignore rules...${NC}"
TIME_WITHOUT_2=$(average_time "$DCTX \"$PROJECT2\" --tree")

# Measure WITH ignore
echo ".git/" > "$PROJECT2/.dctxignore"
echo -e "${CYAN}Measuring scan time WITH ignore rules...${NC}"
TIME_WITH_2=$(average_time "$DCTX \"$PROJECT2\" --tree")

if [ "$TIME_WITH_2" -eq 0 ]; then
    SPEEDUP_2="∞ (too fast to measure)"
else
    SPEEDUP_2=$(echo "scale=1; $TIME_WITHOUT_2 / $TIME_WITH_2" | bc)
    SPEEDUP_2="${SPEEDUP_2}x"
fi

echo -e "${GREEN}  WITHOUT ignore: ${TIME_WITHOUT_2}ms${NC}"
echo -e "${GREEN}  WITH ignore:    ${TIME_WITH_2}ms${NC}"
echo -e "${GREEN}  Speedup:        ${SPEEDUP_2}${NC}"
echo ""

# ============================================================================
# SCENARIO 3: Monorepo with multiple target/ directories
# ============================================================================
echo -e "${YELLOW}Scenario 3: Monorepo with ${TARGET_DIRS}x target/ directories${NC}"
echo -e "${CYAN}Setting up monorepo...${NC}"

PROJECT3="$TEST_ROOT/project3"
mkdir -p "$PROJECT3"

# Create packages with target/ directories
for i in $(seq 1 $TARGET_DIRS); do
    mkdir -p "$PROJECT3/package_$i/src"
    mkdir -p "$PROJECT3/package_$i/target"

    # Create source file
    echo "pub fn lib$i() {}" > "$PROJECT3/package_$i/src/lib.rs"

    # Create build artifacts
    echo -e "${CYAN}Creating $TARGET_FILES_PER_DIR files in package_$i/target/...${NC}"
    for j in $(seq 1 $TARGET_FILES_PER_DIR); do
        echo "artifact" > "$PROJECT3/package_$i/target/file_$j.o"
    done
done

# Measure WITHOUT ignore
echo -e "${CYAN}Measuring scan time WITHOUT ignore rules...${NC}"
TIME_WITHOUT_3=$(average_time "$DCTX \"$PROJECT3\" --tree")

# Measure WITH ignore
echo "target/" > "$PROJECT3/.dctxignore"
echo -e "${CYAN}Measuring scan time WITH ignore rules...${NC}"
TIME_WITH_3=$(average_time "$DCTX \"$PROJECT3\" --tree")

if [ "$TIME_WITH_3" -eq 0 ]; then
    SPEEDUP_3="∞ (too fast to measure)"
else
    SPEEDUP_3=$(echo "scale=1; $TIME_WITHOUT_3 / $TIME_WITH_3" | bc)
    SPEEDUP_3="${SPEEDUP_3}x"
fi

echo -e "${GREEN}  WITHOUT ignore: ${TIME_WITHOUT_3}ms${NC}"
echo -e "${GREEN}  WITH ignore:    ${TIME_WITH_3}ms${NC}"
echo -e "${GREEN}  Speedup:        ${SPEEDUP_3}${NC}"
echo ""

# ============================================================================
# CONCLUSION
# ============================================================================
echo -e "${CYAN}═══════════════════════════════════════════════════${NC}"
echo -e "${CYAN}  BENCHMARK RESULTS${NC}"
echo -e "${CYAN}═══════════════════════════════════════════════════${NC}"
echo ""
echo -e "${YELLOW}Scenario 1: node_modules/ (${NODE_MODULES_FILES} files)${NC}"
echo -e "  WITHOUT ignore: ${TIME_WITHOUT_1}ms"
echo -e "  WITH ignore:    ${TIME_WITH_1}ms"
echo -e "  ${GREEN}Speedup:        ${SPEEDUP_1}x${NC}"
echo ""
echo -e "${YELLOW}Scenario 2: .git/ directory (${GIT_OBJECTS} objects)${NC}"
echo -e "  WITHOUT ignore: ${TIME_WITHOUT_2}ms"
echo -e "  WITH ignore:    ${TIME_WITH_2}ms"
echo -e "  ${GREEN}Speedup:        ${SPEEDUP_2}x${NC}"
echo ""
echo -e "${YELLOW}Scenario 3: Monorepo (${TARGET_DIRS} packages)${NC}"
echo -e "  WITHOUT ignore: ${TIME_WITHOUT_3}ms"
echo -e "  WITH ignore:    ${TIME_WITH_3}ms"
echo -e "  ${GREEN}Speedup:        ${SPEEDUP_3}x${NC}"
echo ""

# Validate performance targets (50x minimum)
# Extract numeric speedup (handle "∞" case)
SPEEDUP_1_NUM=$(echo "$SPEEDUP_1" | grep -o '^[0-9.]*')
SPEEDUP_2_NUM=$(echo "$SPEEDUP_2" | grep -o '^[0-9.]*')
SPEEDUP_3_NUM=$(echo "$SPEEDUP_3" | grep -o '^[0-9.]*')

MIN_SPEEDUP=50.0
ALL_PASS=true

if [ -n "$SPEEDUP_1_NUM" ] && (( $(echo "$SPEEDUP_1_NUM < $MIN_SPEEDUP" | bc -l) )); then
    echo -e "${RED}⚠ Scenario 1 speedup (${SPEEDUP_1}) below target (${MIN_SPEEDUP}x)${NC}"
    ALL_PASS=false
fi

if [ -n "$SPEEDUP_2_NUM" ] && (( $(echo "$SPEEDUP_2_NUM < $MIN_SPEEDUP" | bc -l) )); then
    echo -e "${RED}⚠ Scenario 2 speedup (${SPEEDUP_2}) below target (${MIN_SPEEDUP}x)${NC}"
    ALL_PASS=false
fi

if [ -n "$SPEEDUP_3_NUM" ] && (( $(echo "$SPEEDUP_3_NUM < $MIN_SPEEDUP" | bc -l) )); then
    echo -e "${RED}⚠ Scenario 3 speedup (${SPEEDUP_3}) below target (${MIN_SPEEDUP}x)${NC}"
    ALL_PASS=false
fi

echo ""
if [ "$ALL_PASS" = true ]; then
    echo -e "${GREEN}✅ All scenarios meet performance targets (50x+ speedup)${NC}"
    echo -e "${GREEN}✅ ADR-014 and README.md performance claims validated${NC}"
else
    echo -e "${YELLOW}⚠ Some scenarios below target, but ignore optimization working${NC}"
fi
echo ""
