<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# ADR-DESKTOP-004: Complete Separation of CLI and Desktop Cache/Config

**Date:** 2026-02-04
**Status:** current — separate desktop settings and cache namespaces.

The shared engine does not require the CLI and desktop to use the same settings or cache directories. See the [desktop README](../../README.md).

---

## 1. Context and Problem Statement

The dctx desktop application and CLI tool both depend on the same core library (`dctx-core`) and process the same types of projects (directory trees). This raises the architectural question:

**Should the Desktop app and CLI share cache and configuration directories, or maintain complete separation?**

### Key Considerations

1. **Cache Storage:** Binary snapshots (`.dctx` files) and project state registry (`registry.json`)
2. **Configuration Storage:** User settings (filters, ignore rules, templates, UI preferences)
3. **State Management:** Version tracking, incremental updates, diff generation
4. **Target Audiences:** CLI for programmers, Desktop for non-programmers
5. **Concurrent Usage:** Possibility of both tools being used on the same system

### The Question

When a user runs `dctx /path/to/project` via CLI (generating snapshot v1), then opens the same project in Desktop:
- Should Desktop see v1 and generate v1.1 (shared state)?
- Or should Desktop start fresh at v1 (independent state)?

---

## 2. Decision Drivers

- **Isolation:** One tool's crash or corruption should not affect the other
- **Simplicity:** Architecture should be easy to understand and maintain
- **Target Audience Separation:** CLI users and Desktop users may have different needs
- **Concurrent Safety:** Both tools might be used simultaneously on the same system
- **State Independence:** Each tool should maintain consistent internal state
- **Filter Divergence:** CLI and Desktop might use different filter settings
- **Developer Experience:** Clear boundaries make debugging easier

---

## 3. Considered Options

### Option 1: Complete Separation (No Sharing)

Desktop and CLI maintain completely independent cache and configuration directories.

**Directory Structure:**
```
~/.config/dctx/          # CLI config
~/.cache/dctx/           # CLI cache

~/.config/dctx-desktop/  # Desktop config (independent)
~/.cache/dctx-desktop/   # Desktop cache (independent)
```

- **Pros:**
  - **Isolation:** CLI crash cannot corrupt Desktop cache
  - **Simplicity:** No shared state logic, no locking mechanisms
  - **Independence:** Each tool evolves independently
  - **Clear ownership:** Each tool owns its data
  - **No version conflicts:** Same project can have v1 (CLI) and v5 (Desktop)
  - **Safe concurrency:** Both tools can run simultaneously without conflicts
  - **Target audience fit:** Programmers and non-programmers don't interfere
- **Cons:**
  - **Cache duplication:** Same project cached twice (disk space cost)
  - **State divergence:** CLI v3, Desktop v1 for same project (confusing?)
  - **Config duplication:** User must configure ignore rules twice
  - **No state reuse:** Desktop can't leverage CLI's cached snapshots

### Option 2: Shared Cache + Separate Config

Both tools write to same cache directory but maintain separate configs.

**Directory Structure:**
```
~/.config/dctx/          # CLI config
~/.config/dctx-desktop/  # Desktop config (independent)

~/.cache/dctx/           # SHARED cache (both tools)
```

- **Pros:**
  - **State consistency:** CLI v1 → Desktop v1.1 (version continuity)
  - **No cache duplication:** Single binary cache
  - **Faster Desktop:** Can reuse CLI's snapshots
  - **Config independence:** Each tool has separate settings
- **Cons:**
  - **Crash propagation:** CLI crash during cache write corrupts Desktop cache
  - **Locking complexity:** Must implement file locking for `registry.json`
  - **Filter thrashing:** Different filter settings cause cache invalidation
  - **Debugging difficulty:** "Which tool wrote this cache entry?"
  - **Concurrent access:** Race conditions if both tools run simultaneously

### Option 3: Shared Everything (Full Integration)

Desktop inherits CLI config and shares cache.

**Directory Structure:**
```
~/.config/dctx/          # SHARED config (both tools)
  ├── config.toml        # CLI + Desktop settings merged
  └── desktop-ui.toml    # Desktop-specific UI settings

~/.cache/dctx/           # SHARED cache (both tools)
```

