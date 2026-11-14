<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# dctx product contracts

Status: current design reference. The [specification](../SPEC.md) distinguishes
shipped behavior from planned features; the [README](../../README.md) describes
the current release. Earlier planning is retained in the [archive](../archive/README.md).

## Engine and surfaces

The `dctx` facade composes pack, map, templates and cache through explicit ports.
CLI, desktop and future bindings should use the same selection, ranking,
rendering and retention rules. Domain code has no ambient filesystem, process,
home-directory or output access. See the [crate map](../CRATE-MAP.md).

Content identity uses dctx's algorithm-tagged digest and file metadata. The core
reads `.gitignore` as selection rules, but does not use Git objects or launch Git.
Git mutations belong to the separately consented [plugin](PLUGINS.md).

## Privacy and correctness

- Select and authorize files before reading, hashing, parsing or serving cached facts.
- Publish complete immutable generations; readers pin the generation they use.
- Reclaim only unreachable objects, accounting for live leases and reservations.
- Bound storage and response bytes; return complete records or a typed refusal.
- Keep the same snapshot, policy, contract versions and query deterministic.
- Reuse facts by content identity while resolving imports in the current project.
- Report freshness, structural precision and incomplete extraction honestly.
- Support cancellation and recovery without permanent locks.

The [cache model](../formal/dctx-cache/README.md) and
[plugin models](../formal/dctx-plugins/README.md) describe modeled invariants and
mutation checks; they do not establish every platform's runtime qualification.

## Versioned contracts

| Boundary | Reference |
| --- | --- |
| Facade and wire API | [API ports v1](contracts/API-PORTS-v1.md) |
| Content identity and reuse | [Identity v1](contracts/IDENTITY-v1.md) |
| Parser precision | [Parsers v1](contracts/PARSERS-v1.md) |
| Immutable segments | [Segments v1](contracts/SEGMENTS-v1.md) |
| Managed cache lifecycle | [Cache v1](contracts/CACHE-v1.md) |
| Template source and compiled plan | [DTS v2](contracts/DTS-v2.md) |
| Plugin protocol and distribution | [Plugin API v1](plugin-api/README.md) |

Each boundary has its own version. Sketch signatures and proposed C ABI or
binding formats are design references, not promises of a stable released ABI.
Migrations retain explicit compatibility fixtures and reject unsupported formats.

## Licensing and desktop direction

All first-party dctx code and documentation are Apache-2.0, copyright Eris Margeta.
This includes the current Tauri desktop and the planned GPUI rebuild. The desktop
is free; organizations can fund [maintenance](../../MAINTENANCE.md).
