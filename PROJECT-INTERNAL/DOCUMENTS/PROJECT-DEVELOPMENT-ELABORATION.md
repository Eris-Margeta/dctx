# PROJECT-DEVELOPMENT-ELABORATION

---

FACTORY OF ELECTRONIC UNITS AND LOGIC – TEJL d.o.o.
Sector for System Integration and Development

**Class:** 003-05/25-01
**Reference Number:** 251-01-03-25-04 (Rev. 8)
**Date:** 2026-01-12

SUBJECT: Elaboration on the Development of the 'dctx' Project

---

## DOCUMENT PURPOSES, OBJECTIVES, AND STRUCTURE

### **1.0 INTRODUCTION AND RATIONALE**

`dctx` (Directory Context) is a blazing fast, zero-dependency command-line tool designed to transform a codebase into a structured, version-aware context snapshot for Large Language Models (LLMs). Unlike traditional RAG (Retrieval Augmented Generation) which fragments code, `dctx` provides a holistic, hierarchical view of the project state, enabling AI models to understand architecture and file relationships instantly. It uses a proprietary binary state engine to generate optimized diffs, saving up to 90% of tokens on subsequent runs.

### **2.0 PROJECT OBJECTIVES**

* **Objective 1:** **Zero-Dependency Core:** Build a high-performance Rust core that runs on any architecture without external runtime dependencies (no Python, Node, or heavy crates).
* **Objective 2:** **Centralized "Source of Truth":** Implement a deterministic state management system (ADR-001) where project history is stored in a central system cache, keeping the user's workspace clean.
* **Objective 3:** **Intelligent Diffing & Templating:** Provide an AOT-compiled template system (ADR-003) and smart versioning logic (ADR-002) to generate precise, token-efficient context for any AI model.

### **3.0 TECHNICAL SPECIFICATION AND STANDARDS**

#### **3.1 System Architecture**

The system follows a Monorepo structure with a clear separation of concerns:
*   **dctx-core:** Pure Rust library containing the "Brain" (Walker, Diff Engine, State Manager, Template Renderer). Strictly uses `std` library.
*   **dctx-cli:** The "Interface". Handles argument parsing, TUI, and platform integration (Clipboard).
*   **Centralized State:** Uses a Hash Registry (`projects.db`) to map project paths to binary state files in `~/.cache/dctx/`.

#### **3.2 Technologies**

*   **Language:** Rust (2021 Edition)
*   **Dependencies:** Standard Library (`std`) ONLY for Core/CLI.
*   **Build System:** Cargo
*   **State Format:** Custom Binary (`.dctx`) with Little Endian serialization.
*   **Config Format:** TOML (Custom zero-dep parser).

#### **3.3 Cross-Platform Execution Management**

The `tools/install.sh` script manages compilation and installation on POSIX systems (Linux/macOS). Windows support is handled via `cfg!(target_os = "windows")` compilation flags within the Rust code, specifically in `platform.rs` and `config.rs`.

### **4.0 IMPLEMENTATION STATUS OVERVIEW**

*   **Phase 0: System Integration:** `COMPLETED`
*   **Phase 1: The Interface Layer:** `COMPLETED`
*   **Phase 2: The Core Logic Loop (Base):** `COMPLETED`
*   **Phase 3: Architectural Realignment (ADR-001/005/006):** `COMPLETED`
*   **Phase 4: Output Parity & Diffing (ADR-002):** `COMPLETED`
*   **Phase 5: Critical Fixes & Logic Verification:** `COMPLETED`
*   **Phase 6: The AOT Templating Engine (ADR-003):** `COMPLETED`
*   **Phase 7: Final Polish:** `PENDING`

---

## PROJECT PHASES; COMPLETED AND PLANNED

### **Phase 0: System Integration (COMPLETED)**

*   **0.1. Installer Script:**
    *   `0.1.a.` Created `tools/install.sh` to build release binaries.
    *   `0.1.b.` Implemented system-wide installation to `/usr/local/bin`.
*   **0.2. Default Configuration:**
    *   `0.2.a.` Automated creation of `~/.config/dctx/` directory structure.
    *   `0.2.b.` Deployed default `config.toml`, global `ignore`, and templates.

### **Phase 1: The Interface Layer (COMPLETED)**

