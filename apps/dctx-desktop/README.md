# dctx Desktop

> **Cross-platform GUI for dctx - Directory Context Generator**

**License**: Apache-2.0 · free, forever
**Status**: v0.1.0 - Working prototype
**Platform**: macOS, Windows, Linux

---

## About

dctx Desktop is a modern, cross-platform GUI application for generating AI-ready context snapshots from your codebase. Built with **Tauri 2.0** and powered by the open-source `dctx-core` library.

### Why Desktop?

- **Fast & Native**: Built with Rust, runs at native speed with tiny memory footprint
- **No Internet Required**: All processing happens locally on your machine
- **Privacy First**: Your code never leaves your computer
- **Beautiful UI**: Modern light/dark mode design that adapts to your system

---

## Features (v0.1.0-alpha)

### ✅ Working Features

- **Drag & Drop** - Drop folders directly onto the app window
- **Native File Picker** - Browse for folders with native OS dialog
- **Manual Path Entry** - Type or paste directory paths
- **Async Snapshot Generation** - Non-blocking, UI stays responsive
- **Safety Limits** - Prevents memory exhaustion on large directories (10k file limit)
- **Config Inheritance** - Uses global `~/.config/dctx/config.toml` settings
- **Light/Dark Mode** - Automatically follows system theme
- **Fast Startup** - Launches in < 500ms
- **Small Binary** - Only 5-8MB (uses system WebView)

### 🚧 Coming Soon (v0.2.0)

- Visual filter configuration UI
- Real-time directory tree preview
- Copy to clipboard button
- Settings panel
- Recent projects history

---

## Quick Start

### Prerequisites

