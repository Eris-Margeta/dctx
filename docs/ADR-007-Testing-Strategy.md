# ADR-007: Comprehensive CLI Testing Strategy (The Triple-Run Protocol)

**Date:** 2026-01-12
**Status:** Accepted
**Author:** dctx Team
**Context:** Previous automated tests for `dctx` were insufficient. They failed to detect "lazy" state updates (requiring two runs to register a change) and did not adequately verify the clipboard mode or configuration propagation. We need a rigorous, battle-tested standard for verifying CLI behavior that mirrors actual human usage patterns.

## 1. Context and Problem Statement
*   **State Latency:** A single run of the tool might appear successful (exit code 0), but fail to persist state correctly for the *next* run.
*   **Mode Discrepancy:** Testing file output does not prove that clipboard output works, and vice-versa.
*   **Configuration Drift:** Upgrading the binary without updating the user's config file causes features to fail silently.
*   **Blind Spots:** Automated scripts often check for file existence but not content correctness (e.g., did the version actually bump?).

## 2. Decision: The "Triple-Run" Verification Protocol
Every state-changing action (adding a file, modifying content) must be verified through a sequence of **three** executions to ensure stability and persistence.

1.  **Run 1 (The Trigger):** Execute the command after the change.
    *   *Check:* Did it detect the change? Did it generate the expected output?
2.  **Run 2 (The Confirmation):** Execute the command *again* immediately with NO changes.
    *   *Check:* Does it report "No changes detected"? Does the version remain stable? (This catches "lazy" bumps).
3.  **Run 3 (The Cross-Check):** Execute the command in the *alternative mode* (e.g., if testing Text, run Clipboard).
    *   *Check:* Does the state remain consistent across modes?

## 3. Decision: Mode Isolation
Tests must explicitly separate **Text Mode** and **Clipboard Mode** scenarios.

### Text Mode Tests
*   Verify creation of `.llmcontext.txt`.
*   Verify creation (or absence) of `*-diff.txt` based on config.
*   Verify content inside the files (headers, tree structure).

### Clipboard Mode Tests
*   Verify `stdout` message ("Copied to clipboard").
*   Verify that **NO** files are written to disk (except the binary cache).
*   Verify that the binary state **IS** updated (version increments).

## 4. Decision: Configuration Propagation Testing
We must verify that new configuration keys are correctly merged into existing user config files.

*   **Scenario:** Create a legacy `config.toml` (missing keys). Run the installer/updater.
*   **Expectation:** The file is preserved, but new keys are appended with default values.
*   **Verification:** `grep` for the new key in the config file.

## 5. Decision: Test Environment Structure
Tests must not run in the project root.
*   **Isolation:** Create a dedicated sandbox: `/tmp/dctx-test-env-<timestamp>/`.
*   **Mocking:** Override `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, and `HOME` to point to the sandbox.
*   **Modular Test Data:** Use a dedicated `tests/fixtures/` folder (or generate on the fly) to populate the sandbox.

## 6. Implementation Plan
The `tools/test-integrity.sh` script will be rewritten to strictly adhere to this protocol. It will define helper functions like `assert_version_bump`, `assert_no_change`, and `assert_config_key`.
