# Changelog - dctx Desktop

All notable changes to the desktop application will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added
- Modern UI with light/dark mode support (follows system preference)
- Blue accent color scheme (#3b82f6 - Tailwind blue-500)
- Responsive design with beautiful gradients
- File path input with validation
- Snapshot generation with progress feedback
- Output display with monospace font
- Smooth animations and transitions
- System tray integration (planned)

### Changed
- Migrated from npm to Bun for faster builds
- Updated to Tauri 2.0 stable

---

## [0.1.0-alpha] - 2026-01-25

### Added
- Initial Tauri 2.0 setup with Rust backend
- Direct integration with dctx-core library
- `generate_snapshot` command calling dctx-core
- TypeScript frontend with Vite
- Basic HTML UI with dark theme
- Filter support (extensions, size, binary, hidden, etc.)
- Icon generation for all platforms (macOS, Windows, Linux, iOS, Android)
- PROPRIETARY license for desktop app
- Bun package manager integration

### Technical
- Zero-dependency Rust core integration
- Fast startup (< 500ms)
- Small binary size (~5-8MB with system WebView)
- Cross-platform: macOS, Windows, Linux

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
