<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# ADR-011: Advanced CLI Filters

**Date:** 2026-01-25
**Status:** superseded — historical design and implementation record.

Use the [current product contracts](design/PRODUCT-CONTRACTS.md), [README](../README.md) and [testing guide](TESTING.md) for release behavior.

---

## 1. Context and Problem Statement

Users needed fine-grained control over what files appear in the context output. Without filtering capabilities, users would:
- Receive massive context files including irrelevant file types
- Waste tokens on binary files, large files, or files they don't need
- Lack visibility into what folders were being ignored
- Have no way to focus on specific file types or sizes

Additionally, with binary file detection added in v0.10.0, binary files were being included in the output by default, which could bloat context files unnecessarily.

## 2. Decision Drivers

* **User Control**: Give users precise control over output content
* **Context Selection**: Exclude irrelevant files from tool output
* **Flexibility**: Support multiple filter types that can be combined
* **Usability**: Simple, intuitive flag-based interface
* **Performance**: Filtering should be fast and not impact overall performance
* **Binary Handling**: Change default behavior to exclude binary files (opt-in with `--binary`)
* **Debugging**: Provide visibility into what's being filtered/ignored

## 3. Considered Options

### For Filter Implementation Approach:

#### Option 1: Pre-Walk Filtering (Selected)
**Pros:**
* Simple implementation - filter during tree walk
* Lower memory usage - never loads filtered files

**Cons:**
* Harder to implement ignored folder collection
* Less flexible for future features

