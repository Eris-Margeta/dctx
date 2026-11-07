# Map mode

Map mode explores a project through bounded outlines, symbol searches and source
reads. It builds the index automatically; there is no user `index` command.
The current CLI uses the shared dctx facade. Older storage and Git-based designs
in SPEC-MAP-MODE are not the current CLI contract.
dctx and this guide are Apache-2.0 (decision 078).

## Commands

```text
dctx map <map|find|neighbors|explain|read> [TARGET] [OPTIONS]
```

| Verb | Use |
| --- | --- |
| `map [TARGET]` | Outline the project root, a path or a node. |
| `find TARGET` | Search for a name or concept; quote multiword targets. |
| `neighbors TARGET` | Inspect a node's relations. |
| `explain TARGET` | Inspect one node's details. |
| `read TARGET` | Read a node's source. |

Copy node IDs from results: `m:sample.py` names a module and
`s:sample.py#PythonBeacon` names a symbol.

The following option descriptions follow `dctx map --help`
([source](https://github.com/Eris-Margeta/dctx/blob/main/crates/dctx-cli/src/map/args.rs)):

| Option | Meaning / bounds |
| --- | --- |
| `--json` | Facade JSON v2, including the precision label. |
| `--place PATH` | Place or worktree root; default: current directory. |
| `--budget TOKENS` | Output tokens, 1..2000; `explain` max: 1000. |
| `--depth N` | `map`: 1..3; `neighbors`: 1..2. |
| `--limit N --offset N` | `map`, `find`, `neighbors`: bound records; offset above 0 is refused. Pages use the next cursor. |
| `--kind fn,type,doc` | `find`: comma-separated node kinds. |
| `--in PATH` | `find`: restrict to a relative path. |
| `--edge calls,imports` | `neighbors`: comma-separated edge filter. |
| `--dir in\|out\|both` | `neighbors`: relation direction. |
| `--lines A-B --full` | `read`: help describes exact source lines; full permits 400. See limitation below. |
| `--rev SHA` | Refused: this map build does not link Git. |

The parser also accepts these `--kind` values: `method`, `struct`, `enum`,
`trait`, `impl`, `const`, `macro`, `class`, `interface`, `symbol`, `module`,
`package`, `place`, `test`. Edge names also include `contains`, `implements`,
`uses-type`, `tests`, `documents`, `reexports`.

The CLI currently translates `--lines A-B` into a line count (`B-A+1`) when
calling the facade; it does not forward the starting line. Exact arbitrary-range
selection is therefore **unverified**; do not rely on the help wording alone.
The token estimate is `ceil(2 * UTF-8 bytes / 7)`, not a tokenizer guarantee.

## Languages and precision

These extensions come from
[`Language::for_path`](https://github.com/Eris-Margeta/dctx/blob/main/crates/dctx-map-parse/src/domain/facts.rs).
That selector is case-insensitive; the current facade's separate extension
dispatch matches lowercase spellings, so use the spellings below for CLI input.

| Language | Extensions |
| --- | --- |
| Rust | `.rs` |
| JavaScript / JSX | `.js`, `.mjs`, `.cjs`, `.jsx` |
| TypeScript | `.ts`, `.mts`, `.cts` |
| TypeScript JSX | `.tsx` |
| Markdown | `.md`, `.markdown` |
| Python | `.py`, `.pyi` |
| Go | `.go` |
| Java | `.java` |
| C / C++ | `.c`, `.h`, `.cc`, `.cpp`, `.cxx`, `.hpp`, `.hh` |
| Configuration | `.json`, `.jsonc`, `.toml`, `.yaml`, `.yml` |
| C# | `.cs` |
| Kotlin | `.kt`, `.kts` |
| Ruby | `.rb`, `.rake`, `.gemspec` |
| PHP | `.php`, `.phtml`, `.inc` |
| Swift | `.swift` |

The default CLI extracts structural facts for all these languages. Rust uses a
line-oriented extractor; the others use the first-party structural lexers,
including the JS lexer for TypeScript/TSX and the Markdown heading extractor.
This is not a promise of exact semantic calls, type resolution or macro expansion.
An optional `syntax-rust` build links `syn` for supported Rust syntax, without
type checking; the feature alone does not change the CLI's default parser plan.

JSON answers expose `precision.coverage`, `precision.completion` and
`precision.parser` (the default parser identity is `dctx-structural/1`).
The [model](https://github.com/Eris-Margeta/dctx/blob/main/crates/dctx-map-model/src/domain/precision.rs) distinguishes
`structural`, `syntax` and `opaque` coverage from `complete`, `partial` and
`refused` completion. These labels describe extraction, separately from edge
confidence. Unsupported extensions provide no extracted symbols; the smoke test
checks empty records for `OpaqueBeacon` in `sample.unknown`.

## Index, cache and privacy

Each invocation opens a facade session, takes a strict snapshot and queries its
ephemeral index. Repeated processes share the host cache root, not a persistent
CLI graph database. Identity uses dctx's first-party SHA-256 and file metadata,
without Git object IDs. The retired per-worktree SQLite cache selector
(`map/storage.rs`) has been deleted: queries do not create `~/.cache/dctx/map`,
and the CLI does not read `DCTX_MAP_CACHE`.

The shared cache stores `ledger.v1` and `policy.v1`. Set `DCTX_CACHE_DIR` to an
absolute path to override its root. Otherwise it is:

| Platform | Cache root |
| --- | --- |
| macOS | `~/Library/Caches/dctx/v2` |
| Windows | `%LOCALAPPDATA%/dctx/Cache/v2` |
| Other | `$XDG_CACHE_HOME/dctx/v2`, or `~/.cache/dctx/v2` |

The implemented lifecycle follows
[DX-ARCH-08](https://github.com/Eris-Margeta/dctx/blob/main/docs/design/ARCHITECTURE-DECISIONS.md): time-based expiry, a system
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
listed but their content is never read. The core never reads `.git/` or runs Git
(decision 078). The intended `.gitignore` contract is rules only, not Git metadata.
**Unverified:** automatic `.gitignore` rule loading in this facade CLI path:
`host.rs` supplies `RuleTexts::default()`, with no local/global rule texts.
Do not assume project-specific ignore rules were loaded by this map command.
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
