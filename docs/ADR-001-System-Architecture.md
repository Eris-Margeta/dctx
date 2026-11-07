<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# ADR-001: System Architecture, Configuration Hierarchy & State Management

**Status:** superseded — historical design and implementation record.

Use the [current product contracts](design/PRODUCT-CONTRACTS.md), [README](../README.md) and [testing guide](TESTING.md) for release behavior.
**Date:** 2026-01-08
**Author:** dctx Team
**Context:** Transformation of `dctx` from a C utility to a multi-platform Rust ecosystem (CLI + Library).

## 1. Context and Problem Statement
We need a unified, deterministic way to manage state, configuration, and output across different runtime environments (System CLI, Embedded Library, CI/CD). The previous "binary in parent folder" approach is insufficient for enterprise usage. We need a strict hierarchy for configuration overrides and a centralized "Source of Truth" for file history and ignore logic (file selection).

## 2. Decision: Centralized "Source of Truth" (Registry & Cache)

To ensure consistency regardless of where `dctx` is invoked from, state must be centralized.

*   **The Registry (`projects.db`):** A lightweight text database mapping absolute paths to Hash IDs.
    *   *Location:* `~/.config/dctx/projects.db` (Linux/macOS) or `%APPDATA%\dctx\projects.db` (Windows).
    *   *Format:* `HASH_ID | /absolute/path/to/target_dir`
