<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# dctx documentation

Start with the [project README](../README.md) for installation and the
[runnable quickstart](../examples/README.md) for a small pack and map workflow.
The Cargo-built executable is `dctx-cli`; guides also use the alias `dctx`.

## Usage and contribution

| Document | Scope |
| --- | --- |
| [Choosing how to use dctx](MODALITIES.md) | Clipboard, short and long agent tasks, huge trees, MCP overhead and CI. |
| [Map mode](guides/MAP-MODE.md) | Structural outlines, symbol search, source reads and current limitations. |
| [Plugins](guides/PLUGINS.md) | Installation, consent, trust and platform behavior. |
| [Contributing](../CONTRIBUTING.md) | Development setup and checks. |
| [Support](../SUPPORT.md) | Usage questions and community help through issues. |
| [Testing](TESTING.md) | Cross-platform CI and local checks. |
| [Security policy](../SECURITY.md) | Reporting vulnerabilities privately. |
| [Developer guide](guides/DEVELOPER-GUIDE.md) | Contributor background; verify older commands against the current CLI. |
| [Development practices](guides/DEVELOPMENT-PRACTICES.md) | Implementation patterns and historical troubleshooting. |
| [Rust style guides](guides/RUST-STYLE-GUIDES.md) | Style references. |
| [Code guidelines](standards/CODE-GUIDELINES.md) and [Rust standards](standards/RUST-STANDARDS.md) | Coding conventions. |
| [Apache-2.0 license](../LICENSE) | Repository license. |

## Specifications and architecture

| Document | Scope |
| --- | --- |
| [Specification](SPEC.md) | Product goals, constraints, roadmap and acceptance gates; planned features are identified there. |
| [Crate map](CRATE-MAP.md) | Package inventory, boundaries and direct dependencies. |
| [Architecture history](archive/design/ARCHITECTURE-DECISIONS.md) | Superseded planning record; use current product contracts. |
| [Product contracts](design/PRODUCT-CONTRACTS.md) | Engine and surface contracts. |
| [Map specification](SPEC-MAP-MODE.md) | Map design background; use the map guide and CLI help for current behavior. |
| [API ports](design/contracts/API-PORTS-v1.md), [parsers](design/contracts/PARSERS-v1.md), [cache](design/contracts/CACHE-v1.md), [segments](design/contracts/SEGMENTS-v1.md) | Versioned boundary contracts. |
| [Template contract](design/contracts/DTS-v2.md) | ContextPlan and rendering contract. |
| [Plugin API](design/plugin-api/README.md) and [plugin design](design/PLUGINS.md) | Protocol, authority and distribution design. |
| [Cache model](formal/dctx-cache/README.md) and [plugin models](formal/dctx-plugins/README.md) | Formal models and their validation procedures. |

## Architecture decision records

These records preserve decisions and migration context. Each record carries its
own status; inclusion here does not certify that its entire design is implemented.

| ADR | Topic |
| --- | --- |
| [ADR-001](./ADR-001-System-Architecture.md) | System Architecture (Centralized State) |
| [ADR-002](./ADR-002-Versioning-and-Recovery.md) | Versioning & Recovery |
| [ADR-003](./ADR-003-Templating-Engine.md) | Templating Engine |
| [ADR-004](./ADR-004-CLI-Implementation.md) | CLI Implementation Strategy |
| [ADR-005](./ADR-005-Git-Safety-Strategy.md) | Git Safety Strategy |
| [ADR-006](./ADR-006-Versioning-Strategy.md) | Unified Versioning Strategy |
| [ADR-007](./ADR-007-Testing-Strategy.md) | Testing Strategy |
| [ADR-008](./ADR-008-Content-Omission-Pattern-System.md) | Content Omission Pattern System |
| [ADR-009](./ADR-009-Binary-File-Detection-and-Content-Omission.md) | Binary File Detection |
| [ADR-010](./ADR-010-State-Management-Enhancements.md) | State Management Enhancements |
| [ADR-011](./ADR-011-Advanced-CLI-Filters.md) | Advanced CLI Filters |
| [ADR-012](./ADR-012-Zero-Dependency-TUI.md) | Zero-Dependency TUI |
| [ADR-013](./ADR-013-Monorepo-Structure.md) | Monorepo Structure |
| [ADR-014](./ADR-014-Privacy-Guarantees-and-Ignore-Semantics.md) | Privacy Guarantees & Ignore Semantics |
| [ADR-015](./ADR-015-walker-architecture-refactoring.md) | Walker Architecture Refactoring |
| [ADR-015b](./ADR-015b-Ignore-Engine-Architecture.md) | Ignore Engine Architecture |
| [ADR-016](ADR-016-Legacy-IO-Removal.md) | Legacy IO Removal |
| [ADR-017](ADR-017-Plugin-System.md) | Plugin System |
| [ADR-018](ADR-018-Language-Coverage.md) | Language Coverage |

## Retained reference material

- [`legacy-c/`](../legacy-c/) preserves the original C implementation and project history.
- [`dossiers/`](archive/dossiers) retains dependency, platform, binding and plugin evidence;
  historical results are tied to their original configurations.
- [`research/`](research/) retains experiment scripts, data and reports, including
  the [map benchmark report](research/trail-code-2026-10/REPORT.md).

These directories are retained because their contents have historical or evidence
value. Current usage and contracts are listed above. Superseded process records are
collected in the [archive index](archive/README.md), with a link to their replacement.
