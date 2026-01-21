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

# Test on Linux via Docker
test-linux:
    docker run --rm -v "$PWD":/app -w /app rust:latest cargo test --all
