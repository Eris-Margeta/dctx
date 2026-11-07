<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# ADR-006: Unified Monorepo Versioning Strategy

**Date:** 2026-01-12
**Status:** superseded — historical design and implementation record.

Use the [current product contracts](design/PRODUCT-CONTRACTS.md), [README](../README.md) and [testing guide](TESTING.md) for release behavior.
**Author:** dctx Team
**Context:** The `dctx` project consists of multiple components (Core, CLI, FFI, Desktop App) within a single repository. Managing versions independently (e.g., CLI v0.5 using Core v0.3) creates confusion for users and complexity in dependency management. We need a deterministic, automated strategy to ensure consistency across the ecosystem.

## 1. Context and Problem Statement
*   **Dependency Drift:** If `dctx-core` is updated but `dctx-cli` is not, tracking which binary corresponds to which logic capability becomes difficult.
*   **Release Management:** Releasing "dctx v1.0" should imply that all components are at v1.0.
*   **Metadata Sync:** We have multiple locations stating the version (`VERSION`, `project-meta.yaml`, `Cargo.toml` files). Manual updates are prone to human error.

## 2. Decision: Unified Versioning (Fixed Lock)
We adopt a **Unified Versioning Model**. All components in the official release train share the **exact same version number**.

*   **Source of Truth:** The file `VERSION` in the project root.
*   **Scope:**
    *   `dctx-core`
    *   `dctx-cli`
    *   `project-meta.yaml` (Project Metadata)
    *   `installer` (via `VERSION` file read)
    *   *Future:* `dctx-desktop`

## 3. Decision: The Synchronization Tool
We do not manually edit `Cargo.toml` versions. We use a script `tools/sync-version.sh` which:
1.  Reads the root `VERSION` file.
2.  Propagates this string to all manifest files.
3.  Must be run before any commit that bumps the version.

## 4. Decision: Release Workflow
1.  Developer updates `VERSION` file (e.g., `0.1.0` -> `0.1.1`).
2.  Developer runs `tools/sync-version.sh`.
3.  Developer commits changes: `chore: bump version to 0.1.1`.
4.  CI/CD pipeline reads `VERSION` to tag the release artifacts.

## 5. Consequences
### Positive
*   **Simplicity:** A user checks `dctx --version` and knows exactly what Core logic they are running.
*   **Automation:** Reduces release friction.
*   **Clarity:** "dctx v1.0" is a coherent product, not a mix of libraries.

### Negative
*   **Artificial Bumps:** If we fix a bug in the CLI only, we still bump the Core version, even if the Core logic hasn't changed. We accept this trade-off for the sake of ecosystem consistency.
