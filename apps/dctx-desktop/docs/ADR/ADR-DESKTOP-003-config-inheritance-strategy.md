# ADR-DESKTOP-003: Config Inheritance Strategy (Two-Tier Configuration System)

**Date:** 2026-02-03
**Status:** Accepted

---

## 1. Context and Problem Statement

The dctx desktop application needs configuration management that balances two competing requirements:

1. **Consistency:** Desktop and CLI should behave similarly when processing directories (same ignore rules, same template settings, same output preferences)
2. **Desktop-Specific Settings:** Desktop needs GUI-specific configuration (window size, theme, file count limits, recent projects) that don't apply to CLI

The question is: **How should desktop configuration relate to the global dctx configuration used by the CLI?**

## 2. Decision Drivers

- **User Expectation:** Users expect consistent behavior between CLI and Desktop (same ignore rules, same output)
- **Don't Repeat Yourself:** Configuration should not be duplicated in multiple locations
- **Desktop-Specific Needs:** GUI requires settings (window size, theme) irrelevant to CLI
- **Override Capability:** Desktop users should be able to override global settings when needed
- **Backward Compatibility:** Existing `~/.config/dctx/config.toml` must continue working
- **Discoverability:** Users should understand where to configure what

## 3. Considered Options

### Option 1: Separate Configs (No Inheritance)

Desktop maintains completely independent configuration at `~/.config/dctx-desktop/config.toml` with no connection to global config.

- **Pros:**
  - **Simple implementation:** No inheritance logic required
  - **Clear boundaries:** Desktop config is self-contained
  - **No conflicts:** Desktop changes don't affect CLI
- **Cons:**
  - **Duplication:** User must configure ignore rules, templates, output settings in two places
  - **Inconsistency:** Desktop and CLI can drift, causing confusion
  - **Poor UX:** User edits global config, Desktop doesn't pick it up
  - **Violates DRY:** Same settings defined twice

### Option 2: Full Inheritance with Overrides (Layered Config)

Desktop loads `~/.config/dctx/config.toml` first, then applies overrides from `~/.config/dctx-desktop/config.toml`.

**Config layers:**
```
Layer 1: Core hardcoded defaults
  ↓ (override)
Layer 2: ~/.config/dctx/config.toml (global, shared with CLI)
  ↓ (override)
Layer 3: ~/.config/dctx-desktop/config.toml (desktop-specific)
```

- **Pros:**
  - **Consistency by default:** Desktop inherits CLI settings automatically
  - **No duplication:** Global settings defined once, apply everywhere
  - **Override flexibility:** Desktop can override specific settings when needed
  - **User-friendly:** Edit global config once, affects both CLI and Desktop
  - **Familiar pattern:** Matches CSS cascade, environment variable precedence
- **Cons:**
  - **Implementation complexity:** Requires config merging logic
  - **Debugging:** User must understand layer precedence
  - **Potential confusion:** "I changed config.toml but Desktop didn't update" (if desktop override exists)

### Option 3: Desktop-Only Config with Symlinks

Desktop uses `~/.config/dctx-desktop/config.toml` exclusively, but user can symlink files for sharing.

- **Pros:**
  - **Explicit sharing:** User controls what is shared via symlinks
  - **No magic:** No hidden inheritance logic
- **Cons:**
  - **Advanced user feature:** Symlinks are unfamiliar to non-technical users
  - **Platform issues:** Windows symlinks require admin privileges
  - **Poor default:** New users get no sharing, must discover symlinks manually
  - **Maintenance burden:** User must manage symlinks

## 4. Decision Outcome

**Chosen option:** Full Inheritance with Overrides (Layered Config)

Desktop application implements a two-tier configuration system:
1. **Load global config** from `~/.config/dctx/config.toml` (shared with CLI)
2. **Load desktop config** from `~/.config/dctx-desktop/config.toml` (desktop-specific)
3. **Merge configs** with desktop settings overriding global settings

### Detailed Justification

1. **User Expectation:** Users naturally expect "dctx Desktop" to behave like "dctx CLI" unless explicitly configured otherwise. Inheritance makes this the default.

2. **DRY Principle:** Core settings (ignore rules, templates, output preferences) are defined once in global config and automatically apply to both CLI and Desktop.

3. **Override Safety:** Desktop can safely override settings (e.g., `file_count_limit = 50000`) without affecting CLI behavior.

4. **Familiar Pattern:** Layered configuration is well-understood from CSS, environment variables, and configuration systems in tools like Git, Docker, ESLint.

5. **Future-Proof:** As new Core settings are added, they automatically become available to Desktop without code changes.

### Configuration Structure

**Global Config** (`~/.config/dctx/config.toml`):
```toml
# Shared between CLI and Desktop
text_output_enabled = true
text_output_mode_generates_diff_files = false
omit_svg_content = true
max_diff_files = 999
default_template = "default"
```

