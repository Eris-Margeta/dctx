# dctx-map-query

Incremental Rust/Markdown indexing and the five bounded traversal verbs for
`dctx map`. The crate uses only public APIs of map-model, map-scan, map-parse,
map-resolve and map-store. The original snapshot command keeps its own routing.

```text
dctx map map crates/dctx-core --depth 2
dctx map find 'ignore pattern' --json
dctx map neighbors 's:crates/dctx-core/src/ignore.rs#parse_ignore_pattern_line'
dctx map explain 's:crates/dctx-core/src/ignore.rs#IgnoreEngine::decide'
dctx map read 's:crates/dctx-core/src/ignore.rs#IgnoreEngine::decide'
```

Library callers create `FileSource` and `Index::in_memory` or `Index::open`,
then call `Index::query` and `render_answer`. `query` refreshes the source
inventory; `answer` queries an explicitly refreshed index and hashes selected
files again. The latter is suitable for batches against one captured revision.
`IndexReport` reports parsed files, replaced transactions and stale failures.

Content hashes, rather than mtime/size, decide whether to parse. The resolver
retries unchanged callers when definitions change. Parse failures and exhausted
refresh deadlines mark records stale; reads refuse obsolete spans. Every read
checks path containment, ignore policy and its source hash. Secrets and links
are excluded before the scanner opens content. No watcher or daemon is used.

The CLI stores its index outside the source tree, under
`DCTX_MAP_CACHE`, `$XDG_CACHE_HOME/dctx/map`, or `$HOME/.cache/dctx/map`.
Canonical worktree paths select independent caches. A single-writer lock
refuses concurrent cache use. No snapshot output, registry or gitignore writes
occur in map mode. `DCTX_MAP_FOCUS` accepts comma/newline-separated relative
paths. Admin verbs are rejected.

FTS5 BM25 supplies lexical candidates. Deterministic integer scores add exact
name, public visibility, documentation, in-degree, focus and recent-commit
boosts. Output budgets include metadata and continuation commands; pagination
uses the number of records actually emitted. Reads retain whole source lines,
with a head/tail excerpt and an explicit command for omitted lines.

The landed APIs impose these limits:

- Store search exposes at most 40 candidates and no SQL-side kind/path filter.
  Filters and boosts apply to that candidate set.
- Store has no public move, revision, file-list or lossless unresolved-reference
  APIs. A query-owned `query.json` companion retains parser facts and moves;
  reopening compares regenerated facts with SQLite before answering. Shared
  commit bases and separate small SQLite overlays are not implemented: caches
  are isolated per worktree. `--rev` refuses unavailable or dirty revisions.
- Scan has no metadata-only incremental inventory API. Freshness scans hash all
  authorized files; unchanged files skip parsing and storage writes.
- Model read records allow at most a 400-line location span. Larger nodes are
  served in explicit windows with continuation commands.

`tests/recall.toml` contains 30 hand-written questions with fixed expected ids
from this workspace. The repository integration test indexes the workspace,
requires recall@10 >= 80%, reports full-index and prepared-query p95 timings,
and checks that an unchanged refresh reparses and rewrites nothing. Set
`DCTX_MAP_PERF=1` on an isolated machine to enforce 3 s / 30 ms timing limits;
default tests report timings without load-sensitive assertions.

Validation on the packet VM: query tests passed (14 tests), query Clippy passed
with warnings denied, and the CLI suite passed (20 tests). The final measured
debug run indexed 408 files with no stale parse failures, reached 29/30 recall,
and recorded prepared-query p95 of 27.9 ms. Full indexing took 4.75 s
(scan 0.67 s, parse 0.81 s, resolve 1.01 s, store 2.24 s), so the 3 s target is
**not met**. No release benchmark was run: packet verification permits only
the listed remote test/Clippy commands. Shared commit-base overlays and this
remaining performance target require follow-up; passing predicates do not
establish those two promises.
