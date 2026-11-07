<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Development practices

Use the [contribution guide](../../CONTRIBUTING.md) for setup and review, and
[testing guide](../TESTING.md) for the current CI matrix and local checks.

Keep domain rules independent of IO; adapters implement filesystem, process and
storage boundaries through explicit ports. Prefer small changes with observable
behavior and a focused verification command. Document error and compatibility
contracts where callers use them.

Tests should isolate configuration and caches, exercise failures as well as
successful controls, and avoid timing assumptions. Preserve source privacy when
collecting diagnostics. See the [Rust standards](../standards/RUST-STANDARDS.md),
[code guidelines](../standards/CODE-GUIDELINES.md) and [style notes](../STYLE.md).

The [earlier process manual](../archive/guides/DEVELOPMENT-PRACTICES.md) is historical.
