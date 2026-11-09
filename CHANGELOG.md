<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Changelog

All notable changes to dctx are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/). Dates are ISO 8601.

For current functionality and limitations, see the [README](README.md).
Historical entries describe the behavior and support scope of that release.
Current licensing: all first-party code and documentation, including the desktop app
and planned GPUI rebuild, are Apache-2.0, copyright Eris Margeta. Historical license
and distribution plans below do not describe the current release.

## [Unreleased]

## [1.3.0] - 2026-11-09

Install from the GitHub release archives. The npm, crates.io and PyPI packages and
the Go module are built and tested in CI, but registry publication is still pending;
until it happens, the release archives are the install path.

### Added

- `dctx rpc`: a versioned, newline-delimited JSON engine protocol over stdin/stdout
  (protocol 1.0.0, schema in `docs/protocol/`). Every reply carries `engine_version`;
  operation output is exactly the CLI stdout bytes.
- `dctx context TARGET...`: minimal ranked context for a file, symbol name or stable
  node id, with chunk ids, `--budget`, `--json` and `--session ID --dedupe`.
  `pack --for TEXT --json` now returns the same JSON context page.
- `dctx doctor [--json] [--probe-project]`: lists the dctx binaries visible from the
  current folder with path, ownership scope, version, build commit, SHA-256 and cache
  sizes. It changes nothing and uses no network.
- MCP `--profile lean|three` and `--tools NAME,...` select a smaller tool set with
  compact descriptions (lean start-up tool definitions: 6,278 -> 1,637 bytes). See
  [docs/guides/MCP.md](docs/guides/MCP.md).
- Python, Node (npm) and Go client packages over `dctx rpc`, each with CLI byte-parity
  tests in CI. They are not published to a registry yet.
- Release supply chain: keyless cosign signatures, CycloneDX SBOMs, GitHub build
  provenance attestations, a reproducible Linux musl build check and offline
  (air-gap) bundles. See [docs/VERIFYING.md](docs/VERIFYING.md). CI runs `cargo audit`.
  SECURITY.md and SUPPORT.md describe the security and support policy.
- Robustness tests and a fuzz smoke workflow in CI.
- `dctx pack --for` prints a warning on stderr when the text names no identifiers;
  the page is unchanged.
- `dctx pack --clipboard` supports `xsel` on Linux.
- Experimental `tools/catalog-bench` for huge-tree catalog measurements (not part of
  the CLI).

### Changed

- `dctx pack` prints its `pack bytes=… files=… retention=… version=V1` status line on
  stderr. stdout is exactly the page and ends in a newline; `pack --json` stdout is
  exactly one JSON document. Scripts that read the status line from stdout must read
  stderr instead.
- Indexes live in an engine `major.minor` subdirectory of the cache parent
  (`DCTX_CACHE_DIR` or the platform `dctx/v2` root). 1.3 builds its own index on first
  use; patch releases share it, and older unscoped cache data is ignored, not deleted.
- Faster warm path: one fused inventory walk with directory-relative stats, rule files
  validated by stamps and cheaper decoding (measured checkout 288 -> 94 ms, 4k-file
  tree 88 -> 32 ms). Timestamp-only changes reuse the index generation.
- Each place keeps its own warm index; targeted queries no longer admit the whole
  place first; capacity errors name the exact limit and the flag that changes it.
- Pack output streams with bounded memory and spools large pages. Pack history uses
  compact manifest caches.
- Language extraction audit with new goldens; the default parser id is now
  `dctx-structural/3`.
- `dctx pack --for` keeps bare short names such as `main` and `run`, and keeps the
  case of capitalized words. Exact-case names and non-test code rank first, so
  `--for Session` finds the `Session` types before test functions.
- `dctx cache`, `dctx template`, `dctx context` and a bare `dctx` follow the exit-code
  contract in [docs/CLI-EXIT-CODES.md](docs/CLI-EXIT-CODES.md): 2 for usage errors,
  3 for I/O errors (naming the path), each with a `hint:` line. RPC `cache_status`
  returns the same codes. `dctx context` with no TARGET exits 2.
- A bare `dctx` whose stdin or stdout is not a terminal prints short usage and exits 2
  instead of starting the interactive menu.
- `dctx pack --help` prints only the pack usage and flags.
- `dctx plugin …` and `dctx git` text output is plain text. Refusals go to stderr as
  `❌ Error:` with a `hint:` line; a missing consent names
  `dctx plugin enable <id> --approve`. JSON output and exit codes are unchanged.
- `dctx cache auto` text shows a UTC time next to `last_auto_clean_ms` and
  `next_due_ms`; JSON is unchanged.
