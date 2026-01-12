# ADR-005: Enhanced Git Safety & Global Integration Strategy

**Date:** 2026-01-12
**Status:** Accepted
**Author:** dctx Team
**Context:** `dctx` generates large context files (`.llmcontext.txt`), diffs, and potentially binary state files. While ADR-001 moved the *binary state* to a centralized cache, the *text output* (intended for LLMs) is written to the project directory (specifically `.dctx/`). Users frequently forget to ignore these files, leading to repository pollution. We need a robust, automated, and multi-layered strategy to prevent this.

## 1. Context and Problem Statement
*   **Pollution:** Generated files (`*.llmcontext.txt`) are often large and should never be committed to source control.
*   **User Friction:** Manually adding rules to `.gitignore` for every new tool is tedious and prone to error.
*   **Global vs. Local:** Some users prefer a system-wide rule, while others need project-specific rules that travel with the repo (for team consistency).
*   **Safety:** We must ensure we don't accidentally ignore user files, only our own artifacts.

## 2. Decision Drivers
*   **"Safe by Default":** The tool should prevent accidents without user intervention.
*   **Idempotency:** Repeated runs (or installations) must not duplicate entries in configuration files.
*   **Configurability:** Users must be able to opt-out of automated file modifications.
*   **Transparency:** The tool must inform the user when it modifies their environment.

## 3. Decision: Two-Layered Protection Strategy

We will implement git safety at two distinct points in the lifecycle: **Installation Time** (Global) and **Runtime** (Local).

### 3.1. Layer 1: Global Git Configuration (Installation Time)
*   **Trigger:** Execution of `tools/install.sh`.
*   **Target File:** The user's global git ignore file.
    *   *Linux/macOS:* Typically `~/.config/git/ignore` (or `~/.gitignore_global` if configured). The installer will attempt to detect the configured global ignore file via `git config --global core.excludesfile`. If unset, it defaults to `~/.config/git/ignore`.
*   **Action:** Append `dctx` specific patterns if they are missing.
*   **Feedback:** Print a clear success/skip message to the console.

### 3.2. Layer 2: Local Repository Configuration (Runtime)
*   **Trigger:** Running `dctx` (CLI) inside a directory.
*   **Conditions:**
    1.  Text Output Mode is active (we are actually writing files).
    2.  Configuration `UPDATE_LOCAL_GIT_REPO_GITIGNORE_RULES` is `true` (Default).
    3.  A `.gitignore` file exists (we do not create one if the project isn't using git).
*   **Action:**
    1.  Scan `.gitignore`.
    2.  If our specific patterns are missing, append them.
    3.  **Auto-Stage:** Execute `git add .gitignore` to ensure the rule update is tracked immediately.
*   **Feedback:** Log a message to stdout/stderr: `"🛡️  Git Safety: Updated and staged .gitignore with dctx rules."`

## 4. Decision: Defined Patterns & Markers

To ensure we only manage *our* files, we use a strict list of patterns and a comment marker.

### The Patterns
1.  `.dctx` (Legacy/Backup binary files).
2.  `.dctx/` ( The output directory defined in ADR-001 Phase 3.3).
3.  `*.llmcontext.txt` (Main context file).
4.  `*.llmcontext-*-diff.txt` (Specific diff naming convention).

### The Marker
We wrap or precede our rules with a comment to allow for future programmatic updates/removal and to inform the user.
```gitignore
# [dctx] Automated Ignore Rules - github.com/Eris-Margeta/dctx
.dctx
.dctx/
*.llmcontext.txt
*.llmcontext-*-diff.txt
```

## 5. Decision: Configuration Updates

We add a new key to `AppConfig` (Layer 1 & 2 in configuration hierarchy):

*   **Key:** `update_local_git_repo_gitignore_rules`
*   **Type:** `bool`
*   **Default:** `true`
*   **Location:** `crates/dctx-core/src/config.rs`

## 6. Implementation Plan

### Installer (`install.sh`)
*   Add logic to detect `core.excludesfile`.
*   Read the file, grep for `# [dctx]`.
*   If missing, append the block.

### Core (`git_safety.rs`)
*   Implement `ensure_local_gitignore_is_safe(target_dir: &Path)`.
*   Logic:
    *   Read `.gitignore`.
    *   Check for *all* 4 patterns.
    *   If *any* are missing, append the missing ones (or the whole block if the marker is missing).
    *   If updated, run `std::process::Command::new("git").arg("add").arg(".gitignore")...`

### Core (`config.rs`)
*   Add `update_local_git_repo_gitignore_rules` to `AppConfig` struct and parser.

## 7. Validation
*   **Test 1:** Run installer. Check `~/.config/git/ignore`. Verify patterns exist. Run installer again. Verify no duplicates.
*   **Test 2:** Run `dctx` in a new git repo. Check `.gitignore`. Verify rules added. Check `git status`. Verify `.gitignore` is staged.
*   **Test 3:** Set `update_local_git_repo_gitignore_rules = false` in `~/.config/dctx/config.toml`. Run `dctx`. Verify `.gitignore` is untouched.

