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

### Stage 6: Lint Desktop App (GATE for builds)
**Job:** `lint-desktop`
**Platform:** Ubuntu
**Depends on:** `test-windows`
**Only runs on:** Push to `main` or `dev` branches

Lints the desktop app with full system dependencies installed.

**System dependencies installed:**
- libwebkit2gtk-4.0-dev
- libglib2.0-dev (CRITICAL for Tauri 2.0)
- build-essential, curl, wget, file
- libssl-dev, libgtk-3-dev
- libayatana-appindicator3-dev
- librsvg2-dev

```bash
cargo clippy -p dctx-desktop -- -D warnings
```

**Why separate stage?** Desktop app lint requires compiling Tauri with all system dependencies. We want to catch any desktop-specific issues before spending 30+ minutes building on all platforms.

If this stage fails, all builds are cancelled.

### Stage 7: Build Desktop App (Linux)
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

### Stage 8: Build Desktop App (macOS)
**Job:** `build-desktop-macos`
**Platform:** macOS
**Depends on:** `build-desktop-linux`
**Only runs on:** Push to `main` or `dev` branches

Only runs if Linux build succeeds.

**Artifacts uploaded:**
- `.dmg` installer
- `.app` bundle

If this stage fails, Windows build is cancelled.

### Stage 9: Build Desktop App (Windows)
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
6. lint-desktop (Ubuntu, ~3min) [Desktop app with full deps]
     ↓ (if pass)
7. build-desktop-linux (Ubuntu, ~10min)
     ↓ (if pass)
8. build-desktop-macos (macOS, ~10min)
     ↓ (if pass)
9. build-desktop-windows (Windows, ~10min)
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
- Skips desktop lint and builds (PRs typically don't touch desktop app)

## Key Fixes Applied

1. **Split format and lint into separate stages** - Format is ultra-fast (no compilation), lint only checks core/CLI
2. **Test only core and CLI** - Desktop app has no tests and would require Tauri dependencies
3. **Separate desktop lint stage** - Catches desktop issues before expensive multi-platform builds
4. **Added `libglib2.0-dev` to Linux dependencies** - Required by Tauri 2.0 for glib-sys crate
5. **Added frontend build step** - Creates `dist/` directory before Tauri build
6. **Sequential execution** - Each stage depends on previous (`needs:` key)
7. **Early exit on failure** - One failure cancels all subsequent jobs

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