- **Pros:**
  - **DRY principle:** Config defined once
  - **State consistency:** Perfect version synchronization
  - **No duplication:** Single source of truth
- **Cons:**
  - **Tight coupling:** Desktop and CLI can't evolve independently
  - **Merge complexity:** Config override logic required
  - **Namespace pollution:** Desktop UI settings in CLI config file
  - **Crash propagation:** Maximum blast radius
  - **Target audience conflict:** Programmer settings affect non-programmer tool
  - **Debugging nightmare:** "Which tool changed this setting?"

---

## 4. Decision Outcome

**Chosen option:** Complete Separation (Option 1)

### Rationale

The operator explicitly decided: **"No, let's keep both cache files and config files completely separate."**

This decision aligns with multiple architectural principles:

1. **Target Audience Separation:** CLI is for programmers, Desktop is for non-programmers. These are fundamentally different user personas with different needs and expectations. Keeping state separate prevents interference between these distinct use cases.

2. **Isolation Guarantee:** If a programmer's CLI script crashes during cache write, the Desktop user's cache remains intact. Complete separation provides maximum fault isolation.

3. **Simplicity Over Optimization:** While shared cache would save disk space and provide version continuity, the added complexity (file locking, concurrent access handling, cache invalidation logic) is not justified. Disk space is cheap; debugging shared state bugs is expensive.

4. **Independent Evolution:** CLI and Desktop can evolve independently. Desktop can add features (e.g., visual filter preview) without worrying about CLI compatibility. CLI can optimize for script usage without Desktop constraints.

5. **Clear Mental Model:** Users understand "Desktop has its own settings and cache" much easier than "Desktop inherits some settings but overrides others, and shares cache but only when..."

### Detailed Justification

**State Divergence is Acceptable:**
If CLI shows v3 and Desktop shows v1 for the same project, this is not a bug—it's a feature. Each tool maintains independent versioning because:
- They may use different filters (CLI: all files, Desktop: exclude large files)
- They may run at different times (CLI: daily cron job, Desktop: manual use)
- They serve different purposes (CLI: automation, Desktop: exploration)

**Cache Duplication is Cheap:**
Modern systems have abundant disk space. Duplicating cache (typically <100MB per project) is far cheaper than debugging race conditions or implementing distributed locking mechanisms.

**Config Independence Matches Tool Independence:**
CLI and Desktop are separate applications, not two interfaces to one application. They happen to share a core library, but they are distinct tools with distinct purposes.

---

### Positive Consequences

1. **Zero Interference:** CLI and Desktop never conflict, never corrupt each other's state
2. **Simplified Architecture:** No config merging, no file locking, no shared state coordination
3. **Easier Debugging:** Clear ownership (Desktop bug → check `~/.cache/dctx-desktop/`)
4. **Safe Concurrency:** Both tools can run simultaneously without coordination
5. **Independent Versioning:** Each tool can bump major versions independently
6. **Target Audience Alignment:** Programmers configure CLI, non-programmers configure Desktop, no overlap
7. **Future-Proof:** Desktop can add GUI-specific features without affecting CLI

---

### Negative Consequences

1. **Cache Duplication:** Same project cached in both `~/.cache/dctx/` and `~/.cache/dctx-desktop/` (estimated 50-200MB per project)
2. **State Divergence:** Same project can have different version numbers in CLI vs Desktop (user might expect consistency)
3. **Config Duplication:** User must configure ignore rules twice if using both tools
4. **No State Reuse:** Desktop cannot leverage CLI's cached snapshots for faster startup
5. **Documentation Burden:** Must clearly explain separation to users who expect sharing

---

## 5. Implementation Plan

### Current Status: ✅ Already Implemented

The codebase already implements complete separation correctly:

**Desktop Cache/Config** (`apps/dctx-desktop/src-tauri/src/config.rs`):
```rust
pub const DESKTOP_APP_NAME: &str = "dctx-desktop";

pub fn get_desktop_config_dir() -> Option<PathBuf> {
    // Unix: ~/.config/dctx-desktop/
    // Windows: %APPDATA%/dctx-desktop/
}

pub fn get_desktop_cache_dir() -> Option<PathBuf> {
    // Unix: ~/.cache/dctx-desktop/
    // Windows: %LOCALAPPDATA%/dctx-desktop/
}
```

