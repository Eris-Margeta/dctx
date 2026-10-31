# Changelog

All notable changes to dctx are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/). Dates are ISO 8601.

The open repository is ongoing work. Anything that still depends on third-party
crates is not considered enterprise-ready; enterprise-ready builds are part of
dctx Enterprise.

## [1.0.0] — 2026-10-31

The first v1 release, folded from the 0.26 development line. Every `crates/*` and
`plugins/*` package, `VERSION`, `project-meta.yaml` and the README badge are at **1.0.0**
(`tools/sync-version.sh --check` keeps them and their `Cargo.lock` entries together). The
desktop app keeps its own version line (0.11.0) and its own release workflow.

### Shipped surface
- **Map verbs**: `dctx map map | find | neighbors | explain | read | skeleton | grep | hubs | path | export`,
  with hard token budgets, stable ids, cursor paging and precision labels; call and import edges are lexical.
- **`dctx mcp`**: a stdio MCP server exposing the map verbs as tools.
- **`dctx pack`**: the whole project as one structured block to stdout, the clipboard or `--output`.
- **`dctx languages`**: one registry with three tiers (full, outline, docs) that also renders the README table.
- **Privacy deny**: sensitive files (env files, keys, credentials, ...), `.git` at any depth and symlinks are
  never read or printed by `pack`, any map verb or `mcp`; there is no override flag. Detection is by file name only.
- **Determinism**: two fresh indexes give byte-identical answers, and an unrelated edit changes only `hubs`;
  `scripts/release-check.sh` gates both, and counts network syscalls under `strace -f` (must be zero).
- **`dctx --version`** prints `dctx 1.0.0 (<git sha>)`; the sha is embedded at build time, `unknown` without git.
- **README benchmark section** rendered from the committed run 7 JSON by `scripts/render_readme.py`;
  `just readme-check` fails when the README differs.

### Out of scope for v1
- Any network or cloud feature.
- macOS and Windows support claims (untested); Linux x86_64 is the only tested platform.
- Watch or incremental mode beyond re-indexing.
- Perl, Fortran and Verilog.
- Shell call edges.
- Embeddings and semantic search.
- LSP-grade type resolution.
- A stable plugin ABI and desktop API (unstable, not a v1 contract).
- Cross-repository queries.
- Publishing to crates.io or GitHub (an owner decision).

### Added
- **Call and import edges** in the map index: `dctx map neighbors --edge calls|imports`
  now answers who calls what, resolved by lexical scope, imports and module paths, with
  `name`/`guess` confidence; ambiguous references are dropped.
- **New map verbs**: `skeleton <file>` (declaration signatures, no bodies), `grep <literal>`
  (hits grouped by enclosing symbol), `hubs` (most-called symbols and per-directory counts),
  `path <FROM> <TO>` (shortest edge path) and `export --format json|mermaid` (the whole graph).
  `neighbors --depth` goes up to 4 (breadth-first, each node once).
- **`dctx mcp`**: a stdio MCP server exposing the map verbs as tools.
- **`dctx languages`** (`--json`, `--markdown`): every supported language with its tag, file names, tier
  (full: symbols and lexical call edges; outline: symbols only; docs: sections) and confidence, from one facade
  registry (`dctx::LANGUAGES`) that also feeds `--version --verbose` and the README table, which a test diffs.
- **A CLI exit-code contract** (`docs/CLI-EXIT-CODES.md`) for `map`, `pack`, `mcp`, `languages` and unknown
  commands: 0 ok, 1 no result (an id not in the index, a binary file target, nothing to pack), 2 usage, 3 index
  error. Every failure prints a `hint:` line. Previously every failure exited 1. A non-UTF-8 argument is now a
  usage error instead of a panic, an unknown flag is named as unknown, and `dctx map <verb> --help` prints a page
  with an `Example:` line for each map verb.
- **Outline symbols (`guess` confidence) for 25 more formats**: Scala, Dart, Zig, Solidity,
  Gleam, Groovy; Elixir, Lua, OCaml, R, Julia, Haskell; Terraform/HCL, SQL, shell, Makefile,
  Dockerfile, protobuf, Pascal, GraphQL; and headed sections for `.txt`, `.rst`, `.adoc`,
  `.org` and `.tex` documents.
- **A three-way benchmark against graphify and Trail-Code** (`docs/BENCHMARK-20261030.md`)
  and the gap matrix behind it (`docs/GAP-MATRIX-20261030.md`).

