# Changelog

All notable changes to dctx are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/). Dates are ISO 8601.

The open repository is ongoing work. Anything that still depends on third-party
crates is not considered enterprise-ready; enterprise-ready builds are part of
dctx Enterprise.

## [Unreleased] — 0.26 development line

Planned next CLI/core release: **0.26.0**. No version is bumped here: `VERSION`,
`project-meta.yaml`, `dctx-cli` and `dctx-core` remain at **0.25.0**. The other
`crates/*` packages remain at **0.1.0**, the Git plugin at **1.0.0**, and the
desktop package/configuration at **0.11.0**. These are separate version lines,
not fields to bulk-replace. Follow [RELEASING.md](RELEASING.md) to synchronize
CLI/core metadata and the lockfile, date this section and check the release matrix
before any separately authorized release.

### Added
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

### Known issues
Measured in the Trail-Code benchmark and scheduled in the specification's roadmap:
- The map CLI exposes `contains` edges only; call and import edges are not yet
  emitted.
- No incremental indexing: the first query after an edit re-indexes, and each
  edit adds a full generation to the cache.
- Answer headers change after unrelated edits, which defeats prompt caching;
  JSON answers carry a per-second `age_s`.
- `dctx pack` includes `.env` files unless they are ignored. Add `.env*` to
  `.dctxignore` until the default changes.

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
