<p align="center">
  <img src="https://img.shields.io/badge/dctx-Directory%20Context-8b5cf6?style=for-the-badge" alt="dctx">
</p>

<h1 align="center">dctx</h1>

<p align="center">
  <strong>Transform your codebase into AI-ready context. One command.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-0.14.8--dev-8b5cf6" alt="Version">
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
  <a href="#security--privacy">Security & Privacy</a> •
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

### Interactive Mode
```bash
dctx
```
Run without arguments to launch the interactive menu system—discover all features through an intuitive TUI.

---

## Features

| Feature | Description |
|---------|-------------|
| **Privacy-First Architecture** | Ignored files never read into memory. [Security & Privacy](#security--privacy) |
| **Interactive TUI** | Zero-dependency menu system for users who prefer not to memorize CLI flags. |
| **Blazing Fast** | Multi-threaded Rust core. Processes thousands of files in milliseconds. |
| **Zero Dependencies** | Single static binary. No Python, Node, or runtime required. |
| **Smart Filtering** | Respects `.gitignore` + `.dctxignore`. Never sends `node_modules` to AI. |
| **Advanced Filters** | Filter by extension, size, content, or visibility. Combine filters for precision. |
| **Version Tracking** | Automatic versioning with diffs. Save 90% of tokens on subsequent runs. |
| **Semantic Structure** | XML-tagged output with file IDs, paths, sizes, and timestamps. |
| **Custom Templates** | AOT-compiled template system for custom output formats. |
| **Binary Detection** | Automatically detects and excludes binary files (42 file types). |
| **Cross-Platform** | Linux, macOS, Windows. Native clipboard integration. |

---

## Security & Privacy

### Privacy Guarantee

**dctx NEVER reads ignored files into memory, even in debug mode.**

When you add files to `.dctxignore` (like `.env`, `.git/`, or `credentials/`), they are:
- ❌ Never read from disk
- ❌ Never loaded into memory
- ❌ Never included in output
- ✅ Completely skipped during scanning

This guarantee is enforced through a multi-layer defense system and verified by comprehensive test coverage.

### Rule Types

**IGNORE Rules** (`.dctxignore`)
- Files/folders that "don't exist" for our purposes
- Examples: `.env`, `.git/`, `node_modules/`, credentials
- Behavior: Hidden from tree, never read, folder not recursed
- Use for: Secrets, sensitive data, large dependency folders

**OMIT Rules** (`.dctxomitcontent`)
- Files that exist but we don't care about their contents
- Examples: `package-lock.json`, `Cargo.lock`, `*.svg`
- Behavior: Shown in tree, content replaced with `[Content omitted]`
- Use for: Lockfiles, generated code, large config files

### Debug Mode (`--debug-ignored`)

Verify your ignore rules are working without compromising privacy:

```bash
dctx . --debug-ignored
```

**What happens:**
- ✅ Ignored files/folders appear in directory tree with `[IGNORED]` marker
- ✅ You can verify your `.dctxignore` patterns are matching correctly
- ❌ File contents are NEVER read, even when shown in tree
- ❌ No sensitive data enters memory

**Use case:** Troubleshooting ignore rules, verifying security configuration

### Performance Benefits

Ignore rules provide massive performance improvements:

| Scenario | Time | Speedup |
|----------|------|---------|
| Large `node_modules/` (50k files) | ~80ms | 100x faster |
| Project with `.git/` (5k objects) | ~50ms | 24x faster |
| Monorepo with multiple `target/` | ~120ms | 125x faster |

**How:** Ignored folders are skipped entirely without recursion, saving disk I/O and processing time.

### Trusted Use Cases

**Enterprise & Regulated Environments:**
- Financial services codebases (SOC2, PCI-DSS compliance)
- Healthcare applications (HIPAA compliance)
- Government projects (classified/sensitive data)

**Security-Conscious Development:**
- API keys and credentials in `.env` files
- Private SSH keys in `.ssh/`
- Cloud provider credentials
- Database connection strings

**Example Configuration:**
```bash
# .dctxignore
.env
.env.*
*.key
*.pem
.git/
.ssh/
secrets/
credentials/
```

---

## CLI Reference

```
Usage: dctx [PATH] [OPTIONS]
       dctx template <COMMAND>

Snapshot Options:
  -c, --clipboard       Copy to clipboard
  -d, --diff            Output diff only (if available)
  -t, --tree            Tree-only mode (skips file content)
  -r, --reset           Force V1 reset
  -h, --help            Show help
  -v, --version         Show version

Filter Options:
  --ext <extensions>    Only include files with these extensions
                        Example: --ext rs,toml
  --binary              Include binary files (excluded by default)
  --empty               Show only empty files (0 bytes)
  --non-empty           Show only non-empty files
  --large <MB>          Show only files larger than X megabytes
                        Example: --large 1
  --small <MB>          Show only files smaller than X megabytes
                        Example: --small 0.5
  --hidden              Show ONLY hidden files and folders (starting with .)
  --debug-ignored       List ignored folders for debugging

Template Commands:
  template build <PATH>  Compile a .dctx_template file
  template list          List compiled templates
```

### Filter Examples

```bash
# Only Rust files
dctx . --ext rs -c

# Rust and TOML files under 500KB
dctx . --ext rs,toml --small 0.5

# JavaScript/TypeScript files
dctx . --ext js,ts,tsx

# Hidden configuration files
dctx . --hidden --ext conf,yaml,toml

# See what folders are being ignored
dctx . --debug-ignored

# Include binary files (excluded by default)
dctx . --binary

# Only empty files
dctx . --empty

# Files between 10KB and 100KB
dctx . --large 0.01 --small 0.1
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

## Use as Library

`dctx` is architected as a pure Rust library (`dctx-core`) with a thin CLI wrapper. You can use it programmatically in your Rust projects.

### Rust Library Usage

Add to your `Cargo.toml`:
```toml
[dependencies]
dctx-core = { git = "https://github.com/Eris-Margeta/dctx" }
```

Use in your code:
```rust
use dctx_core::{walker, config, ignore, llm_formatter, FilterConfig};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up ignore engine
    let ignore_engine = ignore::IgnoreEngine::new(Path::new("."));
    let omit_engine = ignore::OmitContentEngine::new(Path::new("."));

    // Walk directory structure
    let tree = walker::walk_directory_structure(
        Path::new("."),
        &ignore_engine,
        &omit_engine,
    )?;

    // Apply filters (optional)
    let filter_config = FilterConfig::default()
        .with_extensions(vec!["rs".to_string(), "toml".to_string()])
        .with_max_size_mb(1.0);

    let filtered_tree = if filter_config.has_active_filters() {
        walker::filter_tree(&tree, &filter_config).unwrap()
    } else {
        tree
    };

    // Format for LLM consumption
    let output = llm_formatter::format_snapshot(&filtered_tree, "my-project");

    println!("{}", output);
    Ok(())
}
```

### Language Bindings (Planned)

**Python Bindings** (Phase 12 - After v1.0):
```python
# Future Python API
import dctx