- **dctx Desktop is free, forever** for individuals, teams and companies
  ([announcement](https://dctx.app/news/desktop-free-forever/)).
- **The dctx Context Management Specification** (`docs/SPEC.md`): prompt-processing
  efficiency as the core belief, the ten RAG defect classes and how dctx rules
  them out, the seven-layer organization policy, prefix-stable tiers and delta
  answers, the persistent index, the plugin boundary and the roadmap.
- **An isolated, reproducible benchmark against Trail-Code**
  (`docs/research/trail-code-2026-10/`): methodology, raw data, scripts and
  numbered findings.
- **Map mode** (`dctx map`): `map`, `find`, `neighbors`, `explain` and `read`
  with hard token budgets (up to 2,000 per answer), cursor paging, stable ids,
  explicit `moved`/`gone`/`ambiguous` outcomes and precision labels on every answer.
- First-party structural extractors for JavaScript, TypeScript/TSX, Python, Go,
  Java, C/C++, C#, Kotlin, Ruby, PHP, Swift, Markdown and JSON/TOML/YAML; Rust
  through `syn`.
- A verified map generation reused across CLI invocations.
- **Plugin host**: capability grants, consent, receipts, liveness by progress,
  signed distribution verified with Ed25519, and Linux confinement; the bundled
  **Git plugin** (`.gitignore` protection and consented staging).
- **Cache lifecycle**: time-based expiry, a system size cap and a bounded
  automatic cleaner (`dctx cache status | gc | purge | config | auto`).
- `dctx pack`: an explicit pack to stdout, the clipboard or `--output`, without
  leaving files in the project.
- The template system's v2 compiler (a sealed plan IR) and organization ceilings.
- `crates/dctx`: one library facade for the CLI, the desktop app and future
  bindings.
- A new website with changelog and news, and the dctx film
  (`presentation/video/`).

### Changed
- **Byte-stable map answers**: the text header is `# <place> <verb>` and JSON no longer
  carries `snapshot`, `generation` or `freshness.age_s`, so answers stay identical across
  unrelated edits (prompt-cache friendly). `--meta` restores the old fields.
- **`dctx pack` never prints env-shaped files** (`.env`, `.env.*`, `*.env`, `.envrc`); they
  stay in the tree by name with their content omitted.
- **Strict sensitive-file deny for every command**: besides env files, `*.pem`, `*.key`,
  `*.p12`, `*.pfx`, `*.jks`, `*.keystore`, SSH private keys (`id_rsa`, `id_ed25519`, ...),
  `credentials`, `credentials.json`, `secrets.yaml` (and other data extensions), `.npmrc`,
  `.netrc`, `.pypirc`, `.pgpass`, `.htpasswd` and `.git-credentials` are never read or printed
  by `pack`, any `dctx map` verb (by id or raw path, with or without `--in`) or `dctx mcp`.
  There is no override flag (see ADR-014).
- **`.git` is closed for good**: a `.git` segment at any depth is never listed or read by the map
  verbs and never read by `pack`, even when `.gitignore` or `.dctxignore` says `!.git/`.
- **No symlink escapes**: `FsSource` opens files with `O_NOFOLLOW`, requires a regular file
  handle, and refuses symlinked directories, so a symlink swapped in after the stat cannot leak
  bytes from outside the root.
- `DCTX_INDEX_IGNORED=1` (ignore files are not applied) is now documented in ADR-014; the
  sensitive-file deny, the `.git` guard and the symlink rules still apply when it is set.
- **Total-source ceiling raised from 64 MiB to 1 GiB**; `DCTX_MAX_SOURCE_BYTES` overrides it.
- The core never reads `.git/`, never spawns `git` and never uses VCS object ids;
  content identity is dctx's own SHA-256 (`crates/dctx-hash`).
- Repository ignore rules are applied before any read on the map path, and
  `--lines A-B` returns exact line ranges.
- Everything in the repository is licensed under Apache-2.0.

### Fixed
- Node spans resolve from one newline index per file instead of rescanning the
  source for every symbol (cold `find` on a 3,000-file repository: 141 s → about 6 s).
- Queries share one generation instead of cloning it (peak memory down by about
  30 % on the same repository).
- One hostile file no longer fails every map verb for the whole repository: a Rust
  file that is not UTF-8 is indexed as a module with no symbols (like every other
  language), and a file name the confined path cannot carry (a backslash) is skipped
  and marks the snapshot `partial`, as `pack` already did.

### Known issues
Measured in the benchmarks and scheduled in the specification's roadmap:
- No incremental indexing: the first query after an edit re-indexes, and each
  edit adds a full generation to the cache.
- Outline-tier formats carry symbols and `contains` edges only; heuristic families are
  labelled `guess`. `dctx languages` lists the tier and confidence of every format.
- The 70-question task protocol (run 7: 140/140) was designed by the dctx team; an
  independently selected protocol (v1 card 6) is pending.

## [0.25.0] — 2026-05-29

The last release of the first Rust generation, with **zero third-party
dependencies** in the core and the CLI.

### Added
- Snapshot, diff and tree modes; clipboard output; an interactive TUI.
- Filters by extension, size, content and visibility; binary detection.
- Ahead-of-time compiled output templates.
- The privacy guarantee: ignored files are never read, not even in debug mode;
  omit rules list a file without reading its content.
- The desktop app (Tauri) on the shared core.

## [0.11.0] — 2026-01-26

### Added
- A redesigned desktop interface with light and dark modes.
- A sequential, fail-fast CI pipeline.

## [0.10.0] — 2026-01-25

### Added
- Advanced CLI filters and their documentation.

## [0.1.0] — 2026-01-08

### Added
- The Rust rewrite of `dircontxt` begins.

## dircontxt [1.0.0] — 2025-11-15

The original tool, written in pure C (`legacy-c/`).

### Added
- One LLM-ready `.llmcontext.txt` snapshot with a manifest and unique ids for
  every file and directory, plus a binary state file.
- Automatic versioning with diffs that carry only the changed files.
- A three-tier ignore system (built-in defaults, a global ignore file and a
  per-project `.dircontxtignore`) and a global configuration file.
