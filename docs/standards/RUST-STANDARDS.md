<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Rust standards

Use small crates with explicit domain boundaries. Domain code is free of IO;
ports define required effects and adapters implement them. Entry points compose
libraries rather than duplicating their rules. The [crate map](../CRATE-MAP.md)
describes the current layout.

## Safety and correctness

First-party Rust forbids unsafe code. Use typed errors, checked arithmetic for
sizes and offsets, explicit allocation limits, and bounded decoding of untrusted
input. Document public errors and invariants. Keep selection and authorization
ahead of content access, including cache reuse. Do not use panics for expected
failures.

## Dependencies

Read package manifests and the lockfile for the actual dependency graph;
[THIRD_PARTY.md](../../THIRD_PARTY.md) inventories the current build. Review exact
versions, features, native code and unsafe boundaries before adding dependencies.
`dctx-ed25519` wraps audited signature verification; do not replace security
primitives merely to reduce dependency counts. The desktop's UI dependencies
belong outside the pure domain and CLI closure.

## Verification

Run focused tests for changed behavior, formatting and warning-denied Clippy.
Use [TESTING.md](../TESTING.md) for the full matrix, toolchains, exclusions and
formal checks. Keep test homes and caches isolated; use fake clocks for lifecycle
properties and separate performance measurement from correctness tests.

First-party packages declare Apache-2.0 directly or inherit the workspace license.
Use `// SPDX-License-Identifier: Apache-2.0` in Rust source headers. Copyright:
Eris Margeta. The [style notes](../STYLE.md) document scoped lint exceptions.
