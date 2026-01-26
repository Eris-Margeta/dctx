# CI Workflow Documentation

## Overview

The CI workflow is designed for **sequential, fail-fast execution** to save compute time and provide faster feedback.

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

Runs all Rust tests for dctx-core and dctx-cli on Linux first (fastest platform).

If this stage fails, macOS and Windows tests are cancelled.

### Stage 4: Test Core + CLI (macOS)
**Job:** `test-macos`
**Platform:** macOS
**Depends on:** `test-linux`

Only runs if Linux tests pass.

If this stage fails, Windows tests are cancelled.

### Stage 5: Test Core + CLI (Windows)
**Job:** `test-windows`
**Platform:** Windows
**Depends on:** `test-macos`

Only runs if Linux and macOS tests pass.

If this stage fails, all builds are cancelled.

### Stage 6: Build Desktop App (Linux)
**Job:** `build-desktop-linux`
**Platform:** Ubuntu
**Depends on:** `test-windows`
**Only runs on:** Push to `main` or `dev` branches

Builds the Tauri desktop app for Linux.

**System dependencies installed:**
- libwebkit2gtk-4.0-dev
- libglib2.0-dev (CRITICAL for Tauri 2.0)
- build-essential, curl, wget, file
- libssl-dev, libgtk-3-dev
- libayatana-appindicator3-dev
- librsvg2-dev

**Build process:**
1. Install Bun (10-100x faster than npm)
2. Install frontend dependencies (`bun install`)
3. Build frontend (`bun run build` → creates `dist/`)
4. Build Tauri app (`bun run tauri build`)

**Artifacts uploaded:**
- `.deb` package (Debian/Ubuntu)
- `.AppImage` (Universal Linux)

If this stage fails, macOS and Windows builds are cancelled.

### Stage 7: Build Desktop App (macOS)
**Job:** `build-desktop-macos`
**Platform:** macOS
**Depends on:** `build-desktop-linux`
**Only runs on:** Push to `main` or `dev` branches

Only runs if Linux build succeeds.

**Artifacts uploaded:**
- `.dmg` installer
- `.app` bundle

If this stage fails, Windows build is cancelled.

### Stage 8: Build Desktop App (Windows)
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
format-check (Ubuntu, ~30s)
  ↓ (if pass)
lint-check (Ubuntu, ~2min) [Core + CLI only, no Tauri]
  ↓ (if pass)
test-linux (Ubuntu)
  ↓ (if pass)
test-macos (macOS)
  ↓ (if pass)
test-windows (Windows)
  ↓ (if pass AND push to main/dev)
build-desktop-linux (Ubuntu)
  ↓ (if pass)
build-desktop-macos (macOS)
  ↓ (if pass)
build-desktop-windows (Windows)
```

## Why Sequential?

**Cost Efficiency:**
- Failing early saves GitHub Actions minutes
- No point building Windows if Linux tests fail

**Fast Feedback:**
- Format issues caught in ~30 seconds
- Lint issues caught in ~2 minutes
- Linux tests complete in ~5 minutes
- If any fail, you know in <7 minutes total

**Predictable Testing:**
- Linux first (fastest, most common CI platform)
- macOS second (Apple platform, unique bugs)
- Windows last (slowest, least common CI bugs in Rust)

## Triggers

**Push to `main` or `dev`:**
- Runs all 7 stages (format → test all platforms → build all platforms)

**Pull request to `main` or `dev`:**
- Runs stages 1-5 only (format → lint → test all platforms)
- Skips desktop builds (artifacts not needed for PRs)

## Key Fixes Applied

1. **Split format and lint into separate stages** - Format is ultra-fast (no compilation), lint only checks core/CLI
2. **Skip desktop app in lint** - Avoids Tauri compilation and system dependency requirements in early gate
3. **Added `libglib2.0-dev` to Linux dependencies** - Required by Tauri 2.0 for glib-sys crate
4. **Added frontend build step** - Creates `dist/` directory before Tauri build
5. **Sequential execution** - Each stage depends on previous (`needs:` key)
6. **Early exit on failure** - One failure cancels all subsequent jobs

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

**"frontendDist path doesn't exist" on any platform:**
- Fixed with frontend build step before Tauri build

**"Tests failed on Linux, but I want to test macOS anyway":**
- Fix Linux tests first (sequential is intentional)
- Or temporarily comment out `needs: test-linux` in `.github/workflows/ci.yml` (not recommended)

**"Builds take too long":**
- Builds only run on push to `main`/`dev`, not on PRs
- Rust caching is enabled (`Swatinem/rust-cache@v2`)
- Bun is much faster than npm for frontend builds