- Docs and package READMEs no longer present `npx dctx`, `npm install`,
  `cargo install dctx-cli`, `pip install dctx` or `go get` as working install paths.
  The Python missing-engine message no longer suggests a PyPI install.
- Bundled SQLite is 3.54.1 (rusqlite 0.41).

### Removed

- `dctx plugin run` and `dctx git` JSON replies no longer repeat the
  `facade_spawns` and `facade_git_children` keys. The facade counters remain once in
  `--json` output and are gone from text output.
- The cwd-relative plugin root fallback used when HOME was unset (see Security).

### Fixed

- Pack headers show `ID="UNIQUE_ID"` with clean quotes in the file-access
  instructions, including stamped and tree-only output. This changes pack output bytes.
- Pack rendering preserves literal template syntax in file bodies and paths, including
  backslashes, for default, stamped and compiled templates. Variable substitution
  applies only to template structure; `pack --for` also preserves source CRLF bytes.
- The last `</FILE_CONTENT_END …>` marker of a pack page is no longer glued to the
  status line.
- `dctx pack --for` with no usable terms says so and suggests backticks, instead of
  reporting that no name matched the index. Its flag refusal now lists `--json`.
- `dctx map` call edges stay within the caller's language family: a Rust `.all(..)`
  no longer resolves to a Makefile target. A guessed method call such as
  `iter.map(..)` links to a project method only when the caller can see that method's
  type. Hubs, neighbors, path and `pack --for` lose these false edges.
- `dctx plugin search` and the other text-mode plugin commands no longer mix JSON
  fields or internal counters (`facade_git= spawns= git_children=`) into the output;
  `dctx git status` without consent no longer prints `git: ConsentRequired`.
- `dctx pack --clipboard` with no clipboard tool names the tools it tried and suggests
  `--output FILE`.
- A symlinked or non-regular `.gitignore` or `.dctxignore` inside the project is
  skipped with a warning instead of failing the command; rule-file errors no longer
  print the absolute project path.
- Cache policy saves no longer get stuck on a `policy.v1.tmp` left by a crash.
- `dctx doctor` shows each binary's build commit (`commit:` in text, `"commit"` in
  JSON) and names an invalid argument.
- `dctx --help` lists `map export`; the `dctx map --help` examples map a file that has
  symbols.
- The Python wrapper raises `BinaryNotFoundError`, not `RuntimeError`, when no home
  directory is available (Windows without `USERPROFILE`/`LOCALAPPDATA`).
- Windows: durable file replacement, inode identity and racy-window directory stamps
  in map refresh. macOS: symlinked ancestors of the project, config and cache roots
  (such as `/var` and `/tmp`) are resolved at the boundary while in-tree symlinks stay
  refused; file names that APFS refuses are handled. Other cross-platform test and
  code fixes.
- README CLI reference, map-mode guide, modalities, start, testing and exit-code docs
  match the current binary and CI: all commands, 50 languages, neighbors depth 1..4,
  exact `--lines`, the global ignore file path, and that `.git/info/exclude` and
  `core.excludesFile` are not read.
- `tools/sync-version.sh` also syncs the Python, Go and npm package versions and the
  release links in the docs and package READMEs.

### Security

- Bundled SQLite upgraded to 3.54.1 (rusqlite 0.41, libsqlite3-sys 0.39.1) for
  RUSTSEC-2026-0459.
- Hardening across the engine and CLI, documented in
  [docs/SECURITY-MODEL.md](docs/SECURITY-MODEL.md).
- Pack and diff output entity-encode `&`, `<`, `>`, `"` and control or line-separator
  characters in file paths, so a file name cannot forge file boundary markers.
  Ordinary paths are unchanged and output stays byte-stable.
- `dctx doctor` no longer executes dctx binaries found in `./node_modules/.bin`,
  `$VIRTUAL_ENV` or relative PATH entries, which an untrusted checkout can plant; they
  are listed with path and SHA-256. `--probe-project` opts in.
- MCP tool arguments are passed after `--` and taken literally, so a flag-shaped id or
  query (for example `--place`) cannot change the place outside the server root. MCP
  never forwards `--place` or `--output`.
- `pack --output`, `map export --output` and `template build --output` never follow a
  symlink or write into a non-regular target; pack and template write a fresh sibling
  file and rename it into place.
- Clipboard tools are looked up only in absolute PATH entries, so `.` or empty entries
  cannot run a repository's own `xclip` or `wl-copy`.
