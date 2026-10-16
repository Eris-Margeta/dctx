# dctx-map-query

Incremental Rust/Markdown indexing and the five bounded traversal verbs for
`dctx map`. The crate uses public map APIs for scanning rules, parsing,
resolution and per-file writes. Its SQLite adapter reads the spec's FTS5 schema
on the same WAL snapshot as query-owned metadata. Snapshot mode keeps its routing.

```text
dctx map map crates/dctx-core --depth 2
dctx map find 'ignore pattern' --json
dctx map neighbors 's:crates/dctx-core/src/ignore.rs#parse_ignore_pattern_line'
dctx map explain 's:crates/dctx-core/src/ignore.rs#IgnoreEngine::decide'
dctx map read 's:crates/dctx-core/src/ignore.rs#IgnoreEngine::decide'
```

Library callers create `FileSource` and `Index::in_memory` or `Index::open`,
then call `Index::query` and `render_answer`. `query` refreshes the source
inventory; `answer` queries an explicitly refreshed index and checks selected
files' metadata. Explicit `read` requests still validate the source hash.
`IndexReport` reports parsed files, replaced transactions and stale failures.

Content hashes, rather than mtime/size, decide whether to parse. The resolver
retries unchanged callers when definitions change. Parse failures and exhausted
refresh deadlines mark records stale; reads refuse obsolete spans. Every read
checks path containment, ignore policy and its source hash. Secrets and links
are excluded before the scanner opens content. No watcher or daemon is used.

The CLI stores its index outside the source tree, under
`DCTX_MAP_CACHE`, `$XDG_CACHE_HOME/dctx/map`, or `$HOME/.cache/dctx/map`.
Canonical worktree paths select independent caches. Readers pin SQLite WAL
snapshots without locking each other. Only initialization or a required refresh
takes an OS advisory lock, rechecking the inventory after waiting. Lock files
remain on disk; the operating system releases locks when a process exits or is
killed. Legacy sentinel files are ignored. No snapshot output, registry or gitignore writes
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
- Store has no public snapshot, move, revision or lossless retry APIs. Query-owned
  SQLite rows retain metadata, resolved graphs, parser facts and moves. Warm
  opens load metadata only. Find hydrates at most 40 candidate nodes; the other
  verbs load their requested roots and reachable neighborhood through indexed
  edges. Parser inputs load only for a writer. Legacy JSON retry facts and move
  history are imported during the first refresh, preserving old-id answers.
  A writer marks publication incomplete before per-file transactions and publishes
  the completed graph afterward. Readers seeing an incomplete publication wait
  for the writer and repair after a crash; already pinned readers keep their old
  coherent snapshot. Shared
  commit bases and separate small SQLite overlays are not implemented: caches
  are isolated per worktree. `--rev` refuses unavailable or dirty revisions.
- Scan has no metadata-only inventory API. The query adapter uses its public
  ignore/classification rules and caches directory and rule stamps. Unchanged
  queries stat known paths without opening source content or rerunning ignore
  matching. Flagged files are hashed; only changed content is parsed. Unix
  change time and inode identity also detect same-size edits preserving mtime.
  On platforms without these fields, preserved-mtime changes are detected by
  explicit source reads. Changed ignore rules invalidate inventory decisions.
- Model read records allow at most a 400-line location span. Larger nodes are
  served in explicit windows with continuation commands.

`tests/recall.toml` contains 30 hand-written questions with fixed expected ids
from this workspace. The repository integration test indexes the workspace,
requires recall@10 >= 80%, reports full-index and prepared-query p95 timings,
and checks that an unchanged refresh reparses and rewrites nothing. Set
`DCTX_MAP_PERF=1` on an isolated machine to enforce 3 s / 30 ms timing limits;
default tests report timings without load-sensitive assertions.

The preceding CM-6b profile recorded scan 0.67 s, parse 0.81 s, resolve 1.01 s,
and storage 2.24 s for 408 files. Warm reopen previously paid the full content
scan, graph resolution and comparison costs again. CM-6c removes those costs
and the unconditional JSON rewrite. Regression tests count real content opens
across reopen/query/render, require zero warm reads and reindexes over 500 files,
measure a 5k-file metadata pre-filter, exercise eight readers with a writer,
and kill an actual advisory-lock holder. Repository evidence now includes
reopened production queries, rather than only prepared answers. The permitted
remote test commands use the test profile; release timing targets require a
separate optimized run. Source-byte counters apply to FileSource IO; Git may
perform its own reads when computing revision and dirty metadata.

Final VM test-profile evidence: repository recall 29/30, reopened query p95
72.5 ms; the 500-file fixture had p95 58.8 ms, zero warm content reads and zero
reindexes. Its one-file update took 263 ms; the 5k-file pre-filter took 126 ms.
These unoptimized, shared-VM measurements do **not** establish the 30/50/20 ms
release targets. The intermediate eager-graph version took 763 ms per reopened
repository query; bounded SQLite hydration removed that measured decoding cost.