*   **1.1. Zero-Dependency CLI:**
    *   `1.1.a.` Implemented custom argument parser using `std::env::args` (ADR-004).
    *   `1.1.b.` Implemented flags: `-c` (Clipboard), `-d` (Diff), `-r` (Reset), `-h`, `-v`.
*   **1.2. Mode Switching & Platform Integration:**
    *   `1.2.a.` Implemented "Headless Mode" vs "Interactive TUI".
    *   `1.2.b.` Created `platform.rs` for OS-agnostic clipboard piping (pbcopy/xclip/clip.exe).

### **Phase 2: The Core Logic Loop - Base (COMPLETED)**

*   **2.1. Core Modules:**
    *   `2.1.a.` Implemented zero-dep `config` loader.
    *   `2.1.b.` Implemented multi-threaded `walker` and `ignore` logic.
*   **2.2. State & Versioning:**
    *   `2.2.a.` Implemented `.dctx` binary writer/reader.
    *   `2.2.b.` Fixed critical bug by embedding Version Header into binary for stateless clipboard support.
    *   `2.2.c.` Connected the "Brain" in `main.rs`: Load -> Walk -> Diff -> Version -> Write.

### **Phase 3: Architectural Realignment (COMPLETED)**

*   **3.1. The Hash Registry (`projects.db`)**
    *   `3.1.a.` Implemented stable FNV-1a path hashing (Zero-Dep).
    *   `3.1.b.` Implemented `registry.rs` to map paths to Hash IDs.
*   **3.2. Centralized Cache Implementation**
    *   `3.2.a.` Redirected binary state I/O to `~/.cache/dctx/<HASH>.dctx`.
    *   `3.2.b.` Ensured cache directory creation and management.
*   **3.3. Output Redirection & Git Safety (ADR-005)**
    *   `3.3.a.` Redirected text output to local `.dctx/` directory.
    *   `3.3.b.` Implemented robust `git_safety.rs` with "Fast-Path" version marker checks.
    *   `3.3.c.` Updated `install.sh` for Global Gitignore and Version-Aware Config updates.
*   **3.4. Unified Versioning Strategy (ADR-006)**
    *   `3.4.a.` Established `VERSION` file as the single source of truth.
    *   `3.4.b.` Created `tools/sync-version.sh` to propagate versions to `Cargo.toml` and metadata.

### **Phase 4: Output Parity, Diffing & Hard Resets (ADR-002) (COMPLETED)**

*   **4.1. Aesthetic Restoration (Parity with Legacy C)**
    *   `4.1.a.` **Header:** Implemented `[DCTX_LLM_CONTEXT_SNAPSHOT_V...]` header.
    *   `4.1.b.` **IDs:** Implemented Semantic IDs (`ROOT`, `Dxxx`, `Fxxx`) in `llm_formatter.rs`.
    *   `4.1.c.` **Indentation:** Implemented visual indentation in the Directory Tree.
*   **4.2. Diff File Generation**
    *   `4.2.a.` Implemented `llm_formatter::generate_diff_context`.
    *   `4.2.b.` Logic: Header -> Summary -> Full Tree -> Partial Content.
*   **4.3. Major Bump Logic (ADR-002)**
    *   `4.3.a.` Implemented "Massive Change" trigger (V1 -> V2).
    *   `4.3.b.` Implemented "Diff Limit" trigger.
    *   `4.3.c.` **Cleanup:** Implemented auto-deletion of old `*-diff.txt` files on major bumps.
*   **4.4. Template Refinement**
    *   `4.4.a.` Implemented robust template parsing with `\n` escape support.
    *   `4.4.b.` Moved `[DIRECTORY:...]` branding into the `default.dctx_template` layout for user control.

---

### **Phase 5: Critical Fixes & Logic Verification (COMPLETED)**

**Goal:** Ensure the system is actually working as intended before adding complex features. Specifically, we must verify the "Single Source of Truth" (the binary) and fix broken output logic.

*   **5.1. Output Logic Remediation** ✓
    *   **Problem:** Legacy `output_mode` config was confusing since binary is now always written to system cache.
    *   **Solution:** Renamed `output_mode` (enum) to `text_output_enabled` (bool). Binary state is ALWAYS written; this config now only controls whether `.txt` files are written to `.dctx/`.
    *   **Backward Compatibility:** Parser still accepts legacy `output_mode = "binary"/"text"/"both"` values.
