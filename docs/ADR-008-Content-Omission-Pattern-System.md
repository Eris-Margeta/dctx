<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# ADR-008: Content Omission Pattern System

**Date:** `2026-01-25`
**Status:** current — architectural principle retained; historical examples describe their original release.

For shipped behavior and limitations, see the [README](../README.md), [testing guide](TESTING.md) and [specification](SPEC.md).

---

## 1. Context and Problem Statement

The dctx tool generates context files for LLMs by walking directory trees and including file contents. However, certain file types—while useful to show in the directory structure—have content that is either:

1. **Verbose and auto-generated**: Package manager lockfiles (10,000+ lines) that pollute context
2. **Unreadable in raw form**: SVG files with complex XML markup
3. **Not useful for AI analysis**: Binary-adjacent files that add noise

Previously, SVG omission was handled via a single boolean config option (`omit_svg_content`), which was inflexible and couldn't be extended to other file types. There was no mechanism to omit lockfiles or apply project-specific content filtering.

The system needed a flexible, hierarchical pattern-based approach similar to `.dctxignore`, but for content omission instead of file exclusion.

## 2. Decision Drivers

* **Flexibility**: Users should be able to define custom content omission patterns per project or globally
* **Consistency**: Should use the same pattern syntax and hierarchy as `.dctxignore` for familiarity
* **Extensibility**: Easy to add new hardcoded rules or override existing ones
* **Backward Compatibility**: Existing `omit_svg_content` behavior must be preserved by default
* **Performance**: No significant overhead compared to file exclusion
* **Cleaner Context**: Automatic omission of common noisy files (lockfiles) to keep selected output focused; this does not establish whole-session token savings

## 3. Considered Options

### Option 1: Extend Boolean Config Options
Add individual boolean flags for each file type: `omit_lockfiles`, `omit_snapshots`, etc.

*   **Pros:**
    *   Simple to implement
    *   Explicit control per file type
*   **Cons:**
    *   Not scalable (need new config for every file type)
    *   No pattern matching flexibility
    *   No project-specific overrides
    *   Config file becomes cluttered

### Option 2: Pattern-Based System with Hierarchical Loading
Create `.dctxomitcontent` files with gitignore-style patterns, loaded hierarchically (hardcoded → global → project).

*   **Pros:**
    *   Highly flexible and extensible
    *   Consistent with existing `.dctxignore` system
    *   Supports pattern matching, negation, and glob syntax
    *   Project-specific and global customization
    *   Can override hardcoded rules with `!` negation
    *   Single file can handle all omission rules
*   **Cons:**
    *   Slightly more complex implementation
    *   Users need to learn pattern syntax (mitigated by familiarity with `.gitignore`)

### Option 3: File Type Categories
Define categories in config (e.g., "lockfiles", "assets") that map to patterns.

*   **Pros:**
    *   Semantic grouping of file types
    *   Easy to enable/disable entire categories
*   **Cons:**
    *   Less flexible than raw patterns
    *   Still requires pattern matching under the hood
    *   Category naming becomes a UX challenge

## 4. Decision Outcome

**Chosen option:** Pattern-Based System with Hierarchical Loading (Option 2)

This option provides the best balance of flexibility, consistency, and extensibility. By reusing the existing `IgnoreRule` parser from `ignore.rs`, the implementation complexity is minimized while gaining full pattern matching capabilities.

### Hierarchical Loading Order
1. **Hardcoded rules** (built into dctx binary)
2. **Global rules** (`~/.config/dctx/omitcontent`)
3. **Project rules** (`.dctxomitcontent` in project root)

Last matching rule wins, with `!` prefix for negation to override earlier rules.

### Hardcoded Omission Rules
The following patterns are compiled into dctx by default:

**SVG Files:**
```
*.svg
```
Replaces the deprecated `omit_svg_content` boolean config option.

**Package Manager Lockfiles:**
```
package-lock.json   # npm
pnpm-lock.yaml      # pnpm
yarn.lock           # Yarn
Cargo.lock          # Rust
go.sum              # Go
Gemfile.lock        # Ruby
poetry.lock         # Python Poetry
Pipfile.lock        # Python Pipenv
```

These files are auto-generated, thousands of lines long, and not useful for LLM context (manifest files like `package.json`, `Cargo.toml` remain included).

### Positive Consequences
*   Cleaner context files by default (lockfiles omitted automatically)
*   Flexible per-project customization via `.dctxomitcontent`
*   Users can override hardcoded rules with global `~/.config/dctx/omitcontent`
*   Consistent pattern syntax with `.dctxignore` (reduced learning curve)
*   Extensible: new hardcoded rules can be added without breaking changes
*   Deprecates `omit_svg_content` config in favor of pattern-based approach

### Negative Consequences
*   Users upgrading from v0.10.0 who used `omit_svg_content = false` must create global `omitcontent` file with `!*.svg`
*   Deprecation warning shown for `omit_svg_content` config option (minor UX friction during migration)
*   Slightly increased complexity in walker logic (mitigated by reusing existing pattern parser)

## 5. Implementation Plan

1. **Create new module**: `crates/dctx-core/src/omit_content.rs`
   - Implement `load_omit_content_rules()` using `IgnoreRule` parser
   - Define `HARDCODED_OMIT_CONTENT_RULES` constant

2. **Update config.rs**:
   - Add `DEFAULT_OMIT_CONTENT_FILENAME = ".dctxomitcontent"`
   - Add `GLOBAL_OMIT_CONTENT_FILENAME = "omitcontent"`
   - Add `get_global_omit_content_path()` helper
   - Deprecate `omit_svg_content` with warning message

3. **Update datatypes.rs**:
   - Add `is_content_omitted: bool` field to `DirContextTreeNode`

4. **Update walker.rs**:
   - Load omit rules via `load_omit_content_rules()`
   - After loading file content, check against omit patterns
   - Set `is_content_omitted` flag if matched

5. **Update llm_formatter.rs**:
   - Remove `app_config` dependency
   - Check `is_content_omitted` flag instead of config option
   - Output: `[Content omitted by .dctxomitcontent pattern]`

6. **Update file_classifier.rs**:
   - Remove SVG special handling (`FileContentHandling::OmitSvgContent`)
   - SVGs now treated as normal text files (omission handled by patterns)

7. **Add tests**:
   - Verify hardcoded rules loaded correctly
   - Test SVG pattern matching
   - Test lockfile pattern matching
   - Test negation overrides

## 6. Validation

Success criteria:
*   All 237 tests pass (including 4 new tests in `omit_content.rs`)
*   Output files show `[Content omitted by .dctxomitcontent pattern]` for matched files
*   Files still appear in directory tree with SIZE metadata
*   `!*.svg` in global `omitcontent` file successfully includes SVG content
*   Lockfiles omitted by default in all new context generations
*   Deprecation warning shown when `omit_svg_content` detected in config

**Metrics:**
- Test coverage: 4 new unit tests for pattern matching
- Context file size reduction: ~30-50% for projects with lockfiles
- Migration path: 100% backward compatible (default behavior unchanged)

---
