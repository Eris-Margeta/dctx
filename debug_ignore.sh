#!/bin/bash
# Debug script to diagnose why .dctxignore isn't working

TARGET_DIR="${1:-.}"
cd "$TARGET_DIR" || exit 1

echo "========================================="
echo "DCTX Ignore Debugging Tool"
echo "========================================="
echo ""
echo "1. Target Directory:"
echo "   $(pwd)"
echo ""

echo "2. Looking for .dctxignore file:"
if [ -f ".dctxignore" ]; then
    echo "   ✓ Found: $(pwd)/.dctxignore"
    echo ""
    echo "3. .dctxignore contents:"
    echo "   ---"
    cat -A .dctxignore | head -20  # cat -A shows invisible chars
    echo "   ---"
    echo ""
    echo "4. Checking for node_modules pattern:"
    if grep -E "^[^#]*node_modules" .dctxignore >/dev/null 2>&1; then
        echo "   ✓ Pattern found"
        grep -n "node_modules" .dctxignore
    else
        echo "   ✗ Pattern NOT found"
    fi
else
    echo "   ✗ NOT FOUND: .dctxignore missing in $(pwd)"
fi

echo ""
echo "5. Check if node_modules directory exists:"
if [ -d "node_modules" ]; then
    echo "   ✓ node_modules/ exists"
    echo "   Size: $(du -sh node_modules 2>/dev/null | cut -f1)"
else
    echo "   ✗ node_modules/ does not exist"
fi

echo ""
echo "6. Global ignore file:"
GLOBAL_IGNORE="$HOME/.config/dctx/ignore"
if [ -f "$GLOBAL_IGNORE" ]; then
    echo "   ✓ Found: $GLOBAL_IGNORE"
    if grep -i "node_modules" "$GLOBAL_IGNORE" >/dev/null 2>&1; then
        echo "   Contents related to node_modules:"
        grep -n "node_modules" "$GLOBAL_IGNORE"
    fi
else
    echo "   ✗ No global ignore file"
fi

echo ""
echo "7. Hardcoded rules (from source):"
echo "   - node_modules/ (hardcoded)"
echo "   - .git/"
echo "   - target/"
echo ""

echo "========================================="
echo "Run this to test:"
echo "  dctx . -c 2>&1 | grep node_modules"
echo "========================================="
