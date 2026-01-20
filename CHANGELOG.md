# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.2.0] - 2026-01-20

### Added
- **Unit Tests for git_safety.rs**: 20 new tests covering:
  - Pattern validation and marker format tests
  - `read_existing_patterns` tests (empty file, patterns, comments, whitespace)
  - `is_fast_path_valid` tests (with/without marker, old versions)
  - `update_gitignore` tests (adding patterns, preserving content)
  - `ensure_local_gitignore_is_safe` integration tests
- **Unit Tests for state.rs**: 14 new tests covering:
  - `load_or_reset_state` with valid, corrupted, and missing binaries
  - `is_folder_recreated` stub behavior
  - `perform_full_cleanup` file deletion and preservation logic
  - `StateLoadResult` enum tests
- **Total test count**: 219 tests (up from 185)

### Changed
- **Website Updates**:
  - Installation tabs now show working methods (curl, cargo, source) instead of unavailable package managers
  - GitHub URLs updated for consistency across all pages
  - Copyright year updated to 2026
- **Code Quality**:
  - Added `Default` impl for `IgnoreRule` (Clippy lint fix)
  - Refactored collapsible `if` statements in `ignore.rs`, `state.rs`, `template.rs`
  - CLI code cleanup in `main.rs` (struct initialization, collapsed conditions)

### Fixed
- All Clippy warnings resolved (0 warnings)
- Website GitHub URL inconsistency fixed

---

## [0.1.6] - 2026-01-16

### Added
- **Performance Testing Suite**: New `tools/performance-test.sh` script for benchmarking dctx against bash commands
- **Performance Results**: `tools/performance-results.md` documenting 13.1x speed improvement
- **Unit Tests**: 4 new tests for file IDs beyond 3 digits (185 total tests)
- **Landing Page Enhancements**:
  - Installation tabs for multiple package managers (brew, apt, choco, npm, pip, cargo, curl)
  - Problem/solution comparison section (bash nightmare vs dctx simplicity)
  - Interactive example output tabs
  - Core benefits section highlighting token savings and workflow efficiency
  - Ecosystem section showcasing CLI, libraries, and upcoming desktop app
- **Checkpoint Protocol**: New `PROJECT-INTERNAL/GUIDES/CHECKPOINT-PROTOCOL.md` for development milestones
- **Checkpoint Template**: `PROJECT-INTERNAL/CHECKPOINTS/checkpoint-template.md`

### Changed
- **Output Format**: Removed curly braces from DIRECTORY header (`[DIRECTORY:{name}]` → `[DIRECTORY:name]`)
- **Output Format**: Removed SIZE attribute from `<FILE_CONTENT_START>` tags (SIZE now only in directory tree)
- **README.md**: Completely rewritten with accurate feature documentation, CLI reference, and performance metrics

### Fixed
- `{{{project}}}` triple-brace template variable now correctly replaced in `render_layout()`
- ANSI escape codes no longer leak into generated markdown files

---

## [0.1.5] - 2026-01-15

### Added
- **AOT Templating Engine** (Phase 6 complete):
  - Template validation (`template.rs::Template::validate()`)
  - Template compilation (`template.rs::Template::compile()`)
  - Binary serialization with magic bytes (`DCTXTMPL`)
  - CLI commands: `dctx template build <path>` and `dctx template list`
  - Runtime loading for custom templates
- **Comprehensive Unit Test Suite**: 181 tests across all core modules
- **Landing Page**: Initial `website/index.html` for dctx.app

### Changed
- Default template: Changed `{{project}}` to `{{{project}}}` (runtime data, not config variable)

---

## [0.1.4] - 2026-01-14

### Added
- **Tree-Only Mode**: `-t` / `--tree` flag to output directory structure without file contents
- **Diff Toggle Config**: `text_output_mode_generates_diff_files` setting
- **Integrity Test Suite**: `tools/test-integrity.sh` with Triple-Run Protocol

### Changed
- Renamed `output_mode` (enum) to `text_output_enabled` (bool) for clarity
- Binary state always written to system cache; config only controls `.txt` output

---

## [0.1.3] - 2026-01-13

### Added
- **Diff File Generation**: `llm_formatter::generate_diff_context`
- **Major Bump Logic**: Automatic V1 → V2 on massive changes
- **Diff Cleanup**: Auto-deletion of old `*-diff.txt` files on major bumps
- **Template Refinement**: `\n` escape support in templates

---

## [0.1.2] - 2026-01-12

### Added
- **Hash Registry**: FNV-1a path hashing in `registry.rs`
- **Centralized Cache**: Binary state stored in `~/.cache/dctx/<HASH>.dctx`
- **Git Safety**: `git_safety.rs` with fast-path version marker checks
- **Version Sync**: `tools/sync-version.sh` for VERSION file propagation

### Changed
- Output redirected to local `.dctx/` directory
- Global gitignore automatically configured

---

## [0.1.1] - 2026-01-11

### Added
- **Semantic IDs**: `ROOT`, `Dxxx`, `Fxxx` in output
- **Visual Indentation**: Directory tree now shows hierarchy
- **Version Header**: `[DCTX_LLM_CONTEXT_SNAPSHOT_V...]` format

---

## [0.1.0] - 2026-01-10

### Added
- Initial release
- Zero-dependency CLI argument parser
- Multi-threaded file walker
- `.dctx` binary state format
- Clipboard integration (`-c` flag)
- Reset flag (`-r`)
- Basic ignore system (`.gitignore` + `.dctxignore`)
- Custom TOML parser (zero-dep)
- Cross-platform support (macOS, Linux, Windows paths)
