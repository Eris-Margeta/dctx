# Contributing to dctx

Thank you for your interest in contributing to dctx! This document provides guidelines and information for contributors.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [How Can I Contribute?](#how-can-i-contribute)
- [Development Setup](#development-setup)
- [Project Structure](#project-structure)
- [Development Workflow](#development-workflow)
- [Pull Request Process](#pull-request-process)
- [Coding Standards](#coding-standards)
- [Testing Guidelines](#testing-guidelines)

---

## Code of Conduct

This project adheres to a Code of Conduct that all contributors are expected to follow:

- Be respectful and inclusive
- Focus on constructive feedback
- Assume good intentions
- No harassment or discrimination

---

## How Can I Contribute?

### Reporting Bugs

Before creating bug reports, please check the issue tracker to avoid duplicates.

**Good bug reports include:**
- Clear description of the problem
- Steps to reproduce
- Expected vs actual behavior
- System information (OS, Rust version, etc.)
- Relevant logs or error messages

### Suggesting Enhancements

Enhancement suggestions are tracked as GitHub issues.

### Your First Code Contribution

Look for issues labeled:
- `good first issue` - Simple issues for newcomers
- `help wanted` - Issues where maintainers need assistance

---

## Development Setup

### Prerequisites

```bash
# Rust (latest stable)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# For desktop app
curl -fsSL https://bun.sh/install | bash

# Task runner (optional)
cargo install just
```

### Clone and Build

```bash
git clone https://github.com/Eris-Margeta/dctx
cd dctx

# Run all checks
just ci

# Build CLI
just build
```

---

## Project Structure

```
dctx/
├── crates/
│   ├── dctx-core/       # Core library (MIT)
│   └── dctx-cli/        # CLI tool (MIT)
├── apps/
│   └── dctx-desktop/    # Desktop app (MIT)
├── docs/                # Documentation + ADRs
└── tools/               # Build scripts
```

---

## Development Workflow

1. **Fork** the repository
2. **Create a branch** from `main`
3. **Make changes**
4. **Test**: `just ci`
5. **Commit** with clear messages
6. **Open a Pull Request**

### Commit Messages

Follow conventional commits:

```
type(scope): subject

Examples:
feat(core): add binary file detection
fix(cli): handle empty directory
docs: update installation guide
```

---

## Pull Request Process

Before submitting:

1. Run `just ci` locally
2. Update documentation if needed
3. Add tests for new features
4. Update CHANGELOG.md for significant changes

---

## Coding Standards

- Follow `cargo fmt` for formatting
- Fix all `cargo clippy` warnings
- Document public APIs
- Keep functions focused
- Use `Result` for errors

---

## Testing

```bash
# Run tests
just test

# Desktop app
just desktop-dev
```

Aim for >80% coverage on core library.

---

## Getting Help

- **Questions**: GitHub Discussions
- **Bugs**: GitHub Issues
- **Email**: eris.margeta@gmail.com

---

## License

By contributing, you agree your contributions will be licensed under MIT License.

Thank you for contributing! 🚀
