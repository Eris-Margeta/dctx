<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Map mode

Map mode explores a project through bounded outlines, symbol searches and source
reads. It builds the index automatically; there is no user `index` command.
The current CLI uses the shared dctx facade. Older storage and Git-based designs
in SPEC-MAP-MODE are not the current CLI contract.
dctx and this guide are Apache-2.0.

For a tool client, see the [MCP server guide](https://github.com/Eris-Margeta/dctx/blob/main/docs/guides/MCP.md), including the opt-in lean
profile and custom tool subsets.

## Definitions outside the index

For `find`, `neighbors`, `explain`, `read`, `grep` and `context`, a literal local
import can explain a missing definition. When an eligible referenced file defines
the requested or imported name, the page includes:

```text
defined outside index: ../lib/shared.py
add it with --also ../lib
```

JSON adds `defined_outside_index`, an array of objects with `path` and `hint`.
The notice does not turn a missing result into a successful indexed result.
The fallback checks literal relative imports/includes, Rust path attributes and
path dependencies, plus sibling Python `from` and Rust `use` targets. It searches
only below the indexed directory's parent; it does not consult package managers,
environment search paths or arbitrary sibling files. It uses the same ignore,
omit and sensitive-file policy and refuses symlink components. Its limits are
256 file attempts, 2 MiB of source bytes, 64 KiB per file, 2048 directory entries,
16 directory levels and 100 ms. Results are sorted by displayed path. A bounded
scan can miss definitions; this is a hint rather than semantic resolution.

Add roots explicitly to query their definitions and source together:

```sh
dctx map find shared --place app --also lib
dctx map read 's:lib/shared.py#shared' --place app --also lib
dctx context shared --place app --also lib
dctx pack app --also lib --for 'shared'
```

`--also DIR` is repeatable on every map verb, `pack` and `context`. Directories
are relative to the invocation directory. With extra roots, all paths and IDs
carry their root directory label, for example `s:app/main.py#run` and
`s:lib/shared.py#shared`. Root labels must be distinct. Each root keeps its own
selection rules and is read-only. The canonical, sorted root set keys the cache;
changing the order of `--also` does not change identities or reuse another set's
index. Without extra roots, existing IDs stay unchanged.

## Commands

```text
dctx map <map|find|neighbors|explain|read|skeleton|grep> <TARGET> [OPTIONS]
dctx map hubs [--limit N] [--json] [--place PATH] [--budget TOKENS] [--meta]
dctx map path <FROM> <TO> [--json] [--place PATH] [--budget TOKENS] [--meta]
dctx map export [--format json|mermaid] [--output PATH] [--place PATH]
```

| Verb | Use |
| --- | --- |
| `map TARGET` | Outline a place-relative file or a node; TARGET is required (omitting it does not scan). |
| `find TARGET` | Search for a name or concept; quote multiword targets. |
| `neighbors TARGET` | Inspect a node's relations. |
| `explain TARGET` | Inspect one node's details. |
| `read TARGET` | Read a node's verified source; a long node pages with `--cursor`. `dctx read` is the same verb. |
| `skeleton FILE` | A file's (or `m:<path>`'s) declaration signatures in source order, no bodies. |
| `grep LITERAL` | Exhaustive literal search of indexed files, hits grouped by enclosing symbol. |
| `hubs` | Most-called symbols and per-directory file and symbol counts. No TARGET. |
| `path FROM TO` | Shortest edge path (calls, imports, contains; either direction; at most 8 hops), or `no path`. |
| `export` | The whole graph as one deterministic JSON or Mermaid document; no token cap. |

Copy node IDs from results: `m:sample.py` names a module and
`s:sample.py#PythonBeacon` names a symbol. `dctx map <verb> --help` prints each
verb's own page and example.

