<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# dctx Desktop Configuration

## Overview

The desktop app uses a **two-tier configuration system**:

1. **Global dctx config** (inherited from CLI): `~/.config/dctx/config.toml`
2. **Desktop-specific config**: `~/.config/dctx-desktop/config.toml`

## Configuration Hierarchy

```
~/.config/
├── dctx/                    # Core dctx CLI config (shared)
│   ├── config.toml          # Global filters, ignore rules, templates
│   └── ignore               # Global ignore patterns
│
└── dctx-desktop/            # Desktop-specific config
    └── config.toml          # Desktop settings (inherits from dctx/)
```

## Cache Directories

```
~/.cache/
├── dctx/                    # Core cache (projects.db, templates)
│   ├── projects.db          # Project registry
│   └── templates/           # Compiled templates
│
└── dctx-desktop/            # Desktop-specific cache
    └── (future: recent files, thumbnails, etc.)
```

## Configuration Files

### Core Config: `~/.config/dctx/config.toml`

Shared with CLI - defines global behavior:

```toml
# Text output behavior
text_output_enabled = true

# SVG content handling
omit_svg_content = true

# Max diff files in output
max_diff_files = 999

# Default template
default_template = "default"

# Git integration
update_local_git_repo_gitignore_rules = true

# Diff file generation
text_output_mode_generates_diff_files = false
```

**Desktop inherits ALL these settings automatically.**

### Desktop Config: `~/.config/dctx-desktop/config.toml`

Desktop-specific overrides and additions:

```toml
# Desktop-specific settings

# Last used directory (auto-saved)
last_directory = "/path/to/my-app"

# Window size
window_width = 1200
window_height = 800

# File count safety limit
file_count_limit = 10000

# Theme preference
theme = "system"  # "system", "light", or "dark"
```

## Behavior

### On Startup

1. Desktop app loads `~/.config/dctx/config.toml` (if exists)
2. Desktop app loads `~/.config/dctx-desktop/config.toml` (if exists)
3. Desktop settings override core settings where applicable
4. Missing configs use sensible defaults

### Configuration Precedence

1. **Desktop-specific settings** (highest priority)
   - Window size, theme, file limit
2. **Core dctx settings** (inherited)
   - Filters, ignore rules, templates
3. **Hardcoded defaults** (fallback)
   - Defined in `dctx-core/src/config.rs`

## Example: Filter Inheritance

If your CLI config has:

```toml
# ~/.config/dctx/config.toml
omit_svg_content = false
```

The desktop app will also show SVG content by default (inherited).

Later, when we add a filter UI, desktop-specific filter overrides will be saved to `~/.config/dctx-desktop/config.toml`.

## File Count Limit

The desktop app enforces a safety limit to prevent memory exhaustion:

- **Default:** 10,000 files
- **Configurable:** Set `file_count_limit` in desktop config
- **Behavior:** Directories exceeding the limit show an error instead of freezing

Example:
```toml
# ~/.config/dctx-desktop/config.toml
file_count_limit = 50000  # Increase for large monorepos
```

## Future: Settings UI

When the settings panel is added (v0.2.0), it will:

1. Display inherited core settings (read-only section)
2. Allow override of core settings for desktop
3. Manage desktop-specific settings (theme, window, limits)
4. Auto-save to `~/.config/dctx-desktop/config.toml`

## Platform Paths

### macOS / Linux
- Config: `~/.config/dctx-desktop/`
- Cache: `~/.cache/dctx-desktop/`

### Windows
- Config: `%APPDATA%\dctx-desktop\`
- Cache: `%LOCALAPPDATA%\dctx-desktop\`

## Implementation

See:
- `apps/dctx-desktop/src-tauri/src/config.rs` - Desktop config module
- `crates/dctx-core/src/config.rs` - Core config module
