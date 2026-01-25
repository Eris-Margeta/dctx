<p align="center">
  <img src="https://img.shields.io/badge/dctx-Directory%20Context-8b5cf6?style=for-the-badge" alt="dctx">
</p>

<h1 align="center">dctx</h1>

<p align="center">
  <strong>Transform your codebase into AI-ready context. One command.</strong>
</p>

<p align="center">
  <a href="https://github.com/Eris-Margeta/dctx/actions/workflows/ci.yml">
    <img src="https://github.com/Eris-Margeta/dctx/actions/workflows/ci.yml/badge.svg" alt="CI">
  </a>
  <a href="https://github.com/Eris-Margeta/dctx/releases">
    <img src="https://img.shields.io/github/v/release/Eris-Margeta/dctx?color=8b5cf6" alt="Release">
  </a>
  <a href="https://github.com/Eris-Margeta/dctx/blob/main/LICENSE">
    <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License">
  </a>
  <img src="https://img.shields.io/badge/rust-100%25-orange" alt="Rust">
  <img src="https://img.shields.io/badge/dependencies-0-brightgreen" alt="Zero Dependencies">
</p>

<p align="center">
  <a href="#installation">Installation</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#features">Features</a> •
  <a href="#cli-reference">CLI Reference</a> •
  <a href="https://dctx.app">Website</a>
</p>

---

## Why dctx?

**Stop fighting with bash commands.** Sharing code context with AI shouldn't require complex shell scripting.

```bash
# The old way (don't do this)
find . -type f \( -name "*.ts" -o -name "*.tsx" \) \
  -not -path "./node_modules/*" -not -path "./dist/*" | \
  head -50 | xargs -I {} sh -c 'echo "=== {} ===" && cat {}' | pbcopy

# The dctx way
dctx . -c
```

`dctx` is a blazing-fast, **zero-dependency** CLI that transforms your codebase into structured, version-aware context snapshots optimized for Large Language Models.

Unlike RAG systems that fragment code into disconnected chunks, dctx provides a **complete, hierarchical view** of your project—letting AI understand architecture, imports, and file relationships instantly.

---

## Installation

### From Source (Recommended)
```bash
git clone https://github.com/Eris-Margeta/dctx
cd dctx
cargo build --release
./target/release/dctx-cli --version
```

### Quick Install Script
```bash
curl -fsSL https://dctx.app/install.sh | sh
```

### Using Cargo
```bash
cargo install --path crates/dctx-cli
```

---

## Quick Start

### Create a Snapshot
```bash
dctx .
```
Creates `.dctx/dctx.llmcontext.txt` with full project structure and content.

### Copy to Clipboard
```bash
dctx . -c
```
Paste directly into ChatGPT, Claude, or any AI assistant.

### Tree-Only Mode
```bash
dctx . -t -c
```
Output directory structure without file contents—perfect for architecture discussions.

### Diff Mode
```bash
dctx . -d -c
```
After making changes, copy only what changed (saves tokens!).

---

## Features

| Feature | Description |
|---------|-------------|
| **Blazing Fast** | Multi-threaded Rust core. Processes thousands of files in milliseconds. |
| **Zero Dependencies** | Single static binary. No Python, Node, or runtime required. |
| **Smart Filtering** | Respects `.gitignore` + `.dctxignore`. Never sends `node_modules` to AI. |
| **Version Tracking** | Automatic versioning with diffs. Save 90% of tokens on subsequent runs. |
| **Semantic Structure** | XML-tagged output with file IDs, paths, sizes, and timestamps. |
| **Custom Templates** | AOT-compiled template system for custom output formats. |
| **Cross-Platform** | Linux, macOS, Windows. Native clipboard integration. |

---

## CLI Reference

```
Usage: dctx [PATH] [OPTIONS]
       dctx template <COMMAND>

Snapshot Options:
  -c, --clipboard  Copy to clipboard
  -d, --diff       Output diff only (if available)
  -t, --tree       Tree-only mode (skips file content)
  -r, --reset      Force V1 reset
  -h, --help       Show help
  -v, --version    Show version

Template Commands:
  template build <PATH>  Compile a .dctx_template file
  template list          List compiled templates
```

---

## Output Format

dctx generates semantically structured output that AI can parse efficiently:

```xml
[DCTX_LLM_CONTEXT_SNAPSHOT_V1]
[DIRECTORY:my-project]

<INSTRUCTIONS>
1. Manifest: The <DIRECTORY_TREE> section lists all files.
   - Each entry: [TYPE] RELATIVE_PATH (ID:UNIQUE_ID, MOD:TIMESTAMP)
2. Content Access: Find UNIQUE_ID, search for <FILE_CONTENT_START ID="...">
</INSTRUCTIONS>

<DIRECTORY_TREE>
[D]  (ID:ROOT, MOD:1234567890)
  [F] src/main.rs (ID:F001, MOD:1234567890, SIZE:1024)
  [F] Cargo.toml (ID:F002, MOD:1234567890, SIZE:256)
</DIRECTORY_TREE>

<FILE_CONTENT_START ID="F001" PATH="src/main.rs" LINES="42">
fn main() {
    println!("Hello, world!");
}
</FILE_CONTENT_END ID="F001">
```

---

## Configuration

### Project-Level: `.dctxignore`

Place in your project root. Same syntax as `.gitignore`:

```gitignore
# Ignore build artifacts
target/
dist/
build/

# Ignore dependencies
node_modules/
vendor/

# Keep specific files
!dist/config.json
```

### Global Config: `~/.config/dctx/`

| File | Purpose |
|------|---------|
| `config.toml` | Global settings |
| `ignore` | Global ignore patterns |
| `templates/` | Custom template files |

---

## Performance

Tested on a 4,500-file Rust project:

| Metric | Bash + find/cat | dctx | Improvement |
|--------|-----------------|------|-------------|
| Time | 0.653s | 0.050s | **13.1x faster** |
| Files filtered | Manual | Automatic | **97% noise removed** |

---

## Development

```bash
# Format code
just fmt

# Run tests
just test

# Run all CI checks locally
just ci

# Test on Linux via Docker
just test-linux

# Build release
just build
```

See [justfile](justfile) for all available commands.

---

## Architecture

```
dctx/
├── crates/
│   ├── dctx-core/     # Pure Rust library (zero external deps)
│   └── dctx-cli/      # Command-line interface
├── tools/             # Build, test, and install scripts
├── website/           # Landing page (dctx.app)
└── .github/           # CI workflows
```

---

## License

MIT License. See [LICENSE](LICENSE) for details.

---

## Contributing

Contributions welcome! Please read the codebase documentation in `PROJECT-INTERNAL/` before submitting PRs.

---

<p align="center">
  <sub>Built with Rust. Zero dependencies. Maximum speed.</sub>
</p>