#### Option 2: Post-Walk Filtering (Selected - Final Choice)
**Pros:**
* Binary state contains full tree (filters don't affect state)
* Easy to apply different filters to same state
* Can collect debug information during walk
* More flexible for future features (e.g., saved filter presets)

**Cons:**
* Higher memory usage - loads all files then filters
* Requires cloning tree

**Decision:** Option 2 chosen. Binary state represents the TRUTH of the directory, filters create VIEWS.

### For Binary File Default Behavior:

#### Option 1: Include Binary by Default (Previous Behavior)
**Pros:**
* Shows everything
* No surprises

**Cons:**
* Bloats output with base64-encoded binary content
* Wastes tokens on useless data

#### Option 2: Exclude Binary by Default (Selected)
**Pros:**
* Cleaner output
* Better token efficiency
* Users can opt-in with `--binary` when needed

**Cons:**
* Might surprise users expecting to see all files
* Requires documentation

**Decision:** Option 2 - exclude by default, opt-in with `--binary`

## 4. Decision Outcome

**Chosen approach:** Post-walk filtering with binary exclusion by default

### Filter Types Implemented:

1. **Extension Filter** (`--ext <extensions>`)
   - Comma-separated list of extensions
   - Case-insensitive matching
   - Example: `--ext rs,toml`

2. **Binary Filter** (`--binary`)
   - Include binary files in output
   - Default: binary files excluded

3. **Content Filters**
   - `--empty`: Show only 0-byte files
   - `--non-empty`: Show only files with content
   - Mutually exclusive

4. **Size Filters**
   - `--large <MB>`: Files larger than X megabytes
   - `--small <MB>`: Files smaller than X megabytes
   - Can be combined for range filtering

5. **Hidden Filter** (`--hidden`)
   - Show ONLY hidden files/folders (starting with `.`)
   - Applies to both files and directories

6. **Debug Filter** (`--debug-ignored`)
   - List ignored folders during walk
   - Helps users understand why directories don't appear

### Implementation Details:

**Module Structure:**
```
crates/dctx-core/src/filter.rs
- FilterConfig struct (builder pattern)
- should_include_file() - file-level filtering
- should_include_directory() - directory-level filtering
- has_active_filters() - optimization check

crates/dctx-core/src/walker.rs
- filter_tree() - applies filters post-walk
- walk_directory_structure_with_debug() - collects ignored folders
```

**Filter Application Order:**
1. Hidden filter (directory and file level)
2. Extension filter
3. Binary filter
4. Empty/Non-empty filter
5. Size filters (min and max)

**Tree Pruning:**
- After filtering, empty directory branches are automatically removed
- Root directory is NEVER filtered out (even with `--hidden`)
- Only directories with at least one matching child are kept

**CLI Integration:**
```rust
struct SnapshotConfig {
    // ... existing fields ...
    filter_extensions: Vec<String>,
    include_binary: bool,
    only_empty: bool,
    only_non_empty: bool,
    min_size_mb: Option<f64>,
    max_size_mb: Option<f64>,
    only_hidden: bool,
    debug_ignored: bool,
}
```

## 5. Positive Consequences

* ✅ **Precise Control**: Users can filter exactly what they need
* ✅ **Bounded Context**: Filtering limits selected output; whole-session savings belong to a system that reuses stable context, not to dctx by default
* ✅ **Flexible Combinations**: All filters work together logically
* ✅ **State Preservation**: Binary state always contains full tree
* ✅ **Performance**: Filtering is fast (tree cloning + filtering < 1ms)
* ✅ **Debugging**: `--debug-ignored` provides visibility into ignore rules
* ✅ **Binary Default**: Excludes binary files by default (better UX)
* ✅ **Tree Pruning**: Empty directories automatically removed
* ✅ **Test Coverage**: 19 comprehensive tests (254 total, all passing)

## 6. Negative Consequences

* ⚠️ **Memory Usage**: Post-walk filtering requires tree cloning
* ⚠️ **Breaking Change**: Binary files now excluded by default (mitigated with `--binary`)
* ⚠️ **Complexity**: More CLI flags to document and support

## 7. Validation

### Test Coverage

**19 new tests in `filter.rs`:**
- Extension filtering (single, multiple, case-insensitive, no extension)
- Binary filtering (default exclusion, explicit inclusion)
- Content filtering (empty, non-empty)
- Size filtering (large, small, range)
- Hidden filtering (files, directories)
- Combined filters (extension+size, extension+binary, hidden+extension)
- Helper functions (`is_hidden`, `get_extension`)

**All tests passing:** 254/254 (100% pass rate)

### Manual Testing

**Test Environment:** Created `/tmp/dctx-filter-test` with:
- Empty files (0 bytes)
- Small files (< 10 bytes)
- Medium files (10-50 bytes)
- Large files (> 50 bytes)
- Hidden files (`.hidden`)
- Binary files (with null bytes)
- Nested directories

**Verified Behaviors:**
- `--ext txt` shows only .txt files
- `--empty` shows only 0-byte files
- `--non-empty` excludes empty files
- `--small 0.00001` shows files ≤ 10 bytes
- `--large 0.00005` shows files ≥ 50 bytes
- `--hidden` shows only files starting with `.`
- `--debug-ignored` lists ignored folders
- `--ext txt --small 0.00001` combines correctly
- Tree pruning removes empty directories
- Root directory never filtered out

### Performance Impact

**Filtering overhead:** < 1ms for typical projects
**Memory overhead:** ~2x tree size during filtering (cloned tree)
**Trade-off:** Acceptable for flexibility and state preservation

## 8. Implementation Notes

**Files Modified:**
- `crates/dctx-core/src/filter.rs` - NEW (389 lines, 19 tests)
- `crates/dctx-core/src/walker.rs` - Added `filter_tree()` and debug support
- `crates/dctx-core/src/lib.rs` - Exported `FilterConfig`
- `crates/dctx-cli/src/main.rs` - Extended CLI parsing and config

**Breaking Changes:**
- Binary files now excluded by default (use `--binary` to include)

**Filter Precedence:**
When multiple filters are active, ALL must pass for a file to be included (AND logic).

**Example Usage:**
```bash
# Only Rust files
dctx . --ext rs

# Rust and TOML files under 500KB
dctx . --ext rs,toml --small 0.5

# Hidden configuration files
dctx . --hidden --ext conf,yaml,toml

# Debug why folders are missing
dctx . --debug-ignored

# Include binary files
dctx . --binary

# Only empty files
dctx . --empty

# Files between 10KB and 100KB
dctx . --large 0.01 --small 0.1
```

**Future Enhancements:**
- Saved filter presets (e.g., `--filter-preset rust-only`)
- Regex pattern matching for file names
- Date-based filtering (modified in last N days)
- Inverse filters (`--exclude-ext` to exclude extensions)
- Filter profiles in config file

---

## 9. Related Documents

- ADR-007: Binary File Detection
- Phase 9 Documentation (PROJECT-DEVELOPMENT-ELABORATION.md)
- CHANGELOG.md (v0.11.0 features)

---

**Decision Date:** January 25, 2026
**Implemented By:** Phase 9 - Advanced CLI Filters
**Test Coverage:** 19 tests (100% pass rate)
**Status:** ✅ Accepted and Implemented