snapshot = dctx.snapshot(
    path=".",
    filters={"ext": ["py", "md"], "max_size_mb": 1.0}
)
context = snapshot.to_llm_format()
```

**Node.js Bindings** (Phase 13 - If demand exists):
```javascript
// Future Node.js API
const dctx = require('dctx');

const snapshot = await dctx.snapshot({
    path: '.',
    filters: { ext: ['ts', 'js'] }
});
const context = snapshot.toLlmFormat();
```

**Use Cases:**
- Jupyter notebooks analyzing codebases
- VS Code extensions with right-click → "Send to AI"
- Build tools and CI/CD pipelines
- AI/ML automation workflows
- Programmatic context generation

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

dctx is structured as a **monorepo** with clear separation between open-source and commercial components:

```
dctx/
├── crates/              # Rust workspace (MIT License)
│   ├── dctx-core/       # Pure Rust library (zero external deps)
│   └── dctx-cli/        # Command-line interface
├── apps/                # Applications
│   └── dctx-desktop/    # Cross-platform GUI (PROPRIETARY - $29)
├── bindings/            # Language bindings (MIT License - Planned)
│   ├── dctx-py/         # Python bindings (Phase 12)
│   └── dctx-node/       # Node.js bindings (Phase 13)
├── tools/               # Build, test, and install scripts
├── docs/                # Documentation
├── website/             # Landing page (dctx.app)
└── .github/             # CI workflows
```

### Licensing Model

**Open Source (MIT)**:
- `dctx-core` - Core Rust library
- `dctx-cli` - Command-line tool with ALL features
- `bindings/*` - Future Python, Node.js bindings

**Commercial (Proprietary)**:
- `apps/dctx-desktop/` - Desktop GUI application ($29 one-time)

This "Open Core" model keeps developer tools free while monetizing GUI convenience.

---

## License

MIT License. See [LICENSE](LICENSE) for details.

---

## Contributing

Contributions welcome! Please read the codebase documentation in `docs/` before submitting PRs.

---

<p align="center">
  <sub>Built with Rust. Zero dependencies. Maximum speed.</sub>
</p>
