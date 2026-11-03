<p align="center">
  <img src="https://img.shields.io/badge/dctx-Context%2C%20engineered-8b5cf6?style=for-the-badge" alt="dctx">
</p>

<h1 align="center">dctx</h1>

<p align="center">
  <strong>The context engine for AI-assisted software development.</strong><br>
  Fewer tokens. Stable prefixes. Exact source. One policy for every workflow.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-1.1.0-8b5cf6" alt="Version">
  <a href="https://github.com/Eris-Margeta/dctx/actions/workflows/ci.yml?query=branch%3Amain">
    <img src="https://github.com/Eris-Margeta/dctx/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI">
  </a>
  <a href="https://github.com/Eris-Margeta/dctx/blob/main/LICENSE">
    <img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License">
  </a>
  <img src="https://img.shields.io/badge/rust-100%25-orange" alt="Rust">
  <a href="https://github.com/Eris-Margeta/dctx/blob/main/MAINTENANCE.md">
    <img src="https://img.shields.io/badge/commercial-maintenance%20available-8b5cf6" alt="Commercial maintenance available">
  </a>
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

Requires a recent stable Rust toolchain with Cargo (the workspace includes Rust 2024 crates). Linux x86_64 is the only tested platform for v1; macOS and Windows builds are untested.

### From source

```zsh
git clone https://github.com/Eris-Margeta/dctx
cd dctx
cargo build --locked --release -p dctx-cli
./target/release/dctx-cli --version   # dctx 1.1.0 (<git sha>)
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

`dctx pack` writes to stdout, the clipboard or `--output`; it creates a text export only when you request a file. Its tree lines carry the `ID` only, with no modification time and no size, so the page stays byte-identical across a bare `touch` and an edit keeps everything before the edited file as a shared prefix (what a prompt cache reuses); `--tree-stamps=on` brings back the 1.0.0 tree lines with `MOD:` and `SIZE:`. The legacy `dctx .` snapshot commands still write project text exports and cache state. Running without arguments opens an interactive terminal menu.

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
- **Languages**: every supported language is in one of three tiers (full, outline, docs); see [Language tiers](#language-tiers) below or run `dctx languages`.
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

### Language tiers

`dctx languages` prints this table (`--json` for tools, `--markdown` for this block) from the one registry that also drives `dctx --version --verbose`; a test fails when the block below differs from it.

- **full**: symbols and call edges. Call edges are lexical, not type-resolved: they come from names, scopes and imports, never from a type checker, and ambiguous references are dropped.
- **outline**: symbols only; no call edges are promised. Shell function bodies can yield same-file call guesses, but v1 does not promise shell call edges.
- **docs**: headed sections.
- **Confidence**: `guess` marks the heuristic outline families, and every record from those files carries confidence `guess`; `structural` means a dedicated lexer or deterministic document rules.

<!-- languages:begin -->
| Language | Tag | Files | Tier | Confidence |
|---|---|---|---|---|
| Rust | `rust` | `.rs` | full | structural |
| JavaScript | `javascript` | `.js`, `.mjs`, `.cjs`, `.jsx` | full | structural |
| TypeScript/TSX | `typescript` | `.ts`, `.mts`, `.cts`, `.tsx` | full | structural |
| Python | `python` | `.py`, `.pyi` | full | structural |
| Go | `go` | `.go` | full | structural |
| Java | `java` | `.java` | full | structural |
| C/C++ | `c` | `.c`, `.h`, `.cc`, `.cpp`, `.cxx`, `.hpp`, `.hh` | full | structural |
| C# | `csharp` | `.cs` | full | structural |
| Kotlin | `kotlin` | `.kt`, `.kts` | full | structural |
| Ruby | `ruby` | `.rb`, `.rake`, `.gemspec`, `Rakefile`, `Gemfile` | full | structural |
| PHP | `php` | `.php`, `.phtml`, `.inc` | full | structural |
| Swift | `swift` | `.swift` | full | structural |
| Vue/Svelte/Astro/HTML script blocks | `web` | `.vue`, `.svelte`, `.astro`, `.html`, `.htm` | full | structural |
| Scala | `scala` | `.scala`, `.sc` | full | guess |
| Dart | `dart` | `.dart` | full | guess |
| Zig | `zig` | `.zig` | full | guess |
| Solidity | `solidity` | `.sol` | full | guess |
| Gleam | `gleam` | `.gleam` | full | guess |
| Fortran | `fortran` | `.f90`, `.f95`, `.f03`, `.f08`, `.f`, `.for` | full | guess |
| Perl | `perl` | `.pl`, `.pm`, `.t`, `.psgi` | full | guess |
| Groovy | `groovy` | `.groovy`, `.gradle` | full | guess |
| PowerShell | `powershell` | `.ps1`, `.psm1`, `.psd1` | full | guess |
| Elixir | `elixir` | `.ex`, `.exs` | full | guess |
| Lua | `lua` | `.lua` | full | guess |
| OCaml | `ocaml` | `.ml`, `.mli` | full | guess |
| R | `r` | `.r` | full | guess |
| Julia | `julia` | `.jl` | full | guess |
| Haskell | `haskell` | `.hs` | full | guess |
| Erlang | `erlang` | `.erl`, `.hrl` | full | guess |
| Verilog/SystemVerilog | `verilog` | `.v`, `.vh`, `.sv`, `.svh` | full | guess |
| Terraform/HCL | `hcl` | `.tf`, `.tfvars`, `.hcl` | full | guess |
| SQL | `sql` | `.sql`, `*.sql.in` | full | guess |
| Clojure | `clojure` | `.clj`, `.cljs`, `.cljc`, `.edn` | full | guess |
| Common Lisp | `lisp` | `.lisp`, `.lsp`, `.cl` | full | guess |
| Scheme | `scheme` | `.scm`, `.ss` | full | guess |
| Emacs Lisp | `elisp` | `.el` | full | guess |
| JSON/JSONC/TOML/YAML | `config` | `.json`, `.jsonc`, `.toml`, `.yaml`, `.yml` | outline | structural |
| Shell | `shell` | `.sh`, `.bash`, `.zsh`, `.bats` | outline | guess |
| Makefile | `make` | `.mk`, `Makefile`, `GNUmakefile` | outline | guess |
| Dockerfile | `docker` | `Dockerfile`, `Containerfile` | outline | guess |
| Protobuf | `proto` | `.proto` | outline | guess |
| Pascal | `pascal` | `.pas`, `.pp`, `.dpr` | outline | guess |
| GraphQL | `graphql` | `.graphql`, `.gql` | outline | guess |
| Nix | `nix` | `.nix` | outline | guess |
| Markdown | `markdown` | `.md`, `.markdown` | docs | structural |
| Plain text | `text` | `.txt` | docs | structural |
| reStructuredText | `rst` | `.rst` | docs | structural |
| AsciiDoc | `asciidoc` | `.adoc`, `.asciidoc` | docs | structural |
| Org | `org` | `.org` | docs | structural |
| LaTeX | `tex` | `.tex` | docs | structural |
<!-- languages:end -->

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

Selection happens before any byte is read, hashed, parsed or cached, and is checked again before output leaves the engine.

On top of the rule files there is a strict built-in deny with no override (no flag, no `!` negation, not even `DCTX_INDEX_IGNORED=1`):

- **Sensitive files** are listed by name but never read: `.env`, `.env.*`, `*.env`, `.envrc`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.jks`, `*.keystore`, SSH private keys (`id_rsa`, `id_dsa`, `id_ecdsa`, `id_ed25519`), `credentials`, `credentials.json` and other data extensions, `*.credentials.json`, `secrets.yaml`/`.yml`/`.json`/`.toml`, `.npmrc`, `.netrc`, `.pypirc`, `.pgpass`, `.htpasswd`, `.git-credentials`. The full list is in [ADR-014](docs/ADR-014-Privacy-Guarantees-and-Ignore-Semantics.md).
- **`.git`** at any depth is never listed or read by the map verbs, and never read by `pack`, even when a rule says `!.git/`.
- **Symlinks** are never followed: the final component is opened with `O_NOFOLLOW`, and a symlinked directory is listed by name only, never entered or read through.

