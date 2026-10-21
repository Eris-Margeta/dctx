# dctx Documentation

This directory contains the public-facing documentation for the dctx project.

## Architecture Decision Records (ADRs)

ADRs document significant architectural decisions made during the development of dctx. Each ADR captures the context, decision, and consequences of important technical choices.

**Why ADRs?**
- Preserve the reasoning behind architectural decisions
- Help new contributors understand the "why" behind the code
- Provide historical context for future maintainers
- Enable informed refactoring decisions

### Available ADRs

| ADR | Title | Status |
|-----|-------|--------|
| [ADR-001](./ADR-001-System-Architecture.md) | System Architecture (Centralized State) | IMPLEMENTED |
| [ADR-002](./ADR-002-Versioning-and-Recovery.md) | Versioning & Recovery | IMPLEMENTED |
| [ADR-003](./ADR-003-Templating-Engine.md) | Templating Engine | IMPLEMENTED |
| [ADR-004](./ADR-004-CLI-Implementation.md) | CLI Implementation Strategy | IMPLEMENTED |
| [ADR-005](./ADR-005-Git-Safety-Strategy.md) | Git Safety Strategy | IMPLEMENTED |
| [ADR-006](./ADR-006-Versioning-Strategy.md) | Unified Versioning Strategy | IMPLEMENTED |
| [ADR-007](./ADR-007-Testing-Strategy.md) | Testing Strategy | IMPLEMENTED |
| [ADR-008](./ADR-008-Content-Omission-Pattern-System.md) | Content Omission Pattern System | IMPLEMENTED |
| [ADR-009](./ADR-009-Binary-File-Detection-and-Content-Omission.md) | Binary File Detection | IMPLEMENTED |
| [ADR-010](./ADR-010-State-Management-Enhancements.md) | State Management Enhancements | IMPLEMENTED |
| [ADR-011](./ADR-011-Advanced-CLI-Filters.md) | Advanced CLI Filters | IMPLEMENTED |
| [ADR-012](./ADR-012-Zero-Dependency-TUI.md) | Zero-Dependency TUI | IMPLEMENTED |
| [ADR-013](./ADR-013-Monorepo-Structure.md) | Monorepo Structure | IMPLEMENTED |
| [ADR-014](./ADR-014-Privacy-Guarantees-and-Ignore-Semantics.md) | Privacy Guarantees & Ignore Semantics | IMPLEMENTED |
| [ADR-015](./ADR-015-walker-architecture-refactoring.md) | Walker Architecture Refactoring | IMPLEMENTED |
| [ADR-015b](./ADR-015b-Ignore-Engine-Architecture.md) | Ignore Engine Architecture | IMPLEMENTED |

## Development Guides

The \`guides/\` directory contains comprehensive guides for developers working on or with dctx.

### Available Guides

| Guide | Description |
|-------|-------------|
| [Developer Guide](./guides/DEVELOPER-GUIDE.md) | Comprehensive onboarding guide for new contributors. Covers project structure, architecture, and development workflow. |
| [Rust Style Guide](./guides/RUST-STYLE-GUIDES.md) | Rust coding standards and best practices used in this project. |
| [Development Practices](./guides/DEVELOPMENT-PRACTICES.md) | Solutions to recurring technical challenges and best practices. |

## Quick Links

- **Main README**: [../README.md](../README.md)
- **Contributing**: See [Developer Guide](./guides/DEVELOPER-GUIDE.md)
- **License**: [MIT License](../LICENSE)

## How to Use This Documentation

### For Users
- Start with the main [README](../README.md) for installation and usage
- Refer to ADRs if you're curious about why certain design decisions were made

### For Contributors
1. Read the [Developer Guide](./guides/DEVELOPER-GUIDE.md) first
2. Review relevant ADRs for the area you're working on
3. Follow the [Rust Style Guide](./guides/RUST-STYLE-GUIDES.md) for code standards
4. Consult [Development Practices](./guides/DEVELOPMENT-PRACTICES.md) for common patterns

### For Maintainers
- Use ADRs as reference when reviewing architectural changes
- Update ADRs when making significant architectural decisions
- Keep guides synchronized with actual codebase state

## Documentation Philosophy

**External Memory Principle**: This documentation serves as the project's long-term memory. Code changes, but the reasoning behind those changes (captured in ADRs) provides valuable context for future work.

**Separation of Concerns**:
- **ADRs** document the "why" (decisions and rationale)
- **Code comments** document the "what" (implementation details)
- **Guides** document the "how" (workflows and patterns)

## Contributing to Documentation

If you find documentation that is outdated or unclear:

1. **Small fixes**: Submit a PR directly
2. **Structural changes**: Open an issue first to discuss
3. **New ADRs**: Follow the ADR template format

Documentation improvements are always welcome!

---

**Note**: This is the public documentation directory. Internal project management documents are not included in public releases.