- The source reader checks an entry's type before opening it and never opens device
  nodes, FIFOs or sockets (Unix opens use `O_NOCTTY`). `StdFs` metadata, listings and
  link reads no longer follow symlinked path components; Windows ancestor guards block
  directory replacement.
- The store adapter refuses symlinked, foreign-owned or group/other-writable store
  roots and creates store directories with mode 0700.
- The plugin root no longer falls back to a directory relative to the working
  directory when HOME is unset; plugin commands refuse unless HOME or
  `DCTX_PLUGIN_ROOT` is an absolute path.
- The Python wrapper loads its `dctx_binary_<platform>` engine package only from the
  install directory that holds `dctx`, never from the working directory or other
  `sys.path` entries.
- The Python, Node and Go package READMEs warn that operation arguments carry full CLI
  authority.
- The no-follow open flag used by `map export --output`, the map store and the desktop
  snapshot rules now comes from the platform instead of a hard-coded x86 value, so it
  also holds on Linux ARM64.
- The npm wrapper tarball built in CI no longer lists the unpublished `@dctx/cli-*`
  platform packages, so installing it never resolves those names from the public
  registry; error messages point to the release archive and `DCTX_BIN`.
- The website playground rate limit can no longer be bypassed with a forged
  `X-Forwarded-For` header. The website deploy builds without secrets and deploys only
  from `main` through a protected environment; the desktop release workflow is
  disabled until the desktop app ships, with pinned toolchains, a frozen lockfile and
  provenance attestations.

## [1.2.0] - 2026-11-07

### Added

- Prebuilt release archives for Linux x64/ARM64 (static musl), macOS x64/ARM64 and Windows x64, with SHA256SUMS.
- Packaging support for npm (`npx dctx`, platform packages under `@dctx/cli-*`) and crates.io (`cargo install dctx-cli`); registry publication is coming soon. Install the prebuilt GitHub release archives now.
- Cross-platform CI: every push and pull request runs the full test suite on Linux, macOS (ARM64 and x64)
  and Windows, on stable Rust and the minimum supported version. See [docs/TESTING.md](docs/TESTING.md).
- The website redeploys automatically from `main`.
- GitHub Sponsors: individuals can support dctx at <https://github.com/sponsors/Eris-Margeta>.

### Changed

- Minimum supported Rust version is now declared: 1.91.
- The source tree is formatted with `cargo fmt`; no behaviour change.

### Removed

- The two internal pre-1.0 project rule files. Project ignore inputs are
  `.gitignore` and `.dctxignore`; built-in and explicit/global omission rules remain.
- The implicit `dctx PATH` snapshot command and its top-level flags, including
  `-d`/`--diff`, `-t`/`--tree` and `-r`/`--reset`. Use `dctx pack PATH`.
  The TUI now uses explicit pack commands without the old registry, state or project exports.
- `dctx cache gc --legacy` and its export-selection flags. Cache collection uses the managed ledger.
- Legacy-named selection test paths; current pure selection and privacy coverage remains.

### Fixed

- Windows CLI test environments now map `APPDATA` to the fixture config directory and provide `LOCALAPPDATA` for cache/data; missing or empty Windows directory variables return clear errors.
- Rust parser subprocess startup failures now stop indexing with a clear error instead of silently degrading source spans.
- `pack --for` keeps each section in source order when its budget runs out and recognizes file targets such as `src/lib.rs`.
- `serve` (including `serve --mcp`) uses the stdio MCP server in the current directory, including fixture folders; `read` routes to `map read`.
- Budget-cut reads move trailing closing delimiters and separators into the continuation, preserving every source line.
- The release recipe checks the current root `CHANGELOG.md` path.

## [1.1.0] - 2026-11-06

Release **1.1.0** (`v1.1.0`; release candidate `v1.1.0-rc1` 2026-11-04). It folds in the 1.0.1 line, which was never released on its own.

### Since rc1 (docs, CI, tooling and SPDX licence headers only; no change to dctx output)

- **Docs**: byte stability re-measured on 2026-11-06 — 33/33 text and 33/33 JSON map answers identical
  after unrelated edits and over five runs (see docs/research/stability-20261105/REPORT.md);
  SPEC and README status texts updated, historical 0/63 kept.
- **Formal**: the plugin TLC gate accepts the newer TLC wording 'Temporal property X was violated.' /
  'Invariant X was violated.' (all 8 baselines, 59 mutants, 37 witnesses pass); host-specific
  tla2tools.jar paths removed; TLC trace files ignored.
- **Self-hosted CI** (`.github/workflows/ci.yml`): the job run step by step on a self-hosted Linux runner (16 cores) from a clean
  `git archive` of the commit; results and badge recorded in `docs/ci/RESULTS.md` — 2279 run, 2279 passed, 0 skipped.
