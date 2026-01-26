# Getting Started - dctx Desktop

**Welcome to dctx Desktop!** This guide will get you up and running in 5 minutes.

---

## What is dctx Desktop?

A **fast, native desktop app** for generating AI-ready context snapshots of your codebase. Built with Tauri and Rust for maximum performance.

**Key Features**:
- 🎯 Drag & drop folders
- 📂 Native file picker
- ⚡ Blazing fast (< 500ms startup)
- 🎨 Beautiful light/dark mode UI
- 🔒 100% local, private
- 🪶 Tiny (5-8 MB binary)

---

## Quick Install

### 1. Install Prerequisites

**macOS**:
```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Bun
curl -fsSL https://bun.sh/install | bash

# Install Xcode Command Line Tools
xcode-select --install
```

**Windows**:
1. Install Rust: https://rustup.rs
2. Install Bun: https://bun.sh
3. Install Visual Studio Build Tools

**Linux**:
```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Bun
curl -fsSL https://bun.sh/install | bash

# Install dependencies (Ubuntu/Debian)
sudo apt-get install libwebkit2gtk-4.0-dev \
    build-essential \
    curl \
    wget \
    libssl-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev
```

### 2. Clone & Build

```bash
# Navigate to desktop app
cd apps/dctx-desktop

# Install dependencies
bun install

# Run the app!
bun run tauri:dev
```

**That's it!** The app should launch in a few seconds.

---

## First Use

### Test It Out

1. **Drag & Drop**: Drag a folder from Finder/Explorer onto the window
2. **Or Browse**: Click "📂 Browse Files" button
3. **Or Type**: Enter a path manually
4. **Generate**: Click "Generate Snapshot"
5. **View**: See the results!

### Try These Paths

**Small (fast)**:
- macOS: `/Applications/Utilities`
- Windows: `C:\Windows\System32\drivers`
- Linux: `/usr/local/bin`

**Medium**:
- Your projects folder with a small project

**Large (will error if >10k files)**:
- `/Applications` - Should show safety error

---

## Understanding the Output

Currently shows:
```
✅ Snapshot generated successfully!

Path: /your/folder
Files found: 361
Tree-only mode: false
Filters active: false

📊 Statistics:
  • Total files: 361
  • Total directories: 5

💡 Tip: Use filters to reduce the output size.
```

**Coming in v0.2.0**: Full formatted snapshot output with file contents.

---

## Configuration

### Global Config (Shared with CLI)

Create `~/.config/dctx/config.toml`:

```toml
text_output_enabled = true
omit_svg_content = true
max_diff_files = 999
default_template = "default"
```

### Desktop Config (Desktop-specific)

Create `~/.config/dctx-desktop/config.toml`:

```toml
# Increase file limit for large monorepos
file_count_limit = 50000

# Last used directory (auto-saved in future)
last_directory = "/Users/you/projects"

# Window size
window_width = 1400
window_height = 900

# Theme: "system", "light", or "dark"
theme = "system"
```

**Tip**: Desktop config automatically inherits global config settings.

---

## Troubleshooting

### App Won't Launch

```bash
# Clean rebuild
rm -rf dist/ src-tauri/target/
bun install
cargo build --manifest-path src-tauri/Cargo.toml
bun run tauri:dev
```

### Drag & Drop Not Working

1. Check console (Cmd+Option+I) for errors
2. See [DRAG_DROP_DEBUG.md](DRAG_DROP_DEBUG.md)

### Browse Button Not Working

**Error**: "dialog.open not allowed"

**Fix**: Permissions issue, see [DEVELOPMENT.md](DEVELOPMENT.md) → Troubleshooting

### "Directory too large" Error

**Cause**: Directory has > 10,000 files (default safety limit)

**Fix**: Edit `~/.config/dctx-desktop/config.toml`:
```toml
file_count_limit = 50000
```

---

## Next Steps

### For Users

1. **Read the docs**: [README.md](README.md)
2. **Configure settings**: [CONFIG.md](CONFIG.md)
3. **Try different directories**
4. **Give feedback**: Report issues or request features

### For Developers

1. **Read developer guide**: [DEVELOPMENT.md](DEVELOPMENT.md)
2. **Understand architecture**: [PHASE_11_COMPLETE.md](PHASE_11_COMPLETE.md)
3. **Browse the code**: Start with `src/main.ts` and `src-tauri/src/main.rs`
4. **Make improvements**: See [CHANGELOG.md](CHANGELOG.md) → Roadmap

---

## Common Commands

```bash
# Run app (full features)
bun run tauri:dev

# Run in browser (UI dev only, no Tauri APIs)
bun run dev

# Build for production
bun run tauri:build

# Build frontend only
bun run build

# Type check
bun run build:ts

# Build backend only
cargo build --manifest-path src-tauri/Cargo.toml
```

---

## Documentation

- **[README.md](README.md)** - Feature overview and user guide
- **[DEVELOPMENT.md](DEVELOPMENT.md)** - Complete developer guide
- **[CONFIG.md](CONFIG.md)** - Configuration system
- **[DRAG_DROP_DEBUG.md](DRAG_DROP_DEBUG.md)** - Drag & drop troubleshooting
- **[CHANGELOG.md](CHANGELOG.md)** - Version history and roadmap
- **[DOCS_INDEX.md](DOCS_INDEX.md)** - Documentation navigation

---

## Get Help

- **Issues**: GitHub Issues
- **Email**: support@dctx.app
- **Docs**: See `docs/` folder

---

## What's Next?

**v0.2.0 (Coming Soon)**:
- Visual filter configuration
- Real-time tree preview
- Copy to clipboard
- Settings panel
- Recent projects dropdown

**See**: [CHANGELOG.md](CHANGELOG.md) for full roadmap

---

**Happy snapshotting! 📸**