- **Rust 1.70+** - Install from [rustup.rs](https://rustup.rs)
- **Bun 1.0+** - Install from [bun.sh](https://bun.sh)

### Installation (Development)

```bash
# Clone repo (if not already)
cd apps/dctx-desktop

# Install dependencies
bun install

# Run desktop app
bun run tauri:dev
```

### Build for Production

```bash
# Build optimized binary
bun run tauri:build
```

**Output locations:**
- **macOS**: `src-tauri/target/release/bundle/macos/dctx Desktop.app`
- **Windows**: `src-tauri/target/release/bundle/msi/dctx-desktop_0.11.0_x64_en-US.msi`
- **Linux**: `src-tauri/target/release/bundle/deb/dctx-desktop_0.11.0_amd64.deb`

---

## Usage

### 1. Select a Directory

**Three ways to select:**

1. **Drag & Drop**: Drag a folder from Finder/Explorer onto the window
2. **Browse Button**: Click "📂 Browse Files" and use native picker
3. **Manual Entry**: Type the full path (e.g., `/Users/you/projects/my-app`)

### 2. Generate Snapshot

Click **"Generate Snapshot"** button.

**What happens:**
- Walks directory structure using `.gitignore` rules
- Counts files and directories
- Checks against safety limit (10,000 files)
- Returns summary with statistics

### 3. View Results

Output shows:
- Path scanned
- Number of files found
- Number of directories
- Filters applied (if any)

### Safety Features

**File Count Limit**: Prevents loading directories with > 10,000 files by default.

**Configurable**: Edit `~/.config/dctx-desktop/config.toml`:
```toml
file_count_limit = 50000  # Increase for large monorepos
```

---

## Configuration

### Two-Tier Config System

1. **Global dctx config**: `~/.config/dctx/config.toml` (shared with CLI)
2. **Desktop config**: `~/.config/dctx-desktop/config.toml` (desktop-specific)

Desktop settings **inherit** from global config and can override them.

**Example global config** (`~/.config/dctx/config.toml`):
```toml
text_output_enabled = true
omit_svg_content = true
max_diff_files = 999
default_template = "default"
```

**Example desktop config** (`~/.config/dctx-desktop/config.toml`):
```toml
last_directory = "/Users/you/projects/my-app"
window_width = 1200
window_height = 800
file_count_limit = 10000
theme = "system"
```

**See**: [CONFIG.md](CONFIG.md) for full documentation

---

## Architecture

```
apps/dctx-desktop/
├── src/                      # Frontend (TypeScript)
│   └── main.ts               # UI logic, event handlers
├── src-tauri/                # Backend (Rust)
│   ├── src/
│   │   ├── main.rs           # Tauri entry point
│   │   └── config.rs         # Config management
│   ├── capabilities/         # Tauri 2.0 permissions
│   │   └── default.json
│   ├── Cargo.toml
│   └── tauri.conf.json       # Tauri configuration
├── index.html                # Main UI
├── package.json              # Bun dependencies
├── vite.config.ts            # Vite bundler
├── CHANGELOG.md              # Version history
├── CONFIG.md                 # Configuration guide
├── DEVELOPMENT.md            # Developer docs
└── VERSION                   # Current version
```

### Tech Stack

| Layer | Technology |
|-------|------------|
| **Frontend** | TypeScript, Vite |
| **Backend** | Rust, Tauri 2.0 |
| **Core Logic** | dctx-core (MIT) |
| **UI** | Custom CSS (no framework) |
| **Package Manager** | Bun (10-100x faster than npm) |
| **Runtime** | System WebView (no Electron bloat) |

---

## Performance

| Metric | Value |
|--------|-------|
| **Binary Size** | 5-8 MB (vs 100+ MB for Electron) |
| **Startup Time** | < 500ms |
| **Memory Usage** | ~50 MB idle |
| **Scan Speed** | 10,000 files/sec |
| **UI Responsiveness** | Always responsive (async backend) |

---

## Development

### Project Structure

```
Frontend (TypeScript/Vite)
  ↓ invoke("generate_snapshot")
Backend (Rust/Tauri)
  ↓ calls
dctx-core (MIT library)
  ↓ walks filesystem
Returns snapshot
```

### Key Files

| File | Purpose |
|------|---------|
| `src/main.ts` | Frontend logic, drag-drop, events |
| `src-tauri/src/main.rs` | Backend commands, Tauri setup |
| `src-tauri/src/config.rs` | Config loading and defaults |
| `src-tauri/capabilities/default.json` | Tauri 2.0 permissions |
| `index.html` | UI layout and styling |

### Development Commands

```bash
# Run in development mode
bun run tauri:dev

# Run frontend only (browser dev mode)
bun run dev

# Build frontend
bun run build

# Build Rust backend
cargo build --manifest-path src-tauri/Cargo.toml

# Type check
bun run build:ts

# Build production app
bun run tauri:build
```

### Browser Dev Mode

**Note**: `bun run dev` opens the UI in a regular browser for rapid development. Tauri APIs won't work in browser mode, but UI/CSS changes are instant.

**For full features**, use: `bun run tauri:dev`

**See**: [DEVELOPMENT.md](DEVELOPMENT.md) for detailed guide

---

## Troubleshooting

### Drag & Drop Not Working

1. **Check permissions** - Tauri 2.0 requires explicit capabilities
2. **Check console** - Open DevTools (Cmd+Option+I) and look for errors
3. **Verify config** - Ensure `dragDropEnabled: true` in `tauri.conf.json`

**See**: [DRAG_DROP_DEBUG.md](DRAG_DROP_DEBUG.md)

### Browse Button Not Working

**Error**: "dialog.open not allowed"

**Fix**: Ensure capabilities are configured in `tauri.conf.json`:
```json
{
  "security": {
    "capabilities": [{
      "permissions": ["dialog:allow-open", "dialog:default"]
    }]
  }
}
```

### App Freezes on Large Directories

**Cause**: Directory has > 10,000 files

**Fix**: Either:
1. Select a smaller subdirectory
2. Increase limit in `~/.config/dctx-desktop/config.toml`:
   ```toml
   file_count_limit = 50000
   ```

---

## Licensing

dctx Desktop is **free, forever** — for individuals, teams and companies — and licensed under Apache-2.0 like the rest of this repository. See the root [LICENSE](../../LICENSE) and the announcement on [dctx.app](https://dctx.app/news/desktop-free-forever/).

Organizations that need enterprise-ready builds, organization and tenant policy control and direct support can license **dctx Enterprise** (per company, from €45,000 per year).

---

## Roadmap

### ✅ v0.1.0-alpha (Current)

- [x] Tauri 2.0 setup
- [x] Drag & drop folder selection
- [x] Native file picker
- [x] Async snapshot generation
- [x] Config system integration
- [x] Light/dark mode
- [x] Safety limits

### 🚧 v0.2.0 (Next - 2 weeks)

- [ ] Visual filter configuration UI
- [ ] Real-time tree preview
- [ ] Copy to clipboard button
- [ ] Settings panel
- [ ] Recent projects dropdown

### 🔮 v0.3.0 (4 weeks)

- [ ] Visual diff viewer
- [ ] Template selection UI
- [ ] Project bookmarks
- [ ] Search within snapshot

### 🚀 v1.0.0 (8 weeks)

- [ ] Code signing (macOS + Windows)
- [ ] Auto-updater
- [ ] License key system
- [ ] Payment integration
- [ ] Production release to dctx.app

---

## System Requirements

### Minimum

- **OS**: macOS 10.15+, Windows 10+, or Linux (glibc 2.27+)
- **RAM**: 512 MB
- **Disk**: 50 MB
- **CPU**: Any modern 64-bit processor

### Recommended

- **RAM**: 2 GB (for large codebases)
- **Disk**: 100 MB (with cache)

---

## Support

- **Documentation**: See `docs/` folder
- **Issues**: GitHub Issues (for bug reports)
- **Email**: support@dctx.app
- **Website**: https://dctx.app (coming soon)

---

## Credits

**Built by**: Eris Margeta
**Core Library**: dctx-core (MIT) - See [crates/dctx-core](../../crates/dctx-core/)
**Powered by**: Tauri, Rust, Bun

---

## Related Projects

- **dctx CLI** - Free command-line tool (MIT)
- **dctx-core** - Core library for Rust (MIT)
- **dctx-py** - Python bindings (planned)
- **dctx-node** - Node.js bindings (planned)

---

**Made with ❤️ for developers who work with AI assistants.**