- **Third-party npm licence inventory**: `scripts/npm_licenses.py` indexes every repository `package-lock.json` into
  `THIRD_PARTY_NPM.md`; `npm_licenses.py --check` runs in CI and fails on a disallowed licence or a stale report.

### Added

- **Perl** (`.pl`, `.pm`, `.t`, `.psgi`): `package A::B;` and `package A { .. }` as `type` symbols holding every later
  `sub` (named `A::B::name`) as `fn` symbols (`guess` confidence, full tier). POD is masked like a comment; call edges
  come from `name(..)`, bareword calls with an argument and `&name`, never after `->`, `sub` or a sigil.
- **Fortran** (`.f90`, `.f95`, `.f03`, `.f08`, `.f`, `.for`): `module`, derived `type` and named `interface` as `type`
  symbols; `program`, `function` and `subroutine` as `fn` symbols named `module::name` (`guess` confidence, full tier).
  Keywords are case-insensitive, `!` comments only, fixed-form column rules are not modelled; call edges skip
  intrinsic and control keywords that precede `(`.
- **Verilog/SystemVerilog** (`.v`, `.vh`, `.sv`, `.svh`): `module`, `macromodule`, `interface`, `package` and `class`
  declarations as `type` outline symbols and `function`/`task` declarations as `fn` symbols (`guess` confidence, full tier),
  with module-scope call edges and `<module> <instance> (` instantiation links resolved through the call index.
- **Prefix-stable `pack` output**: tree lines carry the `ID` only, with no `MOD:` and no `SIZE:`, so a
  bare `touch` leaves the page byte-identical and an edit keeps everything before the edited file as a shared prefix
  for prompt caches. `--tree-stamps=on|off` (default `off`) selects the built-in tree template; `on` reproduces the
  1.0.0 bytes. The desktop tree-only export keeps its digest line last, behind a stable lead line.
- **Stat stamps in the manifest**: a stamp pass runs before the digest, with a racy-timestamp guard; a
  touch-only change is decided by content. A gate replays 200 random edits against a cold rebuild.
- **Record and entry caps**: checked during the scan and set by `DCTX_MAX_RECORDS` and
  `DCTX_MAX_ENTRIES`; the cache is reclaimed on publish, evicts before refusing, and readers share one lock.
- **Open-source maintenance model**: `MAINTENANCE.md`, `TRADEMARK.md`, a DCO in
  `CONTRIBUTING.md` and `FUNDING.yml`; the desktop app is open source too.

### Changed

- **Id lookup** uses a sorted hash index instead of a linear node scan.
- Large source files were split into submodules by pure moves (400-line rule): `pack.rs`, `path.rs`, `reuse.rs`,
  `bound.rs`, `plugin.rs`, `yaml.rs` and others. No behaviour change.
- The version is **1.1.0** for every `crates/*` and `plugins/*` package; the desktop app stays on 0.11.0.

### Fixed

- A closed stdout pipe (`dctx pack | head`) exits 0 without a panic.

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
- **`dctx pack --for "<text>" --budget N`**: task context for an agent card: `map find` on the names the text mentions,
  then neighbors and read of the best hits, whole lines only, within N estimated tokens (ceil(2 × UTF-8 bytes / 7),
  default 3000). `tools/cardgen/context.sh` prepends it to a card; `docs/guides/AGENTS-SNIPPET.md` is the `AGENTS.md` section that tells agents to run it first;
  `tools/cardgen/dogfood.py` records before/after prompt tokens for the ten v1 cards.
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
- Fortran and Verilog (both shipped in 1.1.0).
- Shell call edges.
- Embeddings and semantic search.
- LSP-grade type resolution.
- A stable plugin ABI and desktop API (unstable, not a v1 contract).
- Cross-repository queries.
- Publishing to crates.io or GitHub (a maintainer decision).

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

[Unreleased]: https://github.com/Eris-Margeta/dctx/compare/v1.2.0...HEAD
[1.2.0]: https://github.com/Eris-Margeta/dctx/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/Eris-Margeta/dctx/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/Eris-Margeta/dctx/compare/v0.25.0...v1.0.0
[0.25.0]: https://github.com/Eris-Margeta/dctx/compare/v0.11.0...v0.25.0
[0.11.0]: https://github.com/Eris-Margeta/dctx/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/Eris-Margeta/dctx/compare/v0.1.0...v0.10.0
[0.1.0]: https://github.com/Eris-Margeta/dctx/releases/tag/v0.1.0
