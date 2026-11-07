<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# DX-A1 architecture baseline

Stage 0 evidence for the current dctx map mode, from
[ARCHITECTURE-PROPOSAL §3 and §7](../../docs/archive/design/ARCHITECTURE-PROPOSAL.md)
and the L1 review. The runner measures the landed library path behind
`dctx map`: `dctx-map-query::Index` on SQLite/FTS5 with tree-sitter.
It does not compare any number with a performance threshold, and no test
in this directory asserts one.

## What it measures

For two corpora:

- cold index time: first `Index::open` plus `Index::query` on an empty cache
- warm query p50 and p95 over 30 resident `Index::answer` find calls
- process RSS (`VmHWM` on Linux, otherwise `ps` rss)
- cache bytes, and cache bytes per 1,000 included files
  (`cache_bytes * 1000 / included_files`)
- parse calls on that cold index, plus the stale-file count
- one JSON answer for each of `map`, `find`, `neighbors`, `explain`, and `read`

`included_files` is cold parse calls plus stale files. Warm percentiles use
nearest rank: `ceil(percent * n / 100)` on samples sorted ascending.
Parser precision and recall are reported as not measured. This mode has no
adjudicated gold corpus, and a bounded verb answer is not that gold set.

Each corpus runs in its own process so RSS is not carried from the other
corpus. The place label in answers is the corpus id. The CLI labels a place
with the directory's file name.

## Corpora

`corpus/manifest.json` is the recipe.

1. **dctx.** The repository itself. The report records `git rev-parse HEAD`
   when git can see the worktree, plus a SHA-256 over sorted relative paths.
   The hash skips the manifest's path components (including `.git`, `.agent`,
   `target`, and `node_modules`), secret-shaped names (`.env.*`, `*.pem`,
   `*.key`, `id_rsa*`, `id_ed25519*`), and the scratch and reports prefixes.
   It does not follow symlinks. That hash is the pin of the walked files.
   The map scanner still applies its own ignore rules, so the hash is not
   claimed to be the scanner inventory.

2. **public-domain fixture.** `dx-a1-fixture-v1` writes original CC0-1.0
   Rust, JavaScript, TypeScript, and Markdown into
   `tools/architecture-bench/scratch/fixture`. Counts and the seed live in
   the manifest. Generation is deterministic. The fixture is text only.
   Scratch, the release binary, and `Cargo.lock` are gitignored. Do not
   commit them. After a run, the small files to keep are
   `reports/baseline.json` and `reports/baseline.md`.

The §3 planning envelope (5,000 files, 50 MiB) is an acceptance target for
later gates. This fixture is the sized public-domain corpus for Stage 0.
The dctx worktree is the product corpus.

## Run

From the repository root, on a machine with cargo and the map crate
dependency cache:

```sh
sh tools/architecture-bench/run.sh
```

The script builds `--release` with `CARGO_TARGET_DIR` under
`${TMPDIR:-/tmp}/dx-a1-architecture-bench` so the build output stays outside
the measured tree. The map cache for both corpora is created there as well,
because a cache inside the place is refused.

The VM check for this packet is:

```sh
sh tools/architecture-bench/run.sh
```

Direct dependency versions are pinned in `Cargo.toml`. This package is its
own workspace so the root `Cargo.toml` stays untouched. The first build
compiles bundled SQLite and tree-sitter.

## Reports

| File | Role |
|---|---|
| `reports/baseline.json` | Full observations, including the five verb answers |
| `reports/baseline.md` | Summary table, pins, and verb record counts |

`baseline.json` sets `no_performance_assertion` to true. A slow or large
result is still a successful report.

## Tests

Unit tests cover percentile rank, cache density, RSS text parsing, fixture
determinism and path refusal, manifest refusal, content-hash order, and
argument refusal. They do not start an index and they do not assert a
timing, RSS, or disk limit. Run them only as part of the bench package's
own `cargo test` when a human or CI asks. This packet's runner is `run.sh`.