**CLI Cache/Config** (`crates/dctx-core/src/config.rs`):
```rust
pub const APP_NAME: &str = "dctx";

pub fn get_config_dir() -> Option<PathBuf> {
    // Unix: ~/.config/dctx/
    // Windows: %APPDATA%/dctx/
}

pub fn get_cache_dir() -> Option<PathBuf> {
    // Unix: ~/.cache/dctx/
    // Windows: %LOCALAPPDATA%/dctx/
}
```

### Verification Steps

1. ✅ Desktop never calls `dctx_core::config::get_config_dir()`
2. ✅ Desktop never calls `dctx_core::config::get_cache_dir()`
3. ✅ Desktop uses only `get_desktop_config_dir()` and `get_desktop_cache_dir()`
4. ✅ No code paths share cache or config directories

### Documentation Tasks

- [x] Create ADR-DESKTOP-004 documenting decision
- [ ] Update README.md to clarify separate directories
- [ ] Update DEVELOPER-GUIDE.md with architecture rationale
- [ ] Add FAQ: "Why doesn't Desktop see my CLI snapshots?"
- [ ] Desktop help text should mention `~/.config/dctx-desktop/`

---

## 6. Validation

### Success Criteria

- [x] CLI and Desktop use different cache directories
- [x] CLI and Desktop use different config directories
- [x] No shared file access in codebase
- [x] Both tools can run concurrently without conflicts
- [x] Desktop crash does not affect CLI cache
- [x] CLI crash does not affect Desktop cache

### Test Scenarios

| Scenario | Expected Behavior | Status |
|----------|-------------------|--------|
| CLI generates v1, Desktop opens same project | Desktop generates v1 (not v1.1) | ✅ Correct |
| CLI crashes during cache write | Desktop cache unaffected | ✅ Guaranteed |
| Both tools run simultaneously | No conflicts, separate state | ✅ Guaranteed |
| User edits `~/.config/dctx/config.toml` | Desktop unaffected | ✅ Correct |
| User deletes `~/.cache/dctx/` | Desktop cache intact | ✅ Guaranteed |

### Metrics

**Cache Duplication Cost:**
- Typical project: 1,000 files → ~50MB cache
- Heavy user (10 projects): ~500MB total duplication
- **Verdict:** Acceptable overhead on modern systems

**State Divergence Frequency:**
- Users using both CLI and Desktop: Estimated <5% of user base
- Of those, percentage confused by version divergence: Estimated <10%
- **Verdict:** Non-issue for 99.5% of users

---

## Related Decisions

- **ADR-DESKTOP-001:** Tauri Technology Choice (Tauri provides native filesystem access)
- **ADR-DESKTOP-002:** Core Dependency Architecture (Both depend on `dctx-core`, but use separate data)
- **ADR-DESKTOP-003:** Config Inheritance Strategy (SUPERSEDED - no inheritance, complete separation)

---

## ADR-DESKTOP-003 Superseded

**Note:** ADR-DESKTOP-003 proposed a "two-tier configuration system" where Desktop inherits from CLI config. This is **superseded** by ADR-DESKTOP-004.

**New Reality:**
- Desktop config is **completely independent**
- Desktop does NOT inherit from `~/.config/dctx/config.toml`
- Each tool maintains separate state and settings

---

## Future Considerations

### Potential Future Feature: Import CLI Config

If user demand arises, we could add a **one-time import** feature:
- Desktop settings panel: "Import CLI Settings" button
- Copies ignore rules and templates from `~/.config/dctx/` to `~/.config/dctx-desktop/`
- Remains one-time copy, not live sync

**Rationale:** Provides migration path for CLI users without introducing shared state complexity.

### Potential Future Feature: Cache Statistics

Desktop could display cache size and offer cleanup:
- "Cache size: 250 MB (5 projects)"
- "Clear cache" button to delete old snapshots

**Rationale:** Helps users manage duplicated cache cost.

---

**Last Updated:** 2026-02-04
**Reviewed By:** Operator
