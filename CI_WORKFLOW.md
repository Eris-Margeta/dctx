# CI Workflow Documentation

## Overview

The CI workflow is designed for **sequential, fail-fast execution** to save compute time and provide faster feedback.

**12-stage workflow:**
1. Format check (30s)
2. Lint core+CLI (2min)
3-5. Test core+CLI on all platforms (15min)
6-8. Build CLI binaries on all platforms (10min) - only on push to main/dev
9. Lint desktop app (3min) - only on push to main/dev
10-12. Build desktop app on all platforms (30min) - only on push to main/dev

## Workflow Stages

### Stage 1: Format Check (GATE)
**Job:** `format-check`
**Platform:** Ubuntu
**Duration:** ~30 seconds
**Purpose:** Ultra-fast format check (no compilation needed)

If this stage fails, **ALL** subsequent stages are cancelled immediately.

```
Checks:
- cargo fmt --all -- --check
```

### Stage 2: Lint Core + CLI (GATE)
**Job:** `lint-check`
**Platform:** Ubuntu
**Duration:** ~2 minutes
**Purpose:** Lint core and CLI only (skips desktop app to avoid Tauri dependencies)

Only runs if format-check passes. If this fails, all tests and builds are cancelled.

```
Checks:
- cargo clippy -p dctx-core -p dctx-cli -- -D warnings
```

**Why skip desktop app?** Linting the desktop app requires compiling Tauri, which needs system dependencies like `glib-2.0`. This would slow down the gate check from 2 minutes to 5+ minutes.

### Stage 3: Test Core + CLI (Linux)
**Job:** `test-linux`
**Platform:** Ubuntu
**Depends on:** `lint-check`

Runs tests for **dctx-core** and **dctx-cli** only on Linux (fastest platform).

```bash
cargo test -p dctx-core -p dctx-cli --verbose
```

