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
  <a href="https://github.com/Eris-Margeta/dctx/actions/workflows/ci.yml">
    <img src="https://github.com/Eris-Margeta/dctx/actions/workflows/ci.yml/badge.svg" alt="CI">
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

dctx exists to send **the fewest, most relevant bytes, in an order that stays the same between turns**, so prompts are smaller, caches hit, and the model reasons over exact source instead of guesses. That is the whole philosophy, and every feature has to earn its place against it. The [specification](docs/SPEC.md#2-the-core-belief-prompt-processing-efficiency) turns it into five testable constraints — bounded, deterministic, prefix-stable, non-redundant, honest — and seven metrics.

### One engine for every way software is built

| You work by… | dctx gives you |
|---|---|
| **Hand-coding** in an unfamiliar codebase | `dctx map` outlines, symbol search and exact source reads |
| **Copy-paste** into a chat assistant | `dctx pack` — the whole project or only what changed, in one structured block |
| **Tab completion** and IDE assistants | A library facade with millisecond reads for the editor's own context budget |
| **Agents** in a terminal | Bounded, copyable, deterministic answers: five verbs, one record per line |
| **Fully automated software factories** | One signed organization policy, receipts, no network, byte-identical outputs |

### Zero RAG defects

Chunk retrieval over embeddings fails in predictable ways: it cuts functions in half, returns different chunks for the same question, serves stale code, leaks ignored files, and hides how much it cost. dctx does not use embeddings. It answers from a lexical graph of syntax-owned spans with explicit confidence, verifies bytes before it emits them, and labels every answer with its precision and freshness. The [spec](docs/SPEC.md#3-zero-rag-defects) names ten defect classes and the mechanism and test that rule out each one.

---

## Where it comes from

dctx started in **2025 as `dircontxt`, written in pure C** ([`legacy-c/`](legacy-c/)), to solve the context problem while we were still moving to an AI workflow: one command that gave a model the whole project — structured, versioned, with diffs and a strict ignore system — instead of pasting forty files. It was rewritten in Rust in 2026 and grew a privacy-first core, a template system, a plugin host, a desktop app and map mode for agents. The goal has not changed: **context, engineered.**

---

## Installation

### From source

```bash
git clone https://github.com/Eris-Margeta/dctx
cd dctx
cargo build --release -p dctx-cli
./target/release/dctx-cli --version
```

### Using Cargo

```bash
cargo install --path crates/dctx-cli
```

---

## Quick Start

```bash
dctx pack .            # whole project as one structured context block on stdout
dctx pack . --clipboard  # … straight to the clipboard
dctx . -d -c           # only what changed since the last snapshot
dctx . -t -c           # tree only, no file contents
dctx                   # interactive menu
```

`dctx pack` writes to stdout, the clipboard or `--output`. It never leaves a text file in your project.

---

## Map mode: context for agents

```bash
dctx map map                       # outline the project
dctx map find "SnapshotConfig"     # search names and concepts
dctx map neighbors s:src/lib.rs#Session --edge calls --dir in
dctx map explain s:src/lib.rs#Session
dctx map read s:src/lib.rs#Session # exact, byte-verified source
```

- **Bounded**: every answer has a token budget (up to 2,000) and never splits a record.
- **Deterministic**: the same tree and question give the same bytes, so prompt caches hit.
- **Honest**: every answer carries precision (`structural`, `syntax`, `opaque`) and freshness.
- **15 language families**: Rust, JavaScript, TypeScript/TSX, Python, Go, Java, C/C++, C#, Kotlin, Ruby, PHP, Swift, Markdown, and JSON/TOML/YAML configuration.
- **No Git, no network**: identity comes from dctx's own content digest; the core never reads `.git/`.

Details: [Map mode guide](docs/guides/MAP-MODE.md). Planned next — an MCP server, agent hooks, blast radius for a diff, delta answers and prefix-stable context tiers — is specified in [docs/SPEC.md](docs/SPEC.md#31-roadmap-and-acceptance-gates).

---

## Organization control

dctx is configured by one language, the dctx Template System (DTS). The same file format that defines output templates also controls selection, budgets, parsers, cache limits, plugins and privacy for every mode. The specification extends it to a **seven-layer, signed policy lattice** — built-in → organization → tenant → machine → user → project → session — where each layer can only narrow the one above it, and `dctx policy explain` shows which layer set each value. See [Part IV of the spec](docs/SPEC.md#part-iv--policy).

---

## Plugins

Anything with an outside effect is a plugin: out of process, consent-gated, receipted and signed. The bundled Git plugin protects `.gitignore` and can stage files when you allow it. See the [Plugins guide](docs/guides/PLUGINS.md).

```bash
dctx plugin list
dctx git status
```

---

## Security & Privacy

**dctx never reads ignored files — not into memory, not in debug mode.**

| Rule file | Behaviour |
|---|---|
| `.gitignore`, `.dctxignore`, global ignore | The path does not exist for dctx: never opened, never listed, never recursed |
| `.dctxomitcontent` | Listed in the tree; content replaced with `[Content omitted]` and never read |

Selection happens before any byte is read, hashed, parsed or cached, and is checked again before output leaves the engine. Read-spy tests enforce it (`crates/dctx-adapter-fs/tests/privacy.rs`, `crates/dctx-map-query/tests/source_privacy.rs`).

```bash
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

dctx has **no telemetry**, makes **no network requests** and **calls no model**.

---

## Measured, in the open

We benchmarked dctx against Trail-Code on a dedicated machine with pinned versions, no network during runs and an adversarial corpus. dctx led on cold indexing (0.09–6.05 s vs 2.4–95 s, 1.6–27× less memory), footprint (one 9.2 MB binary vs 405.6 MB of Node.js packages), answer bounds and network privacy. Trail-Code led on call graphs, answers to agent questions and stability under edits. The report, raw data and scripts are in [docs/research/trail-code-2026-10](docs/research/trail-code-2026-10/REPORT.md); the fixes are on the [roadmap](docs/SPEC.md#31-roadmap-and-acceptance-gates). See also the [changelog](CHANGELOG.md).

---

## Open source and Enterprise

Everything in this repository is open source (Apache-2.0) and **ongoing work**. Parts of the current development line use third-party crates; anything with third-party dependencies is not considered enterprise-ready.

The one third-party component we keep on purpose is the **Ed25519 signature verifier** (`ed25519-dalek`), a vetted, widely audited security crate. It is there for **security and trust**: it verifies the signatures on plugins and policy bundles, and we will not replace audited cryptography with something less proven.

**dctx Enterprise** — enterprise-ready builds, organization and tenant policy control, and direct support — is licensed per company, **from €45,000 per year**.

---

## CLI Reference

```
Usage: dctx [PATH] [OPTIONS]
       dctx pack [PATH] [--output PATH] [--clipboard] [--json]
       dctx map <map|find|neighbors|explain|read> [--json]
       dctx cache status|gc|purge|config|auto
       dctx template build <PATH> | template list
       dctx plugin search|install|list|info|enable|disable|remove|update|verify
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

`crates/dctx` is the stable facade used by the CLI and the desktop app. Python, Node and C bindings are planned on the same facade.

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

Apache License 2.0. Copyright Eris Margeta. Everything in this repository is Apache-2.0. The desktop app is free, forever. dctx Enterprise is licensed per company (from €45,000 per year) and funds the project. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) and the [specification](docs/SPEC.md) first.

---

<p align="center">
  <sub>Context, engineered. Built in Rust.</sub>
</p>
