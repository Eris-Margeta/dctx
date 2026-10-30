<p align="center">
  <img src="https://img.shields.io/badge/dctx-Context%2C%20engineered-8b5cf6?style=for-the-badge" alt="dctx">
</p>

<h1 align="center">dctx</h1>

<p align="center">
  <strong>The context engine for AI-assisted software development.</strong><br>
  Fewer tokens. Stable prefixes. Exact source. One policy for every workflow.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-0.25.0--dev-8b5cf6" alt="Version">
  <a href="https://github.com/Eris-Margeta/dctx/actions/workflows/ci.yml?query=branch%3Amain">
    <img src="https://github.com/Eris-Margeta/dctx/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI">
  </a>
  <a href="https://github.com/Eris-Margeta/dctx/blob/main/LICENSE">
    <img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License">
  </a>
  <img src="https://img.shields.io/badge/rust-100%25-orange" alt="Rust">
  <img src="https://img.shields.io/badge/enterprise-licenses%20available-8b5cf6" alt="Enterprise licenses available">
  <img src="https://img.shields.io/badge/telemetry-none-brightgreen" alt="No telemetry">
</p>

<p align="center">
  <a href="#the-core-belief-prompt-processing-efficiency">Philosophy</a> •
  <a href="#installation">Installation</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#map-mode-context-for-agents">Map mode</a> •
  <a href="#security--privacy">Security & Privacy</a> •
  <a href="docs/SPEC.md">Specification</a> •
  <a href="https://dctx.app">Website</a>
</p>

---

## The core belief: prompt-processing efficiency

Every token you put in front of a model is paid for three times:

- **Prefill** — the model must process every input token before it answers. You pay per token, and you wait for it.
- **Cache misses** — providers serve repeated prompt prefixes from a cache for a fraction of the price and latency. One changed byte early in a prompt throws the rest of the cache away.
- **Attention** — answers get worse as irrelevant tokens pile up.