*   **5.2. New Configuration Control: Diff Toggle** ✓
    *   **Implemented:** `text_output_mode_generates_diff_files` (bool, default `false`) in `AppConfig`.
    *   **Logic:** If `false`, `dctx` never writes `*-diff.txt` to disk. Only updates main `.llmcontext.txt`.
*   **5.3. New Feature: Tree-Only Mode (`-t`)** ✓
    *   **Implemented:** `-t` / `--tree` flag in CLI.
    *   **Logic:** When active, formatter skips `<FILE_CONTENT_START>` blocks. Outputs only Header, Instructions, and Directory Tree.
*   **5.4. Single Source of Truth Verification** ✓
    *   **Implemented:** `tools/test-integrity.sh` - Triple-Run Protocol (ADR-007) verifying:
        1.  Fresh Start (V1) - binary creation in `~/.cache/dctx/`
        2.  Incremental Update (V1.1) - version bumping
        3.  Config propagation - `text_output_mode_generates_diff_files`
        4.  Massive Change (V2) - major reset and diff cleanup
    *   **Status:** All 5 test suites PASS.

---

### **Phase 6: The AOT Templating Engine (ADR-003) (COMPLETED)**

**Goal:** Implement the "Ahead-of-Time" compiled template system for custom templates. **Note:** The "Default" template is hardcoded (compiled into the binary) and does *not* use this workflow. This phase is exclusively for enabling users to build their own `.dctx_template` files.

*   **6.1. Template Compilation Logic (The "Build" Command)** ✓
    *   **Context:** Parsing text templates at runtime is slow. We want to "pre-compile" them into an optimized binary format or valid struct representation when the user runs `dctx template build`.
    *   `6.1.a.` **Syntax Validation:** ✓ Implemented in `template.rs::Template::validate()`. Checks for required `[LAYOUT]` section and validates that all `{{VAR}}` references exist in `[VARIABLES]`. Distinguishes between `{{VAR}}` (config) and `{{{DATA}}}` (runtime).
    *   `6.1.b.` **Pass 1 Resolution (Pre-computation):** ✓ Implemented in `template.rs::Template::compile()`. Resolves all static `{{VAR}}` references during build. Compiled templates only contain `{{{data}}}` placeholders.
    *   `6.1.c.` **Serialization:** ✓ Implemented zero-dependency binary format with magic bytes (`DCTXTMPL`), version header, and structured field storage. Saved to `~/.cache/dctx/templates/<name>.bin`.
*   **6.2. CLI Integration** ✓
    *   `6.2.a.` ✓ Implemented `dctx template build <path>` and `dctx template list` subcommands.
    *   `6.2.b.` ✓ Success message includes config instructions with path to `config.toml`.
*   **6.3. Runtime Loading (Custom Path)** ✓
    *   `6.3.a.` ✓ Updated `llm_formatter.rs` with `load_template()` function and `generate_llm_context_with_template()`.
    *   `6.3.b.` **Logic:** ✓
        *   If `default`, uses hardcoded `include_str!`.
        *   If other, loads from `~/.cache/dctx/templates/<name>.bin` via `CompiledTemplate::load_from_file()`.
    *   `6.3.c.` **Pass 2 (Data Injection):** ✓ Works identically for both default and custom templates.

**Implementation Notes:**
*   Added 28 tests in `template.rs` covering validation, compilation, and serialization roundtrips.
*   Updated default template: Changed `{{project}}` to `{{{project}}}` (runtime data, not config variable).
*   All 181 unit tests pass. All 5 integration test suites pass.

---

### **Phase 7: Final Polish (PENDING)**

*   **7.1. Cross-Platform Verification**
    *   `7.1.a.` Verify behavior on Windows (Paths, AppData).
    *   `7.1.b.` Verify behavior on Linux/macOS.
*   **7.2. Documentation & Cleanup**
    *   `7.2.a.` Run `cargo clippy` and resolve all lints.
    *   `7.2.b.` Finalize `README.md` with updated usage instructions.

---

**Compiled by:**
AI Assistant, System Integration Sector

**Approved by:**
Operator, Board for Standardization and Development
