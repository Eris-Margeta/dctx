<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# dctx documentation

Start with [What dctx is](START.md). Choose a workflow before adding tools;
measure whole sessions against grep plus file reads. The current release is
1.2.0. Pages marked **coming** describe future documentation or features and
have no page link; specifications also include planned behavior.

## Start

- [What dctx is](START.md): the claim, current limits and advanced-user notice.
- [Choose your path](START.md#choose-your-path): giant folders, small projects, library builders and team admins.
- [Safe path in ten minutes](START.md#safe-path-in-ten-minutes): release archive, one project, lean MCP and measurement.
- [Responsibilities](START.md#responsibilities): what the engine supplies and what the caller owns.
- [Runnable quickstart](../examples/README.md): a small pack and map workflow.

## Modalities

- [CLI](MODALITIES.md#human-one-off-pack-to-clipboard): terminal, clipboard and shell workflows.
- [Agents](MODALITIES.md#choose-by-task): short tasks, repeated source reads and huge-tree scoping.
- [MCP](guides/MCP.md): lean/default/custom profiles, client setup and startup cost.
- ACP: **coming**.
- [Libraries](MODALITIES.md#rust-library-embed-the-facade): Rust facade now; pip/npm project libraries publishing soon.
- [CI and caching](MODALITIES.md#ci--prompt-cache-prefix-stability): fixed inputs, byte stability and measurement limits.
- [Desktop](MODALITIES.md#desktop-app-tauri-preview-today): Tauri preview and planned GPUI rebuild.

## Scopes

- [System binary](START.md#two-scopes-one-binary): you and your interactive agents own the executable on PATH.
- [Project binary](START.md#agent-builder-embedding-a-library): the planned pinned pip/npm wrapper contract and today's Rust alternative.
- [Coexistence](START.md#two-scopes-one-binary): current shared cache and planned version isolation.
- Coexistence and doctor command guide: **coming** (`doctor` is planned).
- Dedicated scopes page (`SCOPES.md`): **coming**; use the start page's scope section today.

## Engine

- [Pack format and ids](../README.md#what-the-output-looks-like): generated output; [map ids and exact reads](guides/MAP-MODE.md).
- [Catalog and index](MODALITIES.md#huge-trees-scope-before-indexing): shipped index limits and root selection; catalog-first sharding guide: **coming**.
- [Cache and limits](guides/MAP-MODE.md#index-cache-and-privacy): layout, freshness, caps, cleanup and exclusions.
- Sessions and deltas guide: **coming**; see the [roadmap](SPEC.md#31-roadmap-and-acceptance-gates) for planned agent sessions.
- [Engine specification](SPEC.md), [product contracts](design/PRODUCT-CONTRACTS.md) and [crate map](CRATE-MAP.md): boundaries, acceptance gates and implementation inventory.
- [API ports](design/contracts/API-PORTS-v1.md), [parsers](design/contracts/PARSERS-v1.md), [cache](design/contracts/CACHE-v1.md) and [segments](design/contracts/SEGMENTS-v1.md): versioned contracts, including planned behavior.

## Reference

- [Commands](../README.md#cli-reference) and [map command guide](guides/MAP-MODE.md): run `dctx --help` for the complete current CLI.
- Engine rpc protocol: **coming**; MCP's shipped stdio protocol is documented in the [MCP guide](guides/MCP.md).
- Policy file reference: **coming**; current [ignore semantics](ADR-014-Privacy-Guarantees-and-Ignore-Semantics.md) and [planned policy lattice](SPEC.md#part-iv--policy) are separate contracts.
- [Exit codes](CLI-EXIT-CODES.md): deterministic outcomes for scripts and clients.
- [Templates](design/contracts/DTS-v2.md): ContextPlan and rendering contract.
- [Plugins](guides/PLUGINS.md), [plugin API](design/plugin-api/README.md) and [plugin design](design/PLUGINS.md): current availability, trust, authority and protocol design.

Contributor references: [contributing](../CONTRIBUTING.md), [developer guide](guides/DEVELOPER-GUIDE.md),
[development practices](guides/DEVELOPMENT-PRACTICES.md), [Rust style guides](guides/RUST-STYLE-GUIDES.md),
[code guidelines](standards/CODE-GUIDELINES.md) and [Rust standards](standards/RUST-STANDARDS.md).
Older guides and design records preserve context; verify commands against current CLI help.

<details>
<summary>Architecture decision records</summary>

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

</details>

## Trust

- [Benchmarks and method](BENCHMARK-20261030.md): grep plus file reads as the baseline, with historical protocols labeled.
- [Byte-stability evidence](research/stability-20261105/REPORT.md): measured answer bytes, scope and reproduction.
- [Security](../SECURITY.md) and [privacy guarantees](ADR-014-Privacy-Guarantees-and-Ignore-Semantics.md): reporting and filesystem boundaries.
- Reproducible builds and signature verification page (`VERIFYING.md`): **coming**; current [release procedure](../RELEASING.md) and [testing](TESTING.md) cover existing checks.
- [Philosophy](WHY-DCTX.md) and [testable constraints](SPEC.md#2-the-core-belief-prompt-processing-efficiency); dedicated `PHILOSOPHY.md`: **coming**.
- [Cache model](formal/dctx-cache/README.md) and [plugin models](formal/dctx-plugins/README.md): formal validation and mutation gates.
- [Support](../SUPPORT.md), [license](../LICENSE), [maintenance](../MAINTENANCE.md) and [trademark](../TRADEMARK.md): project terms and help.

## For AI evaluators

- [Facts and current status](START.md): what ships, the interim notice, responsibilities and the planned two-scope contract.
- [Verification](TESTING.md), [benchmark method](BENCHMARK-20261030.md) and [stability reproduction](research/stability-20261105/commands.md): check claims against their scripts and limits.
- Dedicated facts and verification page: **coming**.
- Website evaluator files: [llms-small.txt](https://www.dctx.internet.technology/llms-small.txt), [llms.txt](https://www.dctx.internet.technology/llms.txt) and [llms-full.txt](https://www.dctx.internet.technology/llms-full.txt). A new engine-generated facts pipeline is planned; these links point to the published site.

<details>
<summary>Retained historical and research material</summary>

- [`legacy-c/`](../legacy-c/) preserves the original C implementation and project history.
- [`dossiers/`](archive/dossiers) retains dependency, platform, binding and plugin evidence;
  historical results are tied to their original configurations.
- [`research/`](research/) retains experiment scripts, data and reports, including
  the [map benchmark report](research/trail-code-2026-10/REPORT.md).

These directories are retained because their contents have historical or evidence
value. Current usage and contracts are listed above. Superseded process records are
collected in the [archive index](archive/README.md), with a link to their replacement.

</details>
