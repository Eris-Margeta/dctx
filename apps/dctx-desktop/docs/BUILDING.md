<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Building dctx Desktop from Source

If you prefer to build the app yourself rather than using the unsigned binaries, follow this guide.

---

## Prerequisites

### All Platforms

**Rust** (1.70+)
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

**Bun** (1.0+)
```bash
curl -fsSL https://bun.sh/install | bash
```

### macOS

**Xcode Command Line Tools**
```bash
xcode-select --install
```

### Windows

**Visual Studio Build Tools**
1. Download from: https://visualstudio.microsoft.com/downloads/
2. Install "Desktop development with C++" workload

### Linux (Debian/Ubuntu)

**Ubuntu 24.04+ / Debian 13+**
```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libglib2.0-dev \
  build-essential \
  curl \
  wget \
  file \
  libssl-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

**Ubuntu 22.04 / Debian 12 and earlier**
```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.0-dev \
  libglib2.0-dev \
  build-essential \
  curl \
  wget \
  file \
  libssl-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

### Linux (Fedora)

```bash
sudo dnf install \
  webkit2gtk4.0-devel \
  openssl-devel \
  curl \
  wget \
  file \
  gtk3-devel \
  libappindicator-gtk3-devel \
  librsvg2-devel
```

### Linux (Arch)

```bash
sudo pacman -S \
  webkit2gtk \
  base-devel \
  curl \
  wget \
  file \
  openssl \
  gtk3 \
  libappindicator-gtk3 \
  librsvg
```

---

## Build Steps

### 1. Clone Repository

```bash
git clone https://github.com/your-username/dctx.git
cd dctx/apps/dctx-desktop
```

### 2. Install Dependencies

```bash
bun install
```

### 3. Build the App

**Development build (faster, larger):**
```bash
bun run tauri build --debug
```

**Production build (optimized):**
```bash
bun run tauri build
```

Build time: ~5-10 minutes (first time), ~2-3 minutes (subsequent)

---

## Output Locations

### macOS

**App Bundle:**
```
src-tauri/target/release/bundle/macos/dctx Desktop.app
```

**DMG Installer:**
```
src-tauri/target/release/bundle/dmg/dctx Desktop_*.dmg
```

**To install:**
```bash
open "src-tauri/target/release/bundle/dmg/dctx Desktop_*.dmg"
# Drag app to Applications folder
```

### Windows

**MSI Installer:**
```
src-tauri/target/release/bundle/msi/dctx-desktop_*.msi
```

**To install:**
```powershell
.\src-tauri\target\release\bundle\msi\dctx-desktop_*.msi
```

### Linux

**Debian Package (.deb):**
```
src-tauri/target/release/bundle/deb/dctx-desktop_*.deb
```

**AppImage:**
```
src-tauri/target/release/bundle/appimage/dctx-desktop_*.AppImage
```

**To install (.deb):**
```bash
sudo dpkg -i src-tauri/target/release/bundle/deb/dctx-desktop_*.deb
sudo apt-get install -f  # Fix dependencies
```

**To run (AppImage):**
```bash
chmod +x src-tauri/target/release/bundle/appimage/dctx-desktop_*.AppImage
./src-tauri/target/release/bundle/appimage/dctx-desktop_*.AppImage
```

---

## Troubleshooting

### "command not found: bun"

**Solution:**
```bash
curl -fsSL https://bun.sh/install | bash
# Restart terminal or:
source ~/.bashrc  # Linux
source ~/.zshrc   # macOS
```

### "rustc not found"

**Solution:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

### macOS: "xcrun: error: unable to find utility"

**Solution:**
```bash
xcode-select --install
sudo xcode-select --reset
```

### Linux: "webkit2gtk not found"

**Solution (Debian/Ubuntu):**
```bash
sudo apt-get install libwebkit2gtk-4.0-dev
```

### Windows: "LINK : fatal error LNK1181"

**Solution:** Install Visual Studio Build Tools with C++ workload

### Build fails with "out of disk space"

**Solution:** Need at least 5GB free disk space for build artifacts

---

## Development Mode

To run in development mode with hot reload:

```bash
bun run tauri:dev
```

This is faster for development as it doesn't create production bundles.

---

## Clean Build

If you encounter issues, try a clean build:

```bash
# Clean all build artifacts
rm -rf dist/
rm -rf node_modules/
rm -rf src-tauri/target/

# Reinstall and rebuild
bun install
bun run tauri build
```

---

## Build Time Expectations

| Platform | First Build | Subsequent |
|----------|-------------|------------|
| macOS (M1/M2) | 5-7 min | 2-3 min |
| macOS (Intel) | 8-10 min | 3-4 min |
| Windows | 10-15 min | 4-5 min |
| Linux | 8-12 min | 3-4 min |

*Times are for production builds. Debug builds are ~50% faster.*

---

## Verifying Your Build

After building, verify the app works:

**macOS:**
```bash
open "src-tauri/target/release/bundle/macos/dctx Desktop.app"
```

**Windows:**
```powershell
.\src-tauri\target\release\dctx-desktop.exe
```

**Linux:**
```bash
./src-tauri/target/release/dctx-desktop
```

---

## Binary Size

Expected sizes for production builds:

| Platform | Size |
|----------|------|
| macOS | 5-8 MB |
| Windows | 6-9 MB |
| Linux | 6-9 MB |

If your binary is much larger (>20 MB), you may be using a debug build.

---

## Need Help?

- **Issues**: [GitHub Issues](https://github.com/your-repo/dctx/issues)
- **Docs**: [DEVELOPMENT.md](DEVELOPMENT.md)
- **Community**: [Discord](https://discord.gg/your-invite)

---

**Built with:** Tauri, Rust, Bun, TypeScript
