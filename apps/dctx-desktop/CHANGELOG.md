# Changelog - dctx Desktop

All notable changes to the desktop application will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Planned
- Visual filter configuration UI
- Real-time directory tree preview
- Copy to clipboard button
- Settings panel with preferences
- Recent projects dropdown

---

## [0.1.0-alpha] - 2026-01-26

### Added
- **Tauri 2.0 Setup** - Rust backend + TypeScript frontend
- **Drag & Drop** - Drop folders onto app window with instant visual feedback
- **Native File Picker** - Browse button with native OS dialog
- **Manual Path Entry** - Type or paste directory paths
- **Config System** - Two-tier config (global + desktop-specific)
  - Inherits from `~/.config/dctx/config.toml`
  - Desktop overrides at `~/.config/dctx-desktop/config.toml`
- **Safety Limits** - 10,000 file limit (configurable) prevents memory exhaustion
- **Async Snapshot Generation** - Non-blocking backend with `tokio::spawn_blocking`
- **Modern UI** - Light/dark mode following system preference
- **Blue Accent Theme** - #3b82f6 (Tailwind blue-500)
- **Environment Detection** - Graceful degradation in browser dev mode
- **Tauri Permissions** - Full Tauri 2.0 capabilities configuration
- **Icon Generation** - All platforms (macOS, Windows, Linux, iOS, Android)
- **Bun Integration** - 10-100x faster than npm for builds
- **Comprehensive Docs** - README, DEVELOPMENT, CONFIG, DRAG_DROP_DEBUG guides

### Changed
- **Migrated from npm to Bun** - Faster builds and installs
- **Snapshot runs in background** - UI never freezes
- **Removed file content loading** - Only walks directory structure (for now)

### Fixed
- **System freeze on large directories** - Was loading all files into RAM synchronously
- **Drag-and-drop permissions** - Added `dragDropEnabled: true` + `core:event:allow-listen`
- **Browse button permissions** - Added `dialog:allow-open` capability
- **Browser mode errors** - Added Tauri detection and graceful fallbacks
- **Event listener setup** - Multiple event name variations for compatibility

### Technical
- Zero-dependency Rust core integration
- Fast startup (< 500ms)
- Small binary size (5-8MB with system WebView)
- Cross-platform: macOS, Windows, Linux
- Async Rust backend with tokio
- TypeScript frontend with Vite
- Custom CSS (no framework bloat)

---

## Roadmap

### v0.2.0 (Planned - Week 1-2)
- [ ] File browser with drag-and-drop support
- [ ] Visual filter configuration UI
- [ ] Real-time directory tree preview
- [ ] Copy to clipboard button
- [ ] Settings panel

### v0.3.0 (Planned - Week 3-4)
- [ ] Visual diff viewer
- [ ] Template selection dropdown
- [ ] Project bookmarks
- [ ] Recent snapshots history
- [ ] Search within snapshot

### v0.4.0 (Planned - Week 5-6)
- [ ] Code signing (macOS + Windows)
- [ ] Auto-updater integration
- [ ] Error handling improvements
- [ ] Loading states with progress bars
- [ ] Keyboard shortcuts

### v1.0.0 (Planned - Week 7)
- [ ] License key system (14-day trial)
- [ ] Payment integration (Gumroad/Stripe)
- [ ] Offline license validation
- [ ] Final polish and bug fixes
- [ ] Release to dctx.app

---

## Version History

| Version | Date | Status | Notes |
|---------|------|--------|-------|
| 0.1.0-alpha | 2026-01-25 | Development | Initial setup |
| 0.2.0 | TBD | Planned | Core features |
| 0.3.0 | TBD | Planned | Advanced features |
| 1.0.0 | TBD | Planned | Production release |

---

**License**: PROPRIETARY - See [LICENSE](LICENSE)
**Core Library**: MIT Licensed - See [dctx-core](../../crates/dctx-core/)
