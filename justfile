# dctx development commands

# Format all code (core, CLI, desktop)
fmt:
    cargo fmt --all

# Check formatting without modifying
fmt-check:
    cargo fmt --all -- --check

# Run all tests (core + CLI only, desktop has no tests)
test:
    cargo test -p dctx-core -p dctx-cli

# Run clippy lints (core + CLI only)
lint:
    cargo clippy -p dctx-core -p dctx-cli -- -D warnings

# Build release binary (CLI only)
build:
    cargo build --release -p dctx-cli

# v1 release gate on Linux: cargo build/test/clippy, shipped-code scan, determinism, unrelated edit, zero network,
# then the committed bench floors. `just release-check --quick path/to/dctx-cli` skips cargo.
release-check *ARGS:
    ./scripts/release-check.sh {{ARGS}}
    python3 scripts/bench_floors.py

# Fail if the README bench block differs from the one rendered from the committed bench JSON.
# `python3 scripts/render_readme.py` (no flag) rewrites the block.
readme-check:
    python3 scripts/render_readme.py --check

# Render Mermaid diagrams from docs/diagrams/*.mmd into docs/assets/*.svg
diagrams:
    #!/usr/bin/env bash
    set -euo pipefail

    if ! command -v mmdc >/dev/null 2>&1; then
        if ! command -v pnpm >/dev/null 2>&1; then
            echo "ERROR: Mermaid CLI is required. Install mmdc or pnpm."
            echo "Install option: pnpm add -g @mermaid-js/mermaid-cli"
            exit 1
        fi
        RENDERER="pnpm dlx --allow-build=puppeteer @mermaid-js/mermaid-cli"
    else
        RENDERER="mmdc"
    fi

    if [ -z "${PUPPETEER_EXECUTABLE_PATH:-}" ]; then
        if [ -x "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" ]; then
            export PUPPETEER_EXECUTABLE_PATH="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
            export PUPPETEER_SKIP_DOWNLOAD=true
        else
            browser="$(find "$HOME/.cache/puppeteer" "$HOME/Library/Caches/ms-playwright" -type f -name chrome-headless-shell -perm -111 2>/dev/null | sort | tail -n 1 || true)"
            if [ -n "$browser" ]; then
                export PUPPETEER_EXECUTABLE_PATH="$browser"
                export PUPPETEER_SKIP_DOWNLOAD=true
            fi
        fi
    fi

    mkdir -p docs/assets

    shopt -s nullglob
    sources=(docs/diagrams/*.mmd)
    if [ ${#sources[@]} -eq 0 ]; then
        echo "No Mermaid source files found in docs/diagrams/"
        exit 0
    fi

    for source in "${sources[@]}"; do
        name="$(basename "${source%.mmd}")"
        target="docs/assets/${name}.svg"
        echo "Rendering ${source} -> ${target}"
        ${RENDERER} -i "${source}" -o "${target}" -c docs/diagrams/mermaid-config.json --backgroundColor transparent
    done

# ============================================================================
# Desktop App Commands
# ============================================================================

# Build desktop app frontend (required before any Rust desktop commands)
desktop-frontend:
    #!/usr/bin/env bash
    cd apps/dctx-desktop
    bun install
    bun run build

# Lint desktop app (builds frontend first)
desktop-lint: desktop-frontend
    cargo clippy -p dctx-desktop -- -D warnings

# Build desktop app (builds frontend first)
desktop-build: desktop-frontend
    cd apps/dctx-desktop && bun run tauri build

# Run desktop app in dev mode (with hot reload)
desktop-dev:
    cd apps/dctx-desktop && bun run tauri:dev

# Check everything including desktop app
check-all: fmt-check lint desktop-lint

# Run all CI checks locally (format, lint, test, desktop-lint)
ci: fmt-check lint test desktop-lint

# Clean build artifacts
clean:
    cargo clean

# Check (default) or, with `--write`, sync every crates/* and plugins/* version and their Cargo.lock entries to VERSION
sync-version *ARGS:
    ./tools/sync-version.sh {{ARGS}}

# Run integrity tests
test-integrity:
    ./tools/test-integrity.sh

# Run performance tests
test-perf:
    ./tools/performance-test.sh

# Install locally
install:
    ./tools/install.sh

# Uninstall dctx (removes binary, cache, and config)
uninstall:
    #!/bin/bash
    echo "🗑️  Uninstalling dctx..."
    # Remove binary
    if [ -f "/usr/local/bin/dctx" ]; then
        echo "   - Removing binary: /usr/local/bin/dctx"
        if [ -w "/usr/local/bin" ]; then
            rm -f /usr/local/bin/dctx
        else
            echo "   🔐 Sudo required to remove from /usr/local/bin"
            sudo rm -f /usr/local/bin/dctx
        fi
    else
        echo "   ⚠️  Binary not found at /usr/local/bin/dctx"
    fi
    # Remove cache
    if [ -d "$HOME/.cache/dctx" ]; then
        echo "   - Removing cache: $HOME/.cache/dctx"
        rm -rf "$HOME/.cache/dctx"
    fi
    # Remove config
    if [ -d "$HOME/.config/dctx" ]; then
        echo "   - Removing config: $HOME/.config/dctx"
        rm -rf "$HOME/.config/dctx"
    fi
    echo "✅ Uninstallation complete!"

# Create and push release tag (triggers release workflow)
release:
    #!/bin/bash
    set -e

    # Read version from VERSION file
    VERSION=$(cat VERSION | tr -d '[:space:]')
    TAG="v$VERSION"

    echo "🚀 Preparing release for version: $TAG"
    echo ""

    # Step 1: Verify changelog entry exists
    echo "📝 Step 1: Verifying changelog entry for $VERSION..."
    if ! grep -q "^## \[$VERSION\]" docs/DOCUMENTS/CHANGELOG.md; then
        echo "❌ No changelog entry found for version $VERSION"
        echo "   Please add a changelog entry in docs/DOCUMENTS/CHANGELOG.md"
        exit 1
    fi
    echo "✅ Changelog entry found"
    echo ""

    # Step 2: Show changelog preview
    echo "📋 Changelog for $VERSION:"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    ./tools/extract-changelog.sh "$VERSION" | head -20
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""

    # Step 3: Sync version to all files
    echo "🔄 Step 3: Syncing version across project files..."
    ./tools/sync-version.sh
    echo ""

    # Step 4: Check for uncommitted changes
    echo "🔍 Step 4: Checking for uncommitted changes..."
    if ! git diff-index --quiet HEAD --; then
        echo "⚠️  You have uncommitted changes:"
        git status --short
        echo ""
        read -p "Commit these changes before releasing? (y/N) " -n 1 -r
        echo ""
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            git add -A
            git commit -m "chore: prepare release $TAG"
            echo "✅ Changes committed"
        else
            echo "❌ Please commit or stash your changes before releasing"
            exit 1
        fi
    else
        echo "✅ Working directory clean"
    fi
    echo ""

    # Step 5: Check if tag already exists
    echo "🏷️  Step 5: Checking if tag exists..."
    if git rev-parse "$TAG" >/dev/null 2>&1; then
        echo "❌ Tag $TAG already exists!"
        echo "   Delete it first with: git tag -d $TAG && git push origin :refs/tags/$TAG"
        exit 1
    fi
    echo "✅ Tag $TAG does not exist yet"
    echo ""

    # Step 6: Create tag
    echo "🏷️  Step 6: Creating tag $TAG..."
    git tag -a "$TAG" -m "Release $TAG"
    echo "✅ Tag created: $TAG"
    echo ""

    # Step 7: Confirm push
    echo "📤 Step 7: Ready to push tag and trigger release workflow"
    echo "   This will:"
    echo "   - Push tag $TAG to origin"
    echo "   - Trigger GitHub Actions release workflow"
    echo "   - Build binaries for all platforms"
    echo "   - Create GitHub release with artifacts"
    echo ""
    read -p "Push tag $TAG to origin? (y/N) " -n 1 -r
    echo ""
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "❌ Release cancelled. Tag created locally but not pushed."
        echo "   To push later: git push origin $TAG"
        echo "   To delete tag: git tag -d $TAG"
        exit 1
    fi

    # Step 8: Push tag
    echo "📤 Pushing tag $TAG..."
    git push origin "$TAG"
    echo ""
    echo "✅ Release $TAG pushed successfully!"
    echo "   🔗 Check release progress: https://github.com/Eris-Margeta/dctx/actions"
    echo "   📦 Release will be available at: https://github.com/Eris-Margeta/dctx/releases/tag/$TAG"

# Test on Linux via Docker
test-linux:
    docker run --rm -v "$PWD":/app -w /app rust:latest cargo test --all

# Website: build, then check (isolated rebuild, capture drift, links, budgets). Needs tools/website/.venv.
site:
    tools/website/.venv/bin/python tools/website/build.py
    DCTX_BIN=target/release/dctx-cli tools/website/.venv/bin/python tools/website/check.py

# Re-record the CLI outputs the website shows (after a CLI change), then rebuild.
site-capture:
    DCTX_BIN=target/release/dctx-cli python3 tools/website/capture.py
    tools/website/.venv/bin/python tools/website/build.py

# Preview the built site on this machine.
site-serve:
    python3 -m http.server 8089 --bind 127.0.0.1 --directory website

# One-time: regenerate the subset WOFF2 fonts from the variable TTFs.
site-fonts:
    tools/website/.venv/bin/python tools/website/fonts.py presentation/video/fonts

# Playground server: tests, then the Docker build context in tools/playground/dist/.
play:
    cargo test --locked --manifest-path tools/playground/Cargo.toml
    tools/playground/build-dist.sh