The following option descriptions follow `dctx map --help`
([source](https://github.com/Eris-Margeta/dctx/blob/main/crates/dctx-cli/src/map/args.rs)):

| Option | Meaning / bounds |
| --- | --- |
| `--json` | Facade JSON v2, including the precision label. |
| `--place PATH` | Place or worktree root; default: current directory. |
| `--meta` | Restore the snapshot digest and generation in the text header and JSON. Without it, answer bytes do not change across unrelated edits or time. |
| `--profile` | Stage timings and the five slowest files on stderr (`DCTX_PROFILE=1` is the same switch). |
| `--budget TOKENS` | Output tokens, 1..2000; `explain` max: 1000. `read` with no `--budget` uses the 2,000-token hard cap. |
| `--depth N` | `map`: 1..3; `neighbors`: 1..4. `explain` and `read` take no depth. |
| `--limit N` | `map`, `find`, `neighbors` page size; `explain` and `read` accept only 1. `hubs`: 1..60 lines, default 20. |
| `--offset N` | Refused. Pass the previous page's `next` value to `--cursor`. |
| `--cursor TOKEN` | Opaque continuation. A stale token is rejected. |
| `--kind fn,type,doc` | `find`: comma-separated node kinds. |
| `--in PATH` | `find` or `grep`: restrict to a relative path. |
| `-i`, `--ignore-case` | `grep`: ASCII case-insensitive match. |
| `--edge calls,imports` | `neighbors`: comma-separated edge filter. |
| `--dir in\|out\|both` | `neighbors`: relation direction. |
| `--lines A-B --full` | `read`: exactly those absolute source lines inside the node; `--full` permits 400. |
| `--rev SHA` | Refused: this build does not link Git. |

Environment bounds: `DCTX_MAX_SOURCE_BYTES` (total source bytes one scan may read,
default 1 GiB), `DCTX_MAX_RECORDS` (files, symbols and edges one index may hold,
default 1000000), `DCTX_MAX_ENTRIES` (files and directories one scan may visit,
default 100000) and `DCTX_FRESHNESS` (see below).

The parser also accepts these `--kind` values: `method`, `struct`, `enum`,
`trait`, `impl`, `const`, `macro`, `class`, `interface`, `symbol`, `module`,
`package`, `place`, `test`. Edge names also include `contains`, `implements`,
`uses-type`, `tests`, `documents`, `reexports`.

The token estimate is `ceil(2 * UTF-8 bytes / 7)`, not a tokenizer guarantee.

## Languages and precision

This table is the output of `dctx languages --markdown` for the release build
(`dctx languages --json` gives the same data as JSON). Extension matching is
case-insensitive: `Upper.RS` is indexed as Rust.

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

Tiers: **full** extracts symbols and call edges, **outline** symbols only, and
**docs** headed sections. Confidence `guess` marks heuristic outlines, whose
records carry confidence `guess`. See [language coverage](https://github.com/Eris-Margeta/dctx/blob/main/docs/LANGUAGE-COVERAGE.md).

Rust uses `syn` in release builds (the `syntax-rust` feature, reported as parser
`dctx-lex-rust/syn-<version>` with `syntax` coverage), for supported Rust syntax
only and without type checking. The other languages use the first-party
structural lexers, including the JS lexer for TypeScript/TSX and the heading
extractors for documents. This is not a promise of exact semantic calls, type
resolution or macro expansion. `dctx --version --verbose` lists the parsers and
features of the running build.

JSON answers expose `precision.coverage`, `precision.completion` and
`precision.parser` (the default structural parser identity is `dctx-structural/3`).
The [model](https://github.com/Eris-Margeta/dctx/blob/main/crates/dctx-map-model/src/domain/precision.rs) distinguishes
`structural`, `syntax` and `opaque` coverage from `complete`, `partial` and
`refused` completion. These labels describe extraction, separately from edge
confidence. Unsupported extensions provide no extracted symbols; the smoke test
checks empty records for `OpaqueBeacon` in `sample.unknown`.

## Index, cache and privacy

Each invocation opens a facade session and can reuse a compatible persisted
map generation after checking its inventory and file freshness. The default
stamp mode trusts settled files with unchanged size, mtime and inode;
`DCTX_FRESHNESS=strict_live` hashes every file on a warm start. Exact source
reads still verify live bytes. There is no persistent CLI graph database.
The retired per-worktree SQLite cache selector (`map/storage.rs`) has been
deleted; the CLI does not read `DCTX_MAP_CACHE`.

The shared cache stores `ledger.v1`, `policy.v1`, serialized generations and
view manifests in managed object files. Generations contain file inventory,
digests and stamps, nodes, signatures, summaries, spans and edges, rather than
copies of all source contents. See [Choosing how to use dctx](https://www.dctx.internet.technology/docs/modalities/#huge-trees-scope-before-indexing)
for huge-tree scoping and the behavior when the cache cap is reached.
Set `DCTX_CACHE_DIR` to an
absolute path to override its root. Otherwise it is:

| Platform | Cache root |
| --- | --- |
| macOS | `~/Library/Caches/dctx/v2` |
| Windows | `%LOCALAPPDATA%/dctx/Cache/v2` |
| Other | `$XDG_CACHE_HOME/dctx/v2`, or `~/.cache/dctx/v2` |

The implemented lifecycle follows
[DX-ARCH-08](https://github.com/Eris-Margeta/dctx/blob/main/docs/archive/design/ARCHITECTURE-DECISIONS.md): time-based expiry, a system
size cap and an automatic cleaner. Defaults are 7 days for idle overlays/previews,
30 days for unpinned roots and 1 day for staging; the hard cap is 2 GiB, the soft
watermark 1.5 GiB and the eviction target 1.2 GiB. These are cache policies, not
a claim that every map query persists those object types.

```sh
dctx cache status --json
dctx cache gc --dry-run
dctx cache purge --root /absolute/project/path --dry-run
dctx cache purge --all --dry-run
dctx cache config auto off --max-bytes 1000000 --ttl-days 7
dctx cache auto
```

`status` accepts `--root PATH`; `gc` accepts `--root PATH`, `--max-bytes N` and
`--legacy`. Applying `purge` requires `--yes` and either `--root PATH` or `--all`.
`cache config` without arguments shows persisted policy; `auto on|off` controls
auto-clean, and `--max-bytes` can only tighten the cap. Pack, map, template,
plugin and TUI commands run one bounded cleanup when due (daily by default).
`--no-auto-clean` or `DCTX_AUTO_CLEAN=0` disables that slice.

Ignored files are excluded before content reads; omitted files may have metadata
listed but their content is never read. The host loads the global ignore file
(`<config dir>/dctx/ignore`: `$XDG_CONFIG_HOME/dctx/ignore` or
`~/.config/dctx/ignore` on Linux and macOS, `%APPDATA%\dctx\ignore` on Windows),
root and nested `.gitignore` rules, then the chosen root's `.dctxignore`.
`DCTX_INDEX_IGNORED=1` skips those ignore texts; built-in sensitive-file and
`.git` exclusions still apply. The core does not read Git metadata or run Git, so
`.git/info/exclude` and Git's `core.excludesFile` are not applied.
The optional Git plugin has separately disclosed authority; see [Plugins](https://www.dctx.internet.technology/docs/plugins/).

## Two worked examples

These fixtures and output fragments are copied from
[`map_cli.rs`](https://github.com/Eris-Margeta/dctx/blob/main/crates/dctx-cli/tests/map_cli.rs). The test checks fragments,
not full transcripts; the output blocks below are excerpts, without invented
columns or counts. Create a project containing `sample.py`:

```python
def PythonBeacon():
    return 1
```

From that project, outline its module:

```sh
dctx map map m:sample.py --depth 2 --budget 2000
```

Expected output contains:

```text
s:sample.py#PythonBeacon
```

For the second example, add `sample.unknown` with:

```python
def OpaqueBeacon():
    return 1
```

Search the unsupported source extension:

```sh
dctx map find OpaqueBeacon --json
```

Expected JSON contains this exact fragment and does not contain `OpaqueBeacon`:

```text
"records":[]
```

For crate responsibilities and optional parser dependencies, see the
[crate map](https://www.dctx.internet.technology/docs/crate-map/).
