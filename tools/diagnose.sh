#!/bin/bash
# Comprehensive diagnostic for dctx ignore issues

set -e

TARGET="${1:-.}"
DCTX_BIN="${2:-target/release/dctx-cli}"

echo "========================================="
echo "DCTX Ignore Diagnostic Tool"
echo "========================================="
echo ""

# Navigate to target
cd "$TARGET" || exit 1
ABS_PATH=$(pwd)

echo "Target Directory: $ABS_PATH"
echo ""

# Check for .dctxignore
echo "--- .dctxignore File ---"
if [ -f ".dctxignore" ]; then
    echo "Status: ✓ EXISTS"
    echo "Location: $ABS_PATH/.dctxignore"
    echo "Size: $(wc -c < .dctxignore) bytes"
    echo ""
    echo "Contents (with invisible chars shown):"
    cat -An .dctxignore | head -30
    echo ""

    echo "Checking for node_modules patterns:"
    if grep -n "node_modules" .dctxignore; then
        echo "  ✓ Found"
    else
        echo "  ✗ Not found"
    fi
else
    echo "Status: ✗ MISSING"
    echo "Relying on hardcoded rules only"
fi

echo ""
echo "--- Global Ignore File ---"
GLOBAL="$HOME/.config/dctx/ignore"
if [ -f "$GLOBAL" ]; then
    echo "Status: ✓ EXISTS"
    echo "Contents:"
    cat "$GLOBAL"
else
    echo "Status: ✗ NOT FOUND"
fi

echo ""
echo "--- node_modules Directory ---"
if [ -d "node_modules" ]; then
    echo "Status: ✓ EXISTS"
    echo "Size: $(du -sh node_modules 2>/dev/null | cut -f1)"
    echo "Items: $(find node_modules -type f 2>/dev/null | wc -l) files"
else
    echo "Status: ✗ NOT FOUND"
fi

echo ""
echo "--- Hardcoded Rules (from source) ---"
echo "These are ALWAYS active:"
echo "  - node_modules/"
echo "  - .git/"
echo "  - target/"
echo "  - dist/"
echo "  - build/"

echo ""
echo "--- Running dctx ---"
"$DCTX_BIN" . -r 2>&1 | grep -E "🚀|📂|✅|⚠️|❌"

echo ""
echo "--- Checking Output ---"
if [ -f ".dctx/*.llmcontext.txt" ]; then
    OUTPUT_FILE=$(ls .dctx/*.llmcontext.txt | head -1)
    echo "Output: $OUTPUT_FILE"
    echo ""

    echo "Checking for node_modules in output:"
    if grep -i "node_modules" "$OUTPUT_FILE"; then
        echo "  ❌ FOUND - This is the bug!"
    else
        echo "  ✓ NOT FOUND - Working correctly"
    fi

    echo ""
    echo "File count in output:"
    grep -c "\\[F\\]" "$OUTPUT_FILE" || echo "0"

    echo ""
    echo "Total output size:"
    du -h "$OUTPUT_FILE"
else
    echo "No output file generated"
fi

echo ""
echo "========================================="
echo "Diagnostic Complete"
echo "========================================="
