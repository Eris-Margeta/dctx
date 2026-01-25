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

# Test on Linux via Docker
test-linux:
    docker run --rm -v "$PWD":/app -w /app rust:latest cargo test --all