*   **The Cache (Binary Store):** All `.dctx` binary files are stored in a dedicated system directory.
    *   *Location:* `~/.cache/dctx/HASH_ID.dctx` (Linux) or `%LOCALAPPDATA%\dctx\` (Windows).
    *   *Role:* This binary file is the **only source of truth** for versioning and diffing.
*   **Edge Case Handling (Recreated Folders):**
    *   If a target folder is deleted and recreated at the same path, the Registry will still point to the old binary.
    *   *Resolution:* The system must compare the target folder's creation/modification time against the binary's last snapshot timestamp. If the folder appears "younger" than the history, or if the structural mismatch is total, `dctx` acts intelligently (defaults to a fresh V1 snapshot or prompts the user in interactive mode).

## 3. Decision: Configuration Hierarchy & Merging Strategy

Configuration follows a strict 3-Layer Override System. Rules are **extended/merged**, not just replaced.

### Layer 1: Compiled-in Defaults (Hardcoded)
*   **Source:** Defined in code (sourced from `.env` or `const` structs during compilation).
*   **Role:** The absolute fallback truth. Cannot be deleted, only overridden.
*   **Content:** Default ignore patterns (`.git`, `node_modules`), default `OUTPUT_TXT_FILES = false`.

### Layer 2: Global User Config (System CLI)
*   **Source:** `~/.config/dctx/config.toml` (Linux/macOS) or `%APPDATA%\dctx\config.toml` (Windows).
*   **Role:** User-specific preferences that apply to ALL projects on their machine.
*   **Behavior:** Overrides Layer 1. If a key is missing here, Layer 1 value is used.

### Layer 3: Local Project Config
*   **Source:** `dctx.config` file located inside the target directory.
*   **Role:** Project-specific settings.
*   **Behavior:** Overrides Layer 2 (and Layer 1).
    *   *Partial Definition:* This file does NOT need to be complete. It only needs to define values that differ from the system defaults.
    *   *Example:* A project can just set `OUTPUT_TXT_FILES = true` and inherit everything else from Global/Hardcoded rules.

### Library Mode (Conflict Resolution)
*   **Context:** When `dctx` is installed as a library (e.g., `npm install dctx`) inside a project that typically uses the System CLI.
*   **Rule:** Library mode operates in **isolation**.
    *   It **IGNORES** Layer 2 (Global User Config) to ensure reproducible builds across different machines.
    *   It relies **ONLY** on Layer 1 (Hardcoded) and Layer 3 (Local Config provided in the package).
    *   *Requirement:* When used as a library, Layer 3 config should ideally be fully defined to avoid reliance on internal defaults that might change between versions.
*   *(Note: Detailed Library Mode Cache behavior is Deferred to ADR-002)*.

## 4. Decision: Output Logic & Paths

We separate the "Internal State" (Binary) from "User Output" (Text/Diffs).

*   **Binary Output:** ALWAYS created/updated in the System Cache (Decision 2).
*   **Text Output Control:** Controlled by `OUTPUT_TXT_FILES` (Boolean).
    *   Default: `false`.

*   **Text Output Location (`TXT_OUTPUT_DIR`):**
    *   If `OUTPUT_TXT_FILES = true`, the system looks for `TXT_OUTPUT_DIR` variable.
    *   **Default:** `.dctx/` (Relative path inside target directory).
    *   **Relative Path behavior:** Resolves relative to the target directory.
    *   **Absolute Path behavior:** Uses the exact system path provided (e.g., `/tmp/dctx_dumps`).
        *   *Collision Note:* We rely on the naming convention `TargetName.llmcontext.txt` to avoid collisions in shared folders.

*   **Diff Generation (`CREATE_LLM_TXT_DIFF_FILES`):**
    *   Boolean flag.
    *   If `true`: Generates sub-sequent diff files (e.g., `Project-V1.2-diff.txt`) containing full tree + content of changed files only.
    *   The main `.llmcontext.txt` is ALWAYS updated to the latest full state.

## 5. Decision: CLI vs. Command Behavior

The `dctx` binary acts differently based on arguments to support both Power Users (Scripting) and New Users (UI).

*   **Interactive Mode (TUI):**
    *   *Trigger:* Running `dctx` (no arguments).
    *   *Action:* Launches a Terminal Menu (Select Project, Settings, Help).
*   **Command Mode (Headless):**
    *   *Trigger:* Any argument provided (e.g., `dctx .`, `dctx -c`).
    *   *Action:* Executes immediately and exits.
*   **Flags:**
    *   `-c`: Copy to Clipboard (updates binary state, skips writing txt files to disk).
    *   `-d`: Diff Mode (used with `-c`). Copies ONLY the diff (V_old -> V_new) to clipboard.

## 6. Decision: Git Safety
*   If `dctx` is configured to write to a local folder (e.g., `.dctx/`), it must proactively check for `.gitignore`.
*   If `.gitignore` exists and does not ignore `.dctx/`, the tool should auto-append the ignore rule or warn the user.

## 7. Decision: Ignore Policy & Hierarchy (.dctxignore)

The logic for excluding files follows a strict "Last Match Wins" strategy, similar to `.gitignore`. Rules are loaded in a specific order into a single list. When checking a file, the **last rule in the list that matches the file path determines its fate**.

### Naming Conventions
*   **Ignore File:** Always named `.dctxignore`.
*   **Binary File:** Always named `.dctx` (e.g., `HASH.dctx`).

### Hierarchy Layers (Loaded in Order)

#### Layer 1: Hardcoded Project Rules (The Base)
*   **Source:** Defined internally in `dctxignore_hard_rules` (baked into the binary during compilation).
*   **Nature:** These are the "Strongest Base Rules". They apply universally unless explicitly negated later.
*   **Content:** Essential ignores like `.git/`, `node_modules/`.

#### Layer 2: Global User Ignore
*   **Source:** `~/.config/dctx/ignore` (or Windows equivalent).
*   **Nature:** Additive/Override to Layer 1. Applies to all projects on the user's machine.
*   **Logic:**
    *   Adding `pattern` adds another ignore rule.
    *   Adding `!pattern` negates (overrides) a Layer 1 rule (re-includes the file).

#### Layer 3: Local Target Ignore
*   **Source:** `.dctxignore` located in the root of the target directory.
*   **Nature:** The Final Authority for that specific folder.
*   **Logic:**
    *   Rules here are appended to the end of the list.
    *   They override the net result of (Layer 1 + Layer 2).
    *   *No Double Negation:* `!pattern` in L2 and `!pattern` in L3 just results in `!pattern` (Include). It does not toggle back to Ignore.

### Evaluation Algorithm

```code
State = NotIgnored
List = [Rules_L1, Rules_L2, Rules_L3]

For each Rule in List:
    If Rule matches File:
        If Rule is Negative (!): State = NotIgnored
        Else:                    State = Ignored

Return State
```


### Supported Syntax
The `.dctxignore` parser supports a subset of the standard gitignore syntax, implemented without external dependencies:

*   **Standard Match:** `filename.txt` (Matches file/folder by name anywhere).
*   **Path Match:** `path/to/file` (Matches specific relative path).
*   **Wildcards:**
    *   `?`: Matches any single character (except `/`).
    *   `*`: Matches any string of characters (except `/`).
*   **Globstar:** `**` (Matches directories recursively, e.g., `foo/**/bar`).
*   **Directory Only:** `build/` (Matches only directories named `build`).
*   **Negation:** `!important.log` (Un-ignores a file, overriding previous rules).
*   **Comments:** Lines starting with `#` are ignored.
