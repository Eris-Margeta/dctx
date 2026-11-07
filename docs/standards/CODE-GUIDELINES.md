<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Code guidelines

These conventions apply to first-party dctx contributions. See the
[Rust standards](RUST-STANDARDS.md) for language-specific guidance.

- Keep domain types and rules together, with IO behind explicit ports.
- Use public interfaces across crate boundaries; avoid shared mutable globals.
- Keep files focused and readable. Document purpose, contracts and non-obvious rules.
- Prefer clear names, bounded algorithms and explicit units for budgets and limits.
- Return typed errors. Refuse operations when privacy or authority cannot be established.
- Keep configuration, caches and diagnostics isolated in tests.
- Test observable behavior, recovery and adversarial boundaries with successful controls.
- Model critical state transitions and keep mutation witnesses for safety properties.

Run the checks in [TESTING.md](../TESTING.md). New dependencies need a clear reason,
version and feature scope, license inventory and review of native or unsafe code.
Preserve compatibility fixtures when changing formats or replacing implementations.

All first-party contributions are Apache-2.0, copyright Eris Margeta; see
[CONTRIBUTING.md](../../CONTRIBUTING.md) for contributor sign-off requirements.
