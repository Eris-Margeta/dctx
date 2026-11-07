<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# ADR-002: Versioning Strategy, Massive Changes & State Recovery

**Status:** superseded — historical design and implementation record.

Use the [current product contracts](design/PRODUCT-CONTRACTS.md), [README](../README.md) and [testing guide](TESTING.md) for release behavior.
**Date:** 2026-01-08
**Author:** dctx Team
**Context:** Defines how `dctx` handles version increments, file corruption, and massive filesystem changes to maintain a reliable "Source of Truth".

## 1. Context
Previous iterations relied on folder modification timestamps to detect project resets, which proved unreliable across operating systems. We need a deterministic logic to decide when to perform a standard incremental update (diff) versus a major version reset (cleanup).

## 2. Decision: Version Format
We use a simplified semantic versioning scheme: `V{Major}.{Minor}`.
*   **Major:** Represents the epoch or a massive state change (e.g., `V1`, `V2`).
*   **Minor:** Represents incremental snapshots within an epoch (e.g., `V1.1`, `V1.2`).
*   **Format:** `V1.0` is written as `V1`. The first increment is `V1.1`.

## 3. Decision: Minor Version Increment (Standard Flow)
*   **Condition:** Normal usage. Files are added, modified, or removed, but some continuity exists with the previous state.
*   **Action:**
    *   Current Version `V{X}.{Y}` becomes `V{X}.{Y+1}`.
    *   A diff file is generated: `Project-V{X}.{Y+1}-diff.txt`.
    *   The main context file is updated.

## 4. Decision: Major Version Bump (The "Soft Reset")
A Major Version Bump (e.g., `V1.x` -> `V2.0`) is triggered automatically under two conditions. This action signifies a "Fresh Start" for the context window, while preserving the binary history continuity.

### Trigger A: Massive Change Event
*   **Definition:** When comparing the Old State vs. New State, **ZERO files remain unchanged**.
    *   *Scenario:* The user deletes the entire folder content and pastes new content.
    *   *Scenario:* A build script rewrites every single file in the project.
*   **Rationale:** Generating a diff where every line is "Removed" and every new line is "Added" is wasteful for LLM tokens. It is better to treat this as a new baseline.

### Trigger B: Diff Limit Exceeded
*   **Definition:** The Minor version count exceeds the configured limit.
*   **Config Key:** `MAX_NUMBER_OF_DIFF_TXT_FILES` (Default: 999).
*   **Rationale:** Prevents the output directory from being flooded with thousands of small diff files (e.g., `V1.2000-diff.txt`).

### Action on Major Bump
1.  **Increment Major:** `V1.x` becomes `V2`.
2.  **Reset Minor:** Minor count resets to 0 (implicit).
3.  **Cleanup:** **DELETE ALL** existing diff files (`*-diff.txt`) in the output directory associated with the project.
4.  **Baseline:** The new `V2` main context file becomes the new baseline. No diff file is generated for the transition `V1.x -> V2`.

## 5. Decision: Corruption Handling (The "Hard Reset")
*   **Condition:** The binary `.dctx` state file exists but cannot be read (checksum failure, invalid format, incomplete write).
*   **Action:**
    *   **Delete** the corrupted binary file.
    *   **Delete** all associated text outputs (Main context + All Diff files).
    *   **Reset** version to `V1`.
    *   **Log** a warning to the user.

## 6. Decision: Configuration
*   New Config Option: `MAX_NUMBER_OF_DIFF_TXT_FILES` (Integer).
*   Default: `999`.