dctx exists to send **the fewest, most relevant bytes, in an order that stays the same between turns**, so prompts are smaller, caches hit, and the model reasons over exact source instead of guesses. That is the whole philosophy, and every feature has to earn its place against it. The [specification](docs/SPEC.md#2-the-core-belief-prompt-processing-efficiency) turns it into five testable constraints — bounded, deterministic, prefix-stable, non-redundant, honest — and eight metrics.

### One engine for every way software is built

| You work by… | dctx gives you |
|---|---|
| **Hand-coding** in an unfamiliar codebase | `dctx map` outlines, symbol search and exact source reads |
| **Copy-paste** into a chat assistant | `dctx pack` — the whole project or only what changed, in one structured block |
| **Tab completion** and IDE assistants | A library facade with millisecond reads for the editor's own context budget |
| **Agents** in a terminal | Bounded, copyable, deterministic answers: five verbs, one record per line |
| **Fully automated software factories** | A specification for signed organization policy and receipts; the policy lattice is planned |

### Zero RAG defects

Chunk retrieval over embeddings fails in predictable ways: it cuts functions in half, returns different chunks for the same question, serves stale code, leaks ignored files, and hides how much it cost. dctx does not use embeddings. It answers from a lexical graph of syntax-owned spans with explicit confidence, verifies source bytes before emitting them, and reports precision and freshness. The [spec](docs/SPEC.md#3-zero-rag-defects) names ten defect classes and their intended defenses; its [roadmap](docs/SPEC.md#31-roadmap-and-acceptance-gates) distinguishes the measured baseline from planned work.

---

## Where it comes from

dctx started in **2025 as `dircontxt`, written in pure C** ([`legacy-c/`](legacy-c/)), to solve the context problem while we were still moving to an AI workflow: one command that gave a model the whole project — structured, versioned, with diffs and a strict ignore system — instead of pasting forty files. It was rewritten in Rust in 2026 and grew a privacy-first core, a template system, a plugin host, a desktop app and map mode for agents. The goal has not changed: **context, engineered.**

---

## Installation

Requires a recent stable Rust toolchain with Cargo (the workspace includes Rust 2024 crates).

### From source

```zsh
git clone https://github.com/Eris-Margeta/dctx
cd dctx
cargo build --locked --release -p dctx-cli
./target/release/dctx-cli --version
```

The package and Cargo-built executable are named **`dctx-cli`** (`dctx-cli.exe` on Windows). The help text and examples use **`dctx`**. After building, this zsh alias lets you use those examples in the current shell:

```zsh
alias dctx="$PWD/target/release/dctx-cli"
```

### Using Cargo

From the cloned repository root:

```zsh
cargo install --locked --path crates/dctx-cli
dctx-cli --version
alias dctx=dctx-cli
```

Ensure Cargo's binary directory (normally `$HOME/.cargo/bin`) is on `PATH`. This installs `dctx-cli`; the alias supplies `dctx` in the current shell. Neither command installs a package from crates.io.

Alternatively, the Unix [installer](tools/install.sh) builds the CLI and copies it as `dctx` to `/usr/local/bin` (or `DCTX_INSTALL_DIR`). It also creates configuration, templates and ignore rules under `${XDG_CONFIG_HOME:-$HOME/.config}/dctx` and may request sudo for the binary copy. Review it before running.

---

## Quick Start

```zsh
dctx pack .            # whole project as one structured context block on stdout
dctx pack . --clipboard  # … straight to the clipboard
dctx . -d -c           # only what changed since the last snapshot
dctx . -t -c           # tree only, no file contents
dctx                   # interactive menu
```

`dctx pack` writes to stdout, the clipboard or `--output`; it creates a text export only when you request a file. The legacy `dctx .` snapshot commands still write project text exports and cache state. Running without arguments opens an interactive terminal menu.

---

## Map mode: context for agents

```zsh
# Run from this repository's root; map needs a file target.
dctx map map crates/dctx-cli/src/lib.rs --depth 2
dctx map find "SnapshotConfig"
# Use a stable ID returned by map/find for neighbors, explain and read.
dctx map neighbors 's:crates/dctx-cli/src/lib.rs#SnapshotConfig' --edge calls --dir in
dctx map explain 's:crates/dctx-cli/src/lib.rs#SnapshotConfig'
dctx map read 's:crates/dctx-cli/src/lib.rs#SnapshotConfig' --json
```

- **Source bytes**: `map read --json` includes source content; the current text renderer prints metadata only.
- **Bounded**: output uses an estimated token budget (up to 2,000; explain up to 1,000), not a model tokenizer guarantee, and never splits a record.
- **Deterministic**: the same tree and question give the same bytes, so prompt caches hit.
- **Precision and freshness**: use `--json` for the structured metadata. Structural declarations and conservative call edges do not imply exact semantic resolution.
- **Structural language coverage**: Rust, JavaScript, TypeScript/TSX, Python, Go, Java, C/C++, C#, Kotlin, Ruby, PHP, Swift, Markdown, and JSON/JSONC/TOML/YAML configuration. The [extension dispatch](crates/dctx/src/index/extract.rs) and [language tests](crates/dctx/tests/map_languages.rs) cover more languages than the current `--version --verbose` capability list reports; this is not full compiler semantics.
- **Outline symbols** (`guess` confidence): Scala, Dart, Zig, Solidity, Gleam, Groovy, Elixir, Lua, OCaml, R, Julia, Haskell, Terraform/HCL, SQL, shell, Makefile, Dockerfile, protobuf, Pascal and GraphQL. Headed sections for `.txt`, `.rst`, `.adoc`, `.org` and `.tex` documents.
- **Outline symbols and calls**: Clojure, Common Lisp, Scheme, Emacs Lisp and Nix. The script blocks of Vue, Svelte, Astro and HTML.
- **No Git, no network for map queries**: identity comes from dctx's own content digest; map does not need Git history or embeddings.
- **Skeleton and grep**: `dctx map skeleton <file>` prints the declaration signatures of one file, with no bodies, in source order. `dctx map grep <literal>` is an exhaustive literal search of indexed files, with hits grouped by enclosing symbol; `-i` makes it ASCII case-insensitive.
- **Hubs**: `dctx map hubs` lists the most-called symbols and the file and symbol counts per directory (`--limit` 1 to 60, default 20).
- **Path**: `dctx map path <FROM> <TO>` prints the shortest edge path (calls, imports, contains) of at most 8 hops, or `no path`.
- **Export**: `dctx map export --format json|mermaid` writes the whole graph as one deterministic document. The mermaid form is a flowchart of calls and imports. `--output PATH` writes a file.
- **Neighbors**: `--edge calls|imports` selects the edge kind, and `--depth` goes up to 4. Inbound call records carry their call-site lines, at most three per record: `\tL6,L7` in text and a `sites` array in JSON.
- **Find**: quote a multi-word concept as one argument; a name that spells the whole phrase ranks just under an exact name. For config keys with no symbol match, `find` falls back to literal hits, and a SCREAMING_CASE key (`HOME`, `DJANGO_SETTINGS_MODULE`) always lists the code that reads it after the best exact symbol.
- **Outline of a file**: `map map <file>` lists exported top-level declarations first, then the rest in source order, with impl blocks, constants and tests last; without `--budget` its page is 1,000 tokens so `--limit 40` is not cut early. Map and find text lines are `<id>\t<kind>\tL<start>-<end>`, since the id already names the path.
- **MCP**: `dctx mcp` runs a stdio MCP server that exposes the map verbs as tools, including skeleton, path and export.
- **Byte-stable answers**: the text header is `# <place> <verb>`, and JSON leaves out the snapshot digest, generation and age. `--meta` restores them.
- **Source limit**: `DCTX_MAX_SOURCE_BYTES` sets the total source bytes one scan may read (1 to 17179869184; default 1 GiB).

Details: [Map mode guide](docs/guides/MAP-MODE.md). The three-way benchmark against graphify and Trail-Code is in [docs/BENCHMARK-20261030.md](docs/BENCHMARK-20261030.md). Planned next — agent hooks, blast radius for a diff, delta answers and prefix-stable context tiers — is specified in [docs/SPEC.md](docs/SPEC.md#31-roadmap-and-acceptance-gates).

---

## Organization control

The dctx Template System (DTS) defines output templates. The specification proposes extending it to selection, budgets, parsers, cache limits, plugins and privacy through a **seven-layer, signed policy lattice** — built-in → organization → tenant → machine → user → project → session — where each layer can only narrow the one above it. The lattice and `dctx policy explain` are planned, not current CLI commands. See [Part IV of the spec](docs/SPEC.md#part-iv--policy).

---

## Plugins

External effects are designed to run through out-of-process plugins with explicit capabilities and consent. The bundled Git plugin implements `.gitignore` protection and optional staging, but the current product host refuses native plugin launch, including the Git recipe; consent alone does not enable execution. See the [Plugins guide](docs/guides/PLUGINS.md).

```zsh
dctx plugin list
dctx plugin info dctx/git
```

---

## Security & Privacy

**The filesystem selection boundary excludes ignored file contents, including in debug mode.**

| Rule file | Behaviour |
|---|---|
| `.gitignore`, `.dctxignore`, global ignore | Excluded from normal selection; `--debug-ignored` can show ignored path metadata without reading content |
| `.dctxomitcontent` | Listed in the tree; content replaced with `[Content omitted]` and never read |

Selection happens before any byte is read, hashed, parsed or cached, and is checked again before output leaves the engine. Read-spy tests enforce it (`crates/dctx-adapter-fs/tests/privacy.rs`, `crates/dctx-map-query/tests/source_privacy.rs`).

```zsh
dctx . --debug-ignored     # see ignored paths in the tree without reading them
```

```gitignore
# .dctxignore
.env
.env.*
*.key
*.pem
.ssh/
secrets/
credentials/
```

The core pack/map workflows run locally, have **no telemetry** and **call no model**. The current product CLI has no TLS network adapter for plugin discovery or downloads; network-capable distribution and plugin designs are documented separately. See the [Plugins guide](docs/guides/PLUGINS.md#what-is-not-yet-available) for current limits and [SECURITY.md](SECURITY.md) for private vulnerability reporting.

---

## Measured, in the open

We benchmarked dctx against Trail-Code on a dedicated machine with pinned versions, no network during runs and an adversarial corpus. dctx led on cold indexing (0.09–6.05 s vs 2.4–95 s, 1.6–27× less memory), footprint (one 9.2 MB binary vs 405.6 MB of Node.js packages), answer bounds and network privacy. Trail-Code led on call graphs, answers to agent questions and stability under edits. The report, raw data and scripts are in [docs/research/trail-code-2026-10](docs/research/trail-code-2026-10/REPORT.md); the fixes are on the [roadmap](docs/SPEC.md#31-roadmap-and-acceptance-gates). See also the [changelog](CHANGELOG.md).

---

## Open source and Enterprise

Project code is open source (Apache-2.0) and **ongoing work**; third-party dependencies retain their own licenses. Parts of the current development line use third-party crates; anything with third-party dependencies is not considered enterprise-ready.

The dependency policy retains the **Ed25519 signature verifier** (`ed25519-dalek`) for plugin/index verification. Signed organization policy bundles are planned; their presence in the specification does not mean the CLI can enroll or verify them today. See the [dependency policy](docs/SPEC.md#27-dependencies-and-enterprise-readiness).

**dctx Enterprise** — enterprise-ready builds, organization and tenant policy control, and direct support — is licensed per company, **from €45,000 per year**.

---

## CLI Reference

```
Usage: dctx [PATH] [OPTIONS]
       dctx pack [PATH] [--output PATH] [--clipboard] [--json]
       dctx map <map|find|neighbors|explain|read> <TARGET> [--json]
       dctx cache status|gc|purge|config|auto
       dctx template build <PATH> | template list
       dctx plugin search|install|list|info|enable|disable|remove|update|verify|run
       dctx git <status|protect>

Snapshot options:
  -c, --clipboard       Copy to clipboard
  -d, --diff            Output the diff only (if available)
  -t, --tree            Tree only (skips file content)
  -r, --reset           Force V1 reset

Filter options:
  --ext <list>          Only these extensions, e.g. rs,toml
  --binary              Include binary files (excluded by default)
  --empty | --non-empty Only empty / non-empty files
  --large <MB> | --small <MB>
  --hidden              Only hidden files and folders
  --debug-ignored       Show ignored paths without reading them
```

Run `dctx --help` for the complete reference and `dctx --version --verbose` for feature and precision limits.

---

## Use as a library

```toml
[dependencies]
dctx = { path = "crates/dctx" }
```

This path dependency assumes your Cargo project is at this repository's root; adjust it for another location. [`crates/dctx`](crates/dctx/) is the facade used by the CLI; the desktop uses [`crates/dctx-desktop-api`](crates/dctx-desktop-api/) on top of it. Python, Node and C bindings are planned on the same facade.

---

## Architecture

```
dctx/
├── crates/        # Rust workspace: pure core, mode engines, adapters, lexers, plugin host, facade, CLI
├── plugins/       # First-party plugins (dctx-plugin-git)
├── apps/          # dctx-desktop
├── legacy-c/      # The original C implementation (dircontxt)
├── docs/          # Specification, design records, contracts, formal models, guides
├── presentation/  # Slides and the dctx film
├── tools/         # Benchmarks, plugin tooling, test scripts
└── website/       # dctx.app
```

Start with [docs/SPEC.md](docs/SPEC.md), then [docs/CRATE-MAP.md](docs/CRATE-MAP.md).

---

## License

Apache License 2.0. Copyright Eris Margeta. Project code is licensed under Apache-2.0; third-party dependencies retain their own licenses. The desktop app is free, forever. dctx Enterprise is licensed per company (from €45,000 per year) and funds the project. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) and the [specification](docs/SPEC.md) first.

---

<p align="center">
  <sub>Context, engineered. Built in Rust.</sub>
</p>
