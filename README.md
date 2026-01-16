# dctx - Directory Context for LLMs

> **Stop copy-pasting. Start engineering.**

`dctx` is a blazing-fast, zero-dependency command-line tool that transforms your codebase into a structured, version-aware context snapshot for Large Language Models.

Unlike RAG systems that fragment code into disconnected chunks, `dctx` provides a **complete, hierarchical view** of your project—letting AI understand architecture, imports, and file relationships instantly.

**Website:** [dctx.app](https://dctx.app)

---

## Key Features

- **Blazing Fast** — Written in Rust with multi-threaded file scanning. Processes thousands of files in milliseconds.
- **Zero Dependencies** — Single static binary. No Python, Node, or runtime required.
- **Smart Filtering** — Respects `.gitignore` + its own `.dctxignore`. Never accidentally send `node_modules`, binaries, or `.env` to AI.
- **Version Tracking** — Automatic versioning with optimized diffs. Save 90% of tokens on subsequent runs.
- **Semantic Structure** — XML-tagged output with file IDs, paths, sizes, and modification timestamps for AI parsing.
- **Custom Templates** — AOT-compiled template system for custom output formats.

---

## Installation

### From Source (Rust)
```bash
git clone https://github.com/Eris-Margeta/dctx
cd dctx
cargo build --release
./target/release/dctx-cli --version
```

### Quick Install (macOS/Linux)
```bash
curl -fsSL https://dctx.app/install.sh | sh
```

---

## Usage

### Basic Snapshot
```bash
dctx .
```
Creates `.dctx/dctx.llmcontext.txt` containing full project structure and content.

### Copy to Clipboard
```bash
dctx . -c
```
Paste directly into ChatGPT, Claude, or any AI chat.

### Tree-Only Mode
```bash
dctx . -t -c
```
Output directory structure without file contents. Useful for architecture discussions.

### Diff Mode
```bash
dctx . -d -c
```
After making changes, copy only what changed (requires previous snapshot).

### Force Reset
```bash
dctx . -r
```
Clear history and start fresh at V1.

---

## CLI Reference

```
Usage: dctx [PATH] [OPTIONS]
       dctx template <COMMAND>

Snapshot Options:
  -c, --clipboard  Copy to clipboard
  -d, --diff       Output diff only (if available)
  -t, --tree       Tree-Only mode (skips file content)
  -r, --reset      Force V1 reset
  -h, --help       Show help
  -v, --version    Show version

Template Commands:
  template build <PATH>  Compile a .dctx_template file
  template list          List compiled templates
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
- `config.toml` — Global settings
- `ignore` — Global ignore patterns
- `templates/` — Custom template files

---

## Output Format

dctx generates semantically structured output:

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

## Architecture

```
dctx/
├── crates/
│   ├── dctx-core/     # Pure Rust library (walker, differ, templating)
│   └── dctx-cli/      # Command-line interface
├── tools/             # Build and test scripts
└── website/           # Landing page (dctx.app)
```

---

## Performance

Tested on a 4,500-file Rust project:

| Metric | Bash + find/cat | dctx |
|--------|----------------|------|
| Time | 0.653s | 0.050s |
| Speed | — | **13.1x faster** |
| Files filtered | Manual | **97% automatic** |

---

## License

MIT License. See [LICENSE](LICENSE) for details.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.
