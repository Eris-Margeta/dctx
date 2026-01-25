# dctx development commands

# Format all code
fmt:
    cargo fmt --all

# Check formatting without modifying
fmt-check:
    cargo fmt --all -- --check

# Run all tests
test:
    cargo test --all

# Run clippy lints
lint:
    cargo clippy --all -- -D warnings

# Build release binary
build:
    cargo build --release

# Run all CI checks locally (format, lint, test)
ci: fmt-check lint test

# Clean build artifacts
clean:
    cargo clean

# Sync version from VERSION file to Cargo.toml
sync-version:
    ./tools/sync-version.sh

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

    # Step 1: Sync version to all files
    echo "📋 Step 1: Syncing version across project files..."
    ./tools/sync-version.sh
    echo ""

    # Step 2: Check for uncommitted changes
    echo "🔍 Step 2: Checking for uncommitted changes..."
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

    # Step 3: Check if tag already exists
    echo "🏷️  Step 3: Checking if tag exists..."
    if git rev-parse "$TAG" >/dev/null 2>&1; then
        echo "❌ Tag $TAG already exists!"
        echo "   Delete it first with: git tag -d $TAG && git push origin :refs/tags/$TAG"
        exit 1
    fi
    echo "✅ Tag $TAG does not exist yet"
    echo ""

    # Step 4: Create tag
    echo "🏷️  Step 4: Creating tag $TAG..."
    git tag -a "$TAG" -m "Release $TAG"
    echo "✅ Tag created: $TAG"
    echo ""

    # Step 5: Confirm push
    echo "📤 Step 5: Ready to push tag and trigger release workflow"
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

    # Step 6: Push tag
    echo "📤 Pushing tag $TAG..."
    git push origin "$TAG"
    echo ""
    echo "✅ Release $TAG pushed successfully!"
    echo "   🔗 Check release progress: https://github.com/Eris-Margeta/dctx/actions"
    echo "   📦 Release will be available at: https://github.com/Eris-Margeta/dctx/releases/tag/$TAG"

# Test on Linux via Docker
test-linux:
    docker run --rm -v "$PWD":/app -w /app rust:latest cargo test --all
