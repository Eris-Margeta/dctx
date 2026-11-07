<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Desktop documentation index

Start with [Getting started](GETTING_STARTED.md) or the [desktop README](../README.md).
The desktop has its own version and release process, separate from the CLI.

| Guide | What it covers |
| --- | --- |
| [Getting started](GETTING_STARTED.md) | Installation, first snapshot and common questions. |
| [Configuration](CONFIG.md) | Settings and configuration inheritance. |
| [Development](DEVELOPMENT.md) | Setup, architecture and debugging. |
| [Building](BUILDING.md) | Platform prerequisites and build commands. |
| [Distribution](DISTRIBUTION.md) | Packaging and release validation. |
| [Drag and drop](DRAG_DROP_DEBUG.md) | Troubleshooting file and directory selection. |
| [Changelog](../CHANGELOG.md) | Desktop version history and roadmap. |

## Architecture decisions

- [Tauri technology choice](ADR/ADR-DESKTOP-001-tauri-technology-choice.md)
- [Core dependency architecture](ADR/ADR-DESKTOP-002-core-dependency-architecture.md)
- [Configuration inheritance](ADR/ADR-DESKTOP-003-config-inheritance-strategy.md)
- [UI and engine separation](ADR/ADR-DESKTOP-004-complete-separation.md)

The former phase-completion report is not part of this checkout; use the
architecture decisions above for design context. Update the relevant guides and
the desktop changelog when behavior changes, and check relative links before
submitting documentation changes.
