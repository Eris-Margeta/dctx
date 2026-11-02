# Development Guide - dctx Desktop

Complete guide for developing the dctx Desktop application.

---

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Setup](#setup)
3. [Development Workflow](#development-workflow)
4. [Architecture](#architecture)
5. [Key Concepts](#key-concepts)
6. [Testing](#testing)
7. [Building](#building)
8. [Debugging](#debugging)
9. [Common Tasks](#common-tasks)
10. [Troubleshooting](#troubleshooting)

---

## Prerequisites

### Required

- **Rust 1.70+** - [Install rustup](https://rustup.rs)
- **Bun 1.0+** - [Install Bun](https://bun.sh)
- **Xcode Command Line Tools** (macOS only)
  ```bash
  xcode-select --install
  ```

### Verify Installation

```bash
rust --version   # Should be 1.70+
bun --version    # Should be 1.0+
cargo --version  # Should be 1.70+
```

---

## Setup

### 1. Clone and Navigate

```bash
cd apps/dctx-desktop
```

### 2. Install Dependencies

```bash
# Install frontend dependencies
bun install

# Backend dependencies are managed by Cargo
# (will be installed automatically on first build)
```

### 3. Verify Setup

```bash
# Build backend
cargo build --manifest-path src-tauri/Cargo.toml

# Build frontend
bun run build

# Run app
bun run tauri:dev
```

---

## Development Workflow

### Quick Start

```bash
# Start development server with hot reload
bun run tauri:dev
```

**What this does:**
1. Starts Vite dev server on `localhost:1420`
2. Compiles Rust backend
3. Launches Tauri window with hot reload
4. Frontend changes reload automatically
5. Backend changes require restart

### Two Development Modes

#### 1. Full App Mode (Recommended)

```bash
bun run tauri:dev
```

**Features:**
- ✅ All Tauri APIs work
- ✅ Drag & drop works
- ✅ Native file picker works
- ✅ Backend commands work
- ⚠️ Slower iteration (Rust rebuild on change)

#### 2. Browser Mode (UI Development)

```bash
bun run dev
```

**Features:**
- ✅ Instant hot reload
- ✅ UI/CSS changes are instant
- ✅ Chrome DevTools
- ❌ No Tauri APIs
- ❌ Drag & drop simulation only
- ❌ Backend commands disabled

**Use for:** Rapid UI/CSS iteration without waiting for Rust compilation.

---

## Architecture

### High-Level Flow

```
┌─────────────────────────────────────────┐
│           Frontend (TypeScript)          │
│  ┌─────────────────────────────────┐    │
│  │  UI Layer (index.html)          │    │
│  │  • Drag-drop zone               │    │
│  │  • Browse button                │    │
│  │  • Input field                  │    │
│  │  • Output display               │    │
│  └──────────────┬──────────────────┘    │
│                 │                         │
│  ┌──────────────▼──────────────────┐    │
│  │  Logic Layer (main.ts)          │    │
│  │  • Event handlers               │    │
│  │  • Tauri invoke()               │    │
│  │  • State management             │    │
│  └──────────────┬──────────────────┘    │
└─────────────────┼───────────────────────┘
                  │ invoke("generate_snapshot")
                  │
┌─────────────────▼───────────────────────┐
│        Backend (Rust + Tauri)            │
│  ┌─────────────────────────────────┐    │
│  │  Tauri Layer (main.rs)          │    │
│  │  • Commands (#[tauri::command]) │    │
│  │  • Permissions                  │    │
│  │  • Window config                │    │
│  └──────────────┬──────────────────┘    │
│                 │                         │
│  ┌──────────────▼──────────────────┐    │
│  │  Config Layer (config.rs)       │    │
│  │  • Load dctx config             │    │
│  │  • Load desktop config          │    │
│  │  • Merge settings               │    │
│  └──────────────┬──────────────────┘    │
│                 │                         │
│  ┌──────────────▼──────────────────┐    │
│  │  Core Logic (dctx-core)         │    │
│  │  • walk_directory_structure()   │    │
│  │  • load_ignore_rules()          │    │
│  │  • filter_tree()                │    │
│  └─────────────────────────────────┘    │
└─────────────────────────────────────────┘
```

### Directory Structure

```
apps/dctx-desktop/
│
├── Frontend Files
│   ├── src/
│   │   └── main.ts              # Frontend logic
│   ├── index.html               # UI layout
│   ├── package.json             # Bun dependencies
│   ├── vite.config.ts           # Vite config
│   └── tsconfig.json            # TypeScript config
│
├── Backend Files
│   └── src-tauri/
│       ├── src/
│       │   ├── main.rs          # Tauri entry point
│       │   └── config.rs        # Config module
│       ├── capabilities/
│       │   └── default.json     # Tauri 2.0 permissions
│       ├── Cargo.toml           # Rust dependencies
│       └── tauri.conf.json      # Tauri configuration
│
├── Documentation
│   ├── README.md                # User-facing docs
│   ├── DEVELOPMENT.md           # This file
│   ├── CONFIG.md                # Configuration guide
│   ├── DRAG_DROP_DEBUG.md       # Drag & drop debugging
│   └── CHANGELOG.md             # Version history
│
└── Metadata
    ├── VERSION                  # Current version
    └── LICENSE                  # Apache-2.0 license
```

---

## Key Concepts

### 1. Tauri Commands

**Backend (Rust)**:
```rust
// src-tauri/src/main.rs
#[tauri::command]
async fn generate_snapshot(request: SnapshotRequest) -> Result<SnapshotResponse, String> {
    // Backend logic here
    Ok(SnapshotResponse { success: true, ... })
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![generate_snapshot])
        .run(...)
}
```

**Frontend (TypeScript)**:
```typescript
// src/main.ts
import { invoke } from "@tauri-apps/api/core";

const response = await invoke("generate_snapshot", {
    request: { path: "/my/project" }
});
```

### 2. Tauri 2.0 Permissions

**All APIs require explicit permissions** in `tauri.conf.json`:

```json
{
  "security": {
    "capabilities": [{
      "identifier": "default",
      "windows": ["main"],
      "permissions": [
        "core:event:allow-listen",    // For drag-drop events
        "dialog:allow-open"            // For browse button
      ]
    }]
  }
}
```

### 3. Config System

**Two-tier inheritance:**

1. **Core config** (`~/.config/dctx/config.toml`) - Shared with CLI
2. **Desktop config** (`~/.config/dctx-desktop/config.toml`) - Desktop overrides

**Code**:
```rust
// src-tauri/src/config.rs
pub fn load_desktop_config() -> DesktopConfig {
    let mut config = DesktopConfig::default();
    config.core = load_app_config();  // Inherit from core
    // Load desktop-specific overrides...
    config
}
```

### 4. Async Backend

**All heavy operations run in background threads:**

```rust
#[tauri::command]
async fn generate_snapshot(...) -> Result<...> {
    tokio::task::spawn_blocking(move || {
        // Heavy work here (won't block UI)
        walker::walk_directory_structure(...)
    }).await?
}
```

### 5. Environment Detection

**Frontend detects runtime environment:**

```typescript
const isTauri = '__TAURI__' in window;

if (!isTauri) {
    console.log("Running in browser - Tauri APIs disabled");
}
```

---

## Testing

### Manual Testing

```bash
# Run app in dev mode
bun run tauri:dev
```

**Test checklist:**
- [ ] Drag & drop a folder
- [ ] Click "Browse Files" button
- [ ] Enter path manually
- [ ] Generate snapshot on small directory (~100 files)
- [ ] Try large directory (should show error if >10k files)
- [ ] Check light/dark mode switching

### Automated Tests

**Backend tests:**
```bash
cd src-tauri
cargo test
```

**Frontend tests:**
```bash
bun run build:ts  # Type check
```

### Performance Testing

```bash
# Build release binary
bun run tauri:build

# Test startup time
time open "src-tauri/target/release/bundle/macos/dctx Desktop.app"

# Test memory usage
# (Run app and check Activity Monitor / Task Manager)
```

---

## Building

### Development Build

```bash
# Fast debug build
cargo build --manifest-path src-tauri/Cargo.toml
bun run build
```

### Production Build

```bash
# Optimized release build
bun run tauri:build
```

**Outputs:**

| Platform | Output Location |
|----------|----------------|
| **macOS** | `src-tauri/target/release/bundle/macos/dctx Desktop.app` |
| **Windows** | `src-tauri/target/release/bundle/msi/dctx-desktop_*.msi` |
| **Linux** | `src-tauri/target/release/bundle/deb/dctx-desktop_*.deb` |

### Build Optimization

**Reduce binary size:**

Add to `src-tauri/Cargo.toml`:
```toml
[profile.release]
opt-level = "z"     # Optimize for size
lto = true          # Link-time optimization
codegen-units = 1   # Better optimization
strip = true        # Remove debug symbols
```

---

## Debugging

### Frontend Debugging

**Open DevTools in app:**
- macOS: `Cmd+Option+I`
- Windows/Linux: `Ctrl+Shift+I`

**Or in browser mode:**
```bash
bun run dev  # Opens in Chrome with full DevTools
```

### Backend Debugging

**Print to terminal:**
```rust
println!("Debug: {:?}", some_variable);
eprintln!("Error: {}", error_message);
```

**Logs appear in terminal running `bun run tauri:dev`**

### Rust Debugging with LLDB

```bash
cd src-tauri
cargo build
lldb target/debug/dctx-desktop
```

### Check Tauri Permissions

**Look for permission errors in console:**
```
"event.listen not allowed" → Missing core:event:allow-listen
"dialog.open not allowed" → Missing dialog:allow-open
```

**Fix in `tauri.conf.json`** (see Permissions section)

---

## Common Tasks

### Add a New Backend Command

1. **Define command in `main.rs`:**
```rust
#[tauri::command]
fn my_command(arg: String) -> Result<String, String> {
    Ok(format!("Got: {}", arg))
}
```

2. **Register in builder:**
```rust
.invoke_handler(tauri::generate_handler![
    generate_snapshot,
    my_command  // Add here
])
```

3. **Call from frontend:**
```typescript
const result = await invoke("my_command", { arg: "hello" });
```

### Add a New Permission

**Edit `tauri.conf.json`:**
```json
{
  "security": {
    "capabilities": [{
      "permissions": [
        "new:permission:here"
      ]
    }]
  }
}
```

### Change Window Size

**Edit `tauri.conf.json`:**
```json
{
  "app": {
    "windows": [{
      "width": 1400,
      "height": 900
    }]
  }
}
```

### Add a Frontend Dependency

```bash
bun add package-name
```

### Add a Backend Dependency

**Edit `src-tauri/Cargo.toml`:**
```toml
[dependencies]
new-crate = "1.0"
```

Then: `cargo build --manifest-path src-tauri/Cargo.toml`

### Update Tauri

```bash
cd src-tauri
cargo update tauri
cargo update tauri-plugin-dialog
cargo update tauri-plugin-shell
```

---

## Troubleshooting

### "Command not found: bun"

**Solution:**
```bash
curl -fsSL https://bun.sh/install | bash
```

### "rustc not found"

**Solution:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### App Won't Launch

**Check:**
1. Backend compiled? `cargo build --manifest-path src-tauri/Cargo.toml`
2. Frontend built? `bun run build`
3. Permissions correct? Check `tauri.conf.json`

**Clean rebuild:**
```bash
rm -rf dist/ src-tauri/target/
bun install
cargo build --manifest-path src-tauri/Cargo.toml
bun run build
bun run tauri:dev
```

### Drag & Drop Not Working

**See**: [DRAG_DROP_DEBUG.md](DRAG_DROP_DEBUG.md)

**Quick check:**
1. Is `dragDropEnabled: true` in `tauri.conf.json`?
2. Are permissions set correctly?
3. Is event listener registered? (Check console logs)

### Build Fails on macOS

**Error**: "xcrun: error: unable to find utility"

**Solution:**
```bash
xcode-select --install
sudo xcode-select --reset
```

### Binary Too Large

**Current size**: ~5-8 MB

**If larger:**
1. Check `Cargo.toml` has release optimizations
2. Ensure `strip = true` in `[profile.release]`
3. Remove unused dependencies

---

## Code Style

### TypeScript

- Use `const` over `let` when possible
- Prefer `async/await` over `.then()`
- Use type annotations for function parameters
- Add JSDoc comments for complex functions

### Rust

- Follow standard Rust conventions
- Use `cargo fmt` before committing
- Add doc comments (`///`) for public functions
- Prefer `Result<T, E>` over panicking

---

## Git Workflow

```bash
# Create feature branch
git checkout -b feature/my-feature

# Make changes...

# Test
bun run tauri:dev

# Build
bun run tauri:build

# Commit
git add .
git commit -m "feat: add my feature"

# Push
git push origin feature/my-feature
```

---

## Performance Tips

1. **Use `async` for heavy operations** - Don't block UI thread
2. **Lazy load config** - Only load when needed
3. **Debounce user input** - Don't process every keystroke
4. **Use safety limits** - Prevent memory exhaustion
5. **Profile before optimizing** - Measure first

---

## Resources

### Documentation

- **Tauri**: https://tauri.app/v2/
- **Bun**: https://bun.sh/docs
- **Vite**: https://vitejs.dev/
- **TypeScript**: https://www.typescriptlang.org/docs/

### Tools

- **Tauri DevTools**: Built into dev mode
- **Rust Analyzer**: VSCode extension
- **TypeScript ESLint**: Code linting

### Community

- **Tauri Discord**: https://discord.com/invite/tauri
- **Rust Forums**: https://users.rust-lang.org/
- **GitHub Issues**: For bugs and features

---

**Happy developing! 🚀**