**Why skip desktop app?** The desktop app has no tests (it's a GUI). Testing it would require compiling Tauri and installing system dependencies.

If this stage fails, macOS and Windows tests are cancelled.

### Stage 4: Test Core + CLI (macOS)
**Job:** `test-macos`
**Platform:** macOS
**Depends on:** `test-linux`

Only runs if Linux tests pass. Tests core and CLI only.

If this stage fails, Windows tests are cancelled.

### Stage 5: Test Core + CLI (Windows)
**Job:** `test-windows`
**Platform:** Windows
**Depends on:** `test-macos`

Only runs if Linux and macOS tests pass. Tests core and CLI only.

If this stage fails, desktop lint and builds are cancelled.

### Stage 6: Build CLI (Linux)
**Job:** `build-cli-linux`
**Platform:** Ubuntu
**Depends on:** `test-windows`
**Only runs on:** Push to `main` or `dev` branches

Builds the CLI binary for Linux (x86_64).

```bash
cargo build --release -p dctx-cli --target x86_64-unknown-linux-gnu
```

**Artifact:** `dctx-cli-linux-x64`

### Stage 7: Build CLI (macOS)
**Job:** `build-cli-macos`
**Platform:** macOS
**Depends on:** `build-cli-linux`
**Only runs on:** Push to `main` or `dev` branches

Builds CLI binaries for macOS (Intel + Apple Silicon).

**Targets:**
- `x86_64-apple-darwin` (Intel)
- `aarch64-apple-darwin` (Apple Silicon)

**Artifacts:**
- `dctx-cli-macos-x86_64-apple-darwin`
- `dctx-cli-macos-aarch64-apple-darwin`

### Stage 8: Build CLI (Windows)
**Job:** `build-cli-windows`
**Platform:** Windows
**Depends on:** `build-cli-macos`
**Only runs on:** Push to `main` or `dev` branches

Builds the CLI binary for Windows (x86_64).

**Artifact:** `dctx-cli-windows-x64` (dctx-cli.exe)

### Stage 9: Lint Desktop App (GATE for desktop builds)
**Job:** `lint-desktop`
**Platform:** Ubuntu
**Depends on:** `build-cli-windows`
**Only runs on:** Push to `main` or `dev` branches

Lints the desktop app with full system dependencies installed.

**System dependencies installed:**
- libwebkit2gtk-4.1-dev (updated for Ubuntu 24.04)
- libglib2.0-dev (CRITICAL for Tauri 2.0)
- build-essential, curl, wget, file
- libssl-dev, libgtk-3-dev
- libayatana-appindicator3-dev
- librsvg2-dev

**Build process:**
1. Setup Bun
2. Install frontend dependencies (`bun install`)
3. **Build frontend** (`bun run build` → creates `dist/`)
4. Lint Rust code (`cargo clippy -p dctx-desktop`)

```bash
# Frontend must be built BEFORE linting/compiling
cd apps/dctx-desktop
bun install
bun run build

# Then lint
cargo clippy -p dctx-desktop -- -D warnings
```

**Why build frontend first?** Tauri's `generate_context!()` macro validates the `frontendDist` path at compile time. If `dist/` doesn't exist, compilation fails.

**Why separate stage?** Desktop app lint requires compiling Tauri with all system dependencies. We want to catch any desktop-specific issues before spending 30+ minutes building desktop apps on all platforms.

If this stage fails, all desktop builds are cancelled (CLI builds already completed).

### Stage 10: Build Desktop App (Linux)
**Job:** `build-desktop-linux`
**Platform:** Ubuntu
**Depends on:** `lint-desktop`
**Only runs on:** Push to `main` or `dev` branches

Builds the Tauri desktop app for Linux (dependencies already verified by lint-desktop stage).

**Build process:**
1. Install Bun (10-100x faster than npm)
2. Install frontend dependencies (`bun install`)
3. Build frontend (`bun run build` → creates `dist/`)
4. Build Tauri app (`bun run tauri build`)

**Artifacts uploaded:**
- `.deb` package (Debian/Ubuntu)
- `.AppImage` (Universal Linux)

If this stage fails, macOS and Windows builds are cancelled.

### Stage 11: Build Desktop App (macOS)
**Job:** `build-desktop-macos`
**Platform:** macOS
**Depends on:** `build-desktop-linux`
**Only runs on:** Push to `main` or `dev` branches

Only runs if Linux build succeeds.

**Artifacts uploaded:**
- `.dmg` installer
- `.app` bundle

If this stage fails, Windows build is cancelled.

### Stage 12: Build Desktop App (Windows)
**Job:** `build-desktop-windows`
**Platform:** Windows
**Depends on:** `build-desktop-macos`
**Only runs on:** Push to `main` or `dev` branches

Only runs if Linux and macOS builds succeed.

**Artifacts uploaded:**
- `.msi` installer
- `.exe` NSIS installer

## Execution Flow

```
1. format-check (Ubuntu, ~30s)
     ↓ (if pass)
2. lint-check (Ubuntu, ~2min) [Core + CLI only]
     ↓ (if pass)
3. test-linux (Ubuntu, ~5min) [Core + CLI only]
     ↓ (if pass)
4. test-macos (macOS, ~5min) [Core + CLI only]
     ↓ (if pass)
5. test-windows (Windows, ~5min) [Core + CLI only]
     ↓ (if pass AND push to main/dev)
6. build-cli-linux (Ubuntu, ~3min)
     ↓ (if pass)
7. build-cli-macos (macOS, ~6min) [2 targets: Intel + ARM]
     ↓ (if pass)
8. build-cli-windows (Windows, ~3min)
     ↓ (if pass)
9. lint-desktop (Ubuntu, ~3min) [Desktop app with full deps]
     ↓ (if pass)
10. build-desktop-linux (Ubuntu, ~10min)
     ↓ (if pass)
11. build-desktop-macos (macOS, ~10min)
     ↓ (if pass)
12. build-desktop-windows (Windows, ~10min)
```

## Why Sequential?

**Cost Efficiency:**
- Failing early saves GitHub Actions minutes
- No point building Windows if Linux tests fail

**Fast Feedback:**
- Format issues caught in ~30 seconds
- Lint issues caught in ~2 minutes
- Test failures caught in ~7 minutes
- CLI build issues caught in ~20 minutes
- Desktop lint issues caught in ~23 minutes
- Desktop build issues caught in ~50 minutes

**Predictable Testing:**
- Linux first (fastest, most common CI platform)
- macOS second (Apple platform, unique bugs)
- Windows last (slowest, least common CI bugs in Rust)

## Triggers

**Push to `main` or `dev`:**
- Runs all 12 stages (format → lint → test → build CLI → lint desktop → build desktop)

**Pull request to `main` or `dev`:**
- Runs stages 1-5 only (format → lint → test all platforms)
- Skips CLI builds, desktop lint, and desktop builds (artifacts not needed for PRs)

## Key Fixes Applied

1. **Split format and lint into separate stages** - Format is ultra-fast (no compilation), lint only checks core/CLI
2. **Test only core and CLI** - Desktop app has no tests and would require Tauri dependencies
3. **Build CLI binaries on all platforms** - Verifies release builds work and creates distribution artifacts
4. **Separate desktop lint stage** - Catches desktop issues before expensive multi-platform builds
5. **Added `libglib2.0-dev` to Linux dependencies** - Required by Tauri 2.0 for glib-sys crate
6. **Updated to libwebkit2gtk-4.1-dev** - Required for Ubuntu 24.04+ (GitHub Actions runners)
7. **Added frontend build step** - Creates `dist/` directory before Tauri build
8. **Sequential execution** - Each stage depends on previous (`needs:` key)
9. **Early exit on failure** - One failure cancels all subsequent jobs

## Monitoring

Check workflow status at:
```
https://github.com/YOUR_REPO/actions
```

Each stage shows:
- ✅ Green checkmark = passed
- ❌ Red X = failed (subsequent stages cancelled)
- ⏭️ Skipped = previous stage failed or not applicable

## Local Testing

Before pushing, run locally:

```bash
# Format check
cargo fmt --all -- --check

# Lint check
cargo clippy --all -- -D warnings

# Run tests
cargo test --all --verbose

# Build desktop app (requires system dependencies)
cd apps/dctx-desktop
bun install
bun run build
bun run tauri build
```

## Troubleshooting

**"libglib2.0-dev not found" on Linux:**
- Fixed in workflow with `sudo apt-get install -y libglib2.0-dev`

**"libwebkit2gtk-4.0-dev not found" on Ubuntu 24.04:**
- Fixed by using libwebkit2gtk-4.1-dev (new package name in Ubuntu 24.04)

**"gdk-3.0 not found" when testing:**
- Fixed by only testing core/CLI packages, not desktop app
- Desktop app has no tests anyway

**"frontendDist path doesn't exist" on any platform:**
- Fixed with frontend build step before Tauri build

**"Desktop lint failed, but I want to build anyway":**
- Fix desktop lint first (sequential is intentional)
- Desktop lint catches issues before wasting 30+ minutes on builds

**"Builds take too long":**
- Builds only run on push to `main`/`dev`, not on PRs
- Rust caching is enabled (`Swatinem/rust-cache@v2`)
- Bun is much faster than npm for frontend builds