dctx decides by file name only; a secret pasted into an ordinary source file is not detected. The shipped `pack`, `dctx map` and `dctx mcp` path is guarded by `crates/dctx/tests/map_sensitive_files.rs`, `crates/dctx/tests/git_and_symlink_privacy.rs`, `crates/dctx/tests/map_env_files.rs`, the unit tests in `crates/dctx/src/adapters/fs.rs`, `crates/dctx/src/domain/policy.rs` and `crates/dctx-pack/src/snapshot.rs`, and an end-to-end check that runs every command against a git-tracked canary fixture (`lanes/DCX/checks/V1-1.zsh` in the project workspace).

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

The core pack/map workflows run locally, have **no telemetry**, **call no model** and **make no network calls**: the release check (`scripts/release-check.sh`) counts AF_INET/AF_INET6 connect/send calls under `strace -f` for every map verb, `pack`, `cache status` and `mcp`, and fails on any (it reports SKIP where strace cannot run). The current product CLI has no TLS network adapter for plugin discovery or downloads; network-capable distribution and plugin designs are documented separately. See the [Plugins guide](docs/guides/PLUGINS.md#what-is-not-yet-available) for current limits and [SECURITY.md](SECURITY.md) for private vulnerability reporting.

---

## Measured, in the open

<!-- bench:begin -->
<!-- Generated by scripts/render_readme.py from docs/research/bench-3way-20261030/results/run7-53f8b6b/aggregate.json and docs/research/bench-3way-20261030/tasks/results/run5-53f8b6b/tasks_aggregate.json. Edit the script, then run it; `just readme-check` fails on drift. -->

On our 70-question protocol (designed by the dctx team; questions, gold facts and scorer are in [`docs/research/bench-3way-20261030/tasks/`](docs/research/bench-3way-20261030/tasks/)), run 31 Oct 2026 against Trail-Code 0.28.0 and graphify 0.9.97 in code-only mode, dctx scored 140/140 using 29,340 tokens; Trail-Code 122 (88,923), graphify 96 (197,203). Tokens are estimated as stdout bytes / 3.5, rounded up, for every tool.

| Tool | Points /140 | Gold facts /354 | Tokens | Tool calls |
|---|---|---|---|---|
| dctx `53f8b6b` | 140 | 353 | 29,340 | 105 |
| Trail-Code 0.28.0 | 122 | 312 | 88,923 | 70 |
| graphify 0.9.97, code-only | 96 | 238 | 197,203 | 91 |

That protocol is ours, so it can favour dctx. An independently selected protocol (repositories picked by seed, callers gold from language servers, questions written without seeing dctx output) is pending: v1 card 6 publishes it, and its table will be added here whatever the result.

Cold index of the 6 benchmark corpora (flask, ripgrep, pocketbase, trailcode, mixed, docs; median of 3 runs) took dctx 0.13–1.20 s, against Trail-Code 1.04–52.83 s and graphify 4.59–59.56 s (docs: no index), on one Linux x86_64 runner (Linux 6.12.113+deb13-cloud-amd64, x86_64 CPU). Linux x86_64 is the only tested platform.

dctx makes no network calls: it made 0 socket calls in all 4 traced benchmark steps, and the release check (`scripts/release-check.sh`) asserts zero AF_INET/AF_INET6 connect/send calls under `strace -f` for every map verb, `pack`, `cache status` and `mcp`.

Raw results, corpora pins and scripts: [`docs/research/bench-3way-20261030/`](docs/research/bench-3way-20261030/); the write-up is [docs/BENCHMARK-20261030.md](docs/BENCHMARK-20261030.md) (Run 7).
<!-- bench:end -->

An earlier two-way study against Trail-Code (October 2026, dctx `17dff0b`, before the call edges measured above) is in [docs/research/trail-code-2026-10](docs/research/trail-code-2026-10/REPORT.md). See also the [changelog](CHANGELOG.md).

---

## Out of scope for v1

- Any network or cloud feature.
- macOS and Windows support claims: they are untested; Linux x86_64 is the only tested platform.
- Watch or incremental mode beyond re-indexing.
- Shell call edges (shell is outline tier: symbols only).
- Embeddings or semantic search.
- LSP-grade type resolution: call edges are lexical.
- A stable plugin ABI or desktop API: neither is stable in v1.
- Cross-repository queries.
- Publishing to crates.io or GitHub releases (an owner decision).

---

## Open source and maintenance

dctx is open source under the [Apache-2.0](LICENSE) license, which makes it free for any use, including commercial use. The core (`pack`) and every modality stay open, and no feature is held back behind a paid edition. Companies that depend on dctx can fund its upkeep through a [maintenance agreement](MAINTENANCE.md), which covers security response times, long-term support and a written commitment for procurement, or through [GitHub Sponsors](https://github.com/sponsors/Eris-Margeta). Paying never changes the license and never closes a feature. The "dctx" name and logo are trademarks of Eris Margeta; see [TRADEMARK.md](TRADEMARK.md) for what forks may call themselves.

Project code is open source (Apache-2.0) and **ongoing work**; third-party dependencies retain their own licenses. Parts of the current development line use third-party crates; anything with third-party dependencies is not considered enterprise-ready.

The dependency policy retains the **Ed25519 signature verifier** (`ed25519-dalek`) for plugin/index verification. Signed organization policy bundles are planned; their presence in the specification does not mean the CLI can enroll or verify them today. See the [dependency policy](docs/SPEC.md#27-dependencies-and-enterprise-readiness).

---

## CLI Reference

```
Usage: dctx [PATH] [OPTIONS]
       dctx pack [PATH] [--output PATH] [--clipboard] [--json] [--tree-stamps=on|off]
       dctx pack [PATH] --for <TEXT> [--budget TOKENS]
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
  --tree-stamps=on|off  MOD: and SIZE: on tree lines (default off; on = the 1.0.0 text)

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

Apache License 2.0. Copyright Eris Margeta. Project code is licensed under Apache-2.0; third-party dependencies retain their own licenses. The desktop app is free, forever. Paid maintenance agreements fund the project; see [MAINTENANCE.md](MAINTENANCE.md). See [LICENSE](LICENSE) and [NOTICE](NOTICE).

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) and the [specification](docs/SPEC.md) first.

---

<p align="center">
  <sub>Context, engineered. Built in Rust.</sub>
</p>