**Desktop Config** (`~/.config/dctx-desktop/config.toml`):
```toml
# Desktop-specific settings
file_count_limit = 10000       # Safety limit for GUI
window_width = 1200            # UI state
window_height = 800
theme = "system"               # "light", "dark", "system"
last_directory = "/path/to/project"
recent_projects = [            # Recent directories dropdown
  "/path/to/project1",
  "/path/to/project2"
]

# Optional: Override global settings for Desktop
# text_output_enabled = false  # Desktop might not need .txt files
```

### Config Loading Algorithm

```rust
fn load_desktop_config() -> DesktopConfig {
    // 1. Load hardcoded Core defaults
    let mut config = CoreConfig::default();

    // 2. Load global config (shared with CLI)
    if let Some(global_config) = load_global_config() {
        config.merge(global_config);
    }

    // 3. Load desktop-specific config
    let desktop_config = load_desktop_specific_config();

    // 4. Return combined config with desktop settings
    DesktopConfig {
        core: config,  // Inherited + potentially overridden
        file_count_limit: desktop_config.file_count_limit,
        window_width: desktop_config.window_width,
        // ... other desktop-specific fields
    }
}
```

### Positive Consequences

- **Zero Configuration:** Desktop works immediately without creating desktop-specific config
- **Consistent Behavior:** Ignore rules, templates, output settings shared between CLI and Desktop
- **Flexible Overrides:** Power users can override specific settings for Desktop
- **Single Source of Truth:** Global config is authoritative unless explicitly overridden
- **Easy Debugging:** `dctx --version` shows config paths; user can inspect both files

### Negative Consequences

- **Merge Logic Complexity:** Config loading code is more complex than single-file loading
- **Override Confusion:** Users might not realize desktop config overrides global config
- **Debugging Difficulty:** "Where is this setting coming from?" requires checking two files
- **Documentation Burden:** Must explain layering in docs and help text

## 5. Implementation Plan

1. ✅ **Core Config Loading:** Implement `load_global_config()` that reads `~/.config/dctx/config.toml`
2. ✅ **Desktop Config Loading:** Implement `load_desktop_specific_config()` for `~/.config/dctx-desktop/config.toml`
3. ✅ **Merge Logic:** Implement `DesktopConfig::merge()` to apply overrides
4. ✅ **Default Creation:** Create `~/.config/dctx-desktop/config.toml` on first run with desktop-specific defaults
5. ✅ **Validation:** Log loaded config at startup for debugging
6. 🚧 **Documentation:** Update README.md with config file locations and override behavior
7. 🔮 **UI Settings Panel:** Add GUI for editing desktop config (future v0.3.0)

## 6. Validation

**Success Criteria:**

- ✅ Desktop respects global `text_output_enabled` setting without desktop config
- ✅ Desktop can override global settings via desktop config
- ✅ Desktop-specific settings (window size, theme) don't pollute global config
- ✅ Config loading logs clearly show which file provided which setting
- 🚧 User documentation clearly explains layering

**Test Scenarios:**

| Scenario | Global Config | Desktop Config | Expected Behavior |
|----------|---------------|----------------|-------------------|
| Fresh install | Default | Missing | Uses Core defaults |
| Global only | Custom | Missing | Inherits global settings |
| Desktop override | `text_output_enabled = true` | `text_output_enabled = false` | Desktop uses `false`, CLI uses `true` |
| Desktop-specific | N/A | `window_width = 1600` | Desktop uses 1600px width |

**Performance:**

- Config loading adds < 5ms to startup time (measured)
- Two TOML file reads are negligible overhead

---

## Configuration Precedence Reference

For documentation and user reference:

```
Setting Priority (highest to lowest):
1. Desktop-specific config (~/.config/dctx-desktop/config.toml)
2. Global config (~/.config/dctx/config.toml)
3. Core hardcoded defaults
```

**Example: Determining `text_output_enabled`:**

1. Check `~/.config/dctx-desktop/config.toml` for `text_output_enabled`
   - If present → use this value
   - If absent → continue to step 2
2. Check `~/.config/dctx/config.toml` for `text_output_enabled`
   - If present → use this value
   - If absent → continue to step 3
3. Use hardcoded default (`true`)

---

## Related Decisions

- **ADR-DESKTOP-001:** Tauri Technology Choice (Tauri provides native file system access for config loading)
- **ADR-DESKTOP-002:** Core Dependency Architecture (Desktop uses Core config types directly)

---

## Future Considerations

### Settings UI Panel (v0.3.0)

Future versions should add a Settings panel in the GUI that:
- Shows current effective config (merged result)
- Indicates which settings come from global vs desktop config
- Allows editing desktop config without manual TOML editing
- Provides "Reset to Global" button to remove desktop overrides

### Config Validation

Future versions should validate configs at load time:
- Detect invalid TOML syntax
- Warn about unknown keys (typos)
- Validate value ranges (e.g., `file_count_limit > 0`)

---

**Last Updated:** 2026-02-03
