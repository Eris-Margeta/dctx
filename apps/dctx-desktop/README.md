# dctx Desktop

> **Cross-platform GUI for dctx - Directory Context Generator**

**License**: Proprietary (Commercial) - See [LICENSE](LICENSE)
**Price**: $29 USD (one-time payment)
**Status**: Alpha - Not yet released

---

## About

dctx Desktop is a cross-platform GUI application for generating AI-ready context snapshots from your codebase. Built with Tauri and integrating the open-source `dctx-core` library.

**Key Features** (Planned):
- Drag-and-drop directory selection
- Visual filter configuration
- Real-time snapshot preview
- One-click clipboard copy
- Diff viewer for changes
- Template management
- Project bookmarks

---

## Architecture

```
apps/dctx-desktop/
├── src/               # Frontend (TypeScript + Vite)
│   └── main.ts
├── src-tauri/         # Backend (Rust + Tauri)
│   ├── src/
│   │   └── main.rs   # Tauri commands calling dctx-core
│   └── Cargo.toml    # Depends on dctx-core
├── index.html
├── package.json
└── LICENSE           # PROPRIETARY
```

**Tech Stack**:
- **Frontend**: TypeScript, Vite (minimal for now, Svelte later)
- **Backend**: Tauri 2.0 + dctx-core
- **UI**: Custom CSS (TailwindCSS + shadcn/ui planned)

---

## Development

### Prerequisites

- Rust 1.70+ (for Tauri backend)
- Bun 1.0+ (for frontend) - https://bun.sh

### Setup

```bash
# Install frontend dependencies
bun install

# Run in development mode
bun run tauri:dev
```

### Build

```bash
# Build for production
bun run tauri:build
```

Outputs:
- macOS: `src-tauri/target/release/bundle/macos/dctx Desktop.app`
- Windows: `src-tauri/target/release/bundle/msi/dctx-desktop_0.11.0_x64_en-US.msi`
- Linux: `src-tauri/target/release/bundle/deb/dctx-desktop_0.11.0_amd64.deb`

---

## Integration with dctx-core

The desktop app calls `dctx-core` directly via Tauri commands:

```rust
// src-tauri/src/main.rs
use dctx_core::{walker, FilterConfig};

#[tauri::command]
fn generate_snapshot(path: String) -> Result<String, String> {
    let tree = walker::walk_directory_structure(...)?;
    Ok(llm_formatter::format_snapshot(&tree))
}
```

```typescript
// src/main.ts
import { invoke } from "@tauri-apps/api/core";

const snapshot = await invoke("generate_snapshot", { path: "/my/project" });
```

---

## Licensing

**This application is PROPRIETARY software.**

- Source code is available for transparency and audit
- You may NOT redistribute or modify this software
- Valid license key required after 14-day trial
- See [LICENSE](LICENSE) for full terms

**Open Source Alternative**: Use the free CLI (`dctx-cli`) or the MIT-licensed `dctx-core` library.

---

## Roadmap

**Phase 11 (Current - 7 weeks)**:
- [x] Basic Tauri setup with dctx-core integration
- [ ] File browser with drag-and-drop
- [ ] Filter configuration UI
- [ ] Clipboard integration
- [ ] Diff viewer
- [ ] Template selection
- [ ] Code signing (macOS + Windows)
- [ ] Auto-updater
- [ ] License key system

**Post-Launch**:
- AI API integration (Claude, GPT-4)
- Dark/light theme toggle
- Project templates
- Cloud sync (optional)

---

## Support

For support, email: support@dctx.app

---

**Built with Tauri. Powered by dctx-core (MIT).**
