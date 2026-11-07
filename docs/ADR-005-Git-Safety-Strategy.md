<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# ADR-005: Version-Aware Git Safety & Configuration Strategy

**Date:** 2026-01-12
**Status:** superseded — historical design and implementation record.

Use the [current product contracts](design/PRODUCT-CONTRACTS.md), [README](../README.md) and [testing guide](TESTING.md) for release behavior.
**Author:** dctx Team
**Context:** `dctx` generates large context files (`.llmcontext.txt`), diffs, and binary state files. While binary files are centralized (ADR-001), text output remains in the project directory. Users frequently forget to ignore these files, leading to repository pollution. Furthermore, as the tool evolves (e.g., updating from v0.1.0 to v0.1.2), new configuration options and ignore rules must be propagated to the user's system without overwriting their custom settings or degrading runtime performance.

## 1. Context and Problem Statement
*   **Pollution:** Generated files (`*.llmcontext.txt`, `.dctx/`) must never be committed.
*   **Stale Configs:** When upgrading `dctx`, existing `config.toml` files lack new keys, causing the binary to rely on hardcoded defaults rather than user intent.
*   **Stale Ignore Rules:** Updates might introduce new file patterns that old `.gitignore` files don't cover.
*   **Performance:** Scanning and parsing `.gitignore` on every execution to check for rules is inefficient. We need a "Fast Path" to skip this check when the environment is already safe.

## 2. Decision Drivers
*   **"Safe by Default":** The tool must prevent accidents without user intervention.
*   **Non-Destructive Updates:** User customizations in `config.toml` must be preserved during upgrades.
*   **Performance:** Runtime checks must be negligible (0ms).
*   **Idempotency:** Repeated installations must not duplicate entries.

## 3. Decision: The Versioning Authority
The `VERSION` file in the project root (and the `CARGO_PKG_VERSION` in the binary) is the single source of truth.
*   **Installer:** Uses the `VERSION` file content to tag configuration blocks.
*   **Runtime:** The binary uses its compiled version to validate markers in local files.

## 4. Decision: Defined Patterns & Markers
We define a strict list of files that **MUST** be ignored in all environments.

### The Patterns
1.  `.dctx` (Legacy/Backup binary files).
2.  `.dctx/` (The output directory defined in ADR-001 Phase 3.3).
3.  `*.llmcontext.txt` (Main context file).
4.  `*.llmcontext-*-diff.txt` (Specific diff naming convention).

### The Global Marker (Installation)
Used in `~/.config/git/ignore` to delineate our managed block.
```gitignore
# >>> dctx-ignore-vX.Y.Z >>>
... rules ...
# <<< dctx-ignore-vX.Y.Z <<<
```

### The Local Marker (Runtime)
Used in project `.gitignore` for high-speed checks.
```gitignore
# [dctx vX.Y.Z]
```

## 5. Decision: Global Git Safety (Installation Time)

We implement a **Versioned Block Replacement** strategy for the Global Gitignore file.

*   **Trigger:** Execution of `tools/install.sh`.
*   **Targets:**
    1.  The Current Repo `.gitignore` (where the installer is running).
    2.  The System Global Gitignore (`~/.config/git/ignore`).
*   **Logic:**
    1.  **Check Version:** Does the file contain the specific header `# >>> dctx-ignore-v[CURRENT_VERSION] >>>`?
        *   **YES:** Stop. The file is up to date.
        *   **NO (Old Block Found):** Detect the old block (`dctx-ignore-v*`) and **replace** it entirely with the new version block.
        *   **NO (No Block Found):** Check for individual patterns.
            *   If patterns are missing, append the full new block.
    2.  **Feedback:** Print a clear status message (e.g., "Updated global gitignore from v0.1.1 to v0.1.2").

## 6. Decision: Local Git Safety (Runtime)

We implement a **Fast-Path Runtime Check** to ensure safety with minimal overhead.

*   **Trigger:** Running `dctx` (CLI) inside a directory.
*   **Conditions:**
    1.  Text Output Mode is active (we are writing files).
    2.  Config `update_local_git_repo_gitignore_rules = true`.
    3.  A `.gitignore` file exists.
*   **The "Fast Path" Algorithm:**
    1.  **Read First:** Scan `.gitignore` for the marker `# [dctx vX.Y.Z]` (where X.Y.Z matches the running binary).
    2.  **Match Found:** **STOP.** The environment is known to be safe for this version. (Cost: essentially zero).
    3.  **No Match / Mismatch:**
        *   **Scan:** Check if the required patterns are present.
        *   **Update:** Append missing patterns.
        *   **Stamp:** Append or Update the marker to `# [dctx vX.Y.Z]`.
        *   **Git Add:** Execute `git add .gitignore` to track the change.
        *   **Notify:** Print `🛡️ Git Safety: Updated local rules to vX.Y.Z`.

## 7. Decision: Configuration Update Strategy (Merge-Append)

We use a **Non-Destructive Merge-Append** strategy for `config.toml` during installation.

*   **Trigger:** Execution of `tools/install.sh`.
*   **Logic:**
    1.  **Read Version:** Check `config.toml` for a version comment (e.g., `# dctx-config-version = "0.1.1"`).
    2.  **Compare:** If the file version matches the installer version, stop.
    3.  **Update:** If versions differ (or comment is missing):
        *   Scan the file for specific *new* keys introduced in the new version.
        *   **Do Not Overwrite:** Existing keys and values remain untouched.
        *   **Append:** Missing keys are appended to the end of the file with explanatory comments.
        *   **Stamp:** Update the version comment to the new version.

## 8. Implementation Plan

### 8.1. Configuration (`crates/dctx-core/src/config.rs`)
*   Add `update_local_git_repo_gitignore_rules` (bool, default true).

### 8.2. Core Logic (`crates/dctx-core/src/git_safety.rs`)
*   Refactor `ensure_local_gitignore_is_safe`.
*   Implement `check_fast_path_marker(path, version)`.
*   Implement `add_or_update_marker(path, version)`.

### 8.3. Installer (`tools/install.sh`)
*   Implement the "Merge-Append" logic for `config.toml` using `grep`/`cat`.
*   Implement the "Block Replacement" logic for Global Gitignore.
*   Ensure it updates the `.gitignore` of the source repo itself.

## 9. Validation
*   **Upgrade Test:** Configure v1 manually. Install v2. Verify config retains manual settings + has new keys.
*   **Safety Test:** Run `dctx` in a repo. Verify `.gitignore` gets the rules + the version marker.
*   **Speed Test:** Run `dctx` again. Verify "Fast Path" is taken (no `git add` execution).
