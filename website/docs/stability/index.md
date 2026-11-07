<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# STAB-01: byte stability on `0495f5f`

Measured **2026-11-06 UTC** on Linux from commit
`0495f5f87717df5385209d306b92328a8cd96cf6`. The directory retains its original evidence date. No engine changes were made.

| Measurement | Text | JSON |
|---|---:|---:|
| Answers identical after unrelated edit | 33/33 | 33/33 |
| Answers identical over five separate invocations | 33/33 | 33/33 |
| Baseline → post-edit common-prefix ratio, minimum / mean | 1.0 / 1.0 | 1.0 / 1.0 |
| Consecutive same-request transitions, five per case | 165 | 165 |
| Same-request M7, minimum / mean | 1.0 / 1.0 | 1.0 / 1.0 |
| Consecutive changing-question transitions | 195 | 195 |
| Changing-question M7, minimum / median / mean | 0.007795889 / 0.107142857 / 0.214830257 | 0.008776458 / 0.067594433 / 0.182446794 |
| Changing-question M7, total prefix bytes / total current bytes | 0.061846978 | 0.079892615 |

All **405 CLI invocations exited 0**, with empty stderr: 396 measurement invocations
(66 cases × six runs), three target-resolution invocations, and six snapshot
controls. All 33 default text headers were exactly `# <place> <verb>`; all 33
default JSON answers omitted snapshot, generation and `freshness.age_s`.
`--meta --json` controls confirmed that each corpus's snapshot changed after the edit.

## Protocol and pins

The original producer is
[`phase_query.py`](https://github.com/Eris-Margeta/dctx/blob/main/docs/research/trail-code-2026-10/scripts/phase_query.py), with byte comparisons
in [`aggregate.py`](https://github.com/Eris-Margeta/dctx/blob/main/docs/research/trail-code-2026-10/scripts/aggregate.py), section 7. It runs
five warm queries, appends a comment to an unrelated file in another directory,
reruns queries, and restores the file. Its original evidence remains at
[`data/stability.csv`](https://github.com/Eris-Margeta/dctx/blob/main/docs/research/trail-code-2026-10/data/stability.csv).

The original `/path/to/workdir/dctx-vs-trail` host tree is absent. The repository contains
the scripts and historical results, but no original seven-corpus checkout set or
the host's `scripts/corpora.json`. Checked alternate `bench3` and `perf-20261101`
corpora paths were also absent. This run uses the documented fallback: three corpora
extracted exclusively from tracked files at the pinned current commit. No fetch or
network access was used. The new denominator differs from the historical 0/63;
Trail-Code's historical 59/70 was not rerun.

| Corpus | Exact tracked prefix extracted | Files | Symbol / file | Unrelated file edited |
|---|---|---:|---|---|
| parser-mixed | `crates/dctx-map-parse/tests/fixtures/` | 17 | `helper`, `rust/src/support.rs` | `ts/module.ts` |
| lexer-go | `crates/dctx-lex-go/` | 19 | `Add`, `tests/fixtures/basic.go` | `src/lib.rs` |
| map-query | `crates/dctx-map-query/src/` | 30 | `render_answer_at`, `adapters/output.rs` | `domain/ranking.rs` |

Each prefix is copied in full using `git ls-tree -r --name-only <pin> -- <prefix>`
and `git show <pin>:<path>`, retaining paths relative to the prefix. Every file's
size and SHA-256, and the CLI binary SHA-256, are in [`corpora.json`](https://github.com/Eris-Margeta/dctx/blob/main/docs/research/stability-20261105/corpora.json).
Work copies and isolated cache/config directories live in a disposable `/tmp`
directory. The live worktree's corpus files are never edited.

The original applicable map query shapes are retained: `find --limit 10`, inbound
`neighbors --edge calls` at depths 1 and 2, outbound neighbors, `map --depth 2
--limit 40`, `read`, and `explain`. Each now runs in **both** formats. The current
bounded `skeleton`, `grep`, `hubs`, and `path` verbs add four shapes per corpus,
again in both formats: 11 × 2 × 3 = 66 cases. Stable target IDs are resolved by
`find --json --limit 40`, matching exact path and symbol. `path` tests a file's
containment edge to that symbol. `grep` is confined to the target file.

Default budgets and default output are used, without `--meta` in the measured
answers. There are five warm sweeps, separated by 1.05 seconds, then a sixth sweep
after appending exactly `\n// STAB-01 unrelated comment\n` to the named unrelated
file. The comment is outside declarations and contains none of the queried names.
It changes snapshot identity without changing the selected declarations, calls,
search hits or hub counts. Original bytes are restored in a `finally` block and
verified. Configuration and caches are isolated using `XDG_CONFIG_HOME`,
`XDG_CACHE_HOME` and `DCTX_CACHE_DIR`; inherited `DCTX_*` overrides are removed.

Every JSON case has records except outbound neighbors for the two leaf functions
`helper` and `Add`. Their empty answers are included. All three inbound-neighbor
cases have records, all paths are found, and `render_answer_at` has nine returned
outbound neighbors plus eleven more in the bounded page. No cursor pages were
followed. Whole-graph `export` has no unrelated graph input and is outside this
bounded-answer protocol. The historical legacy snapshot overview, Trail-only grep
and no-refresh shapes are outside the map-answer comparison.

## M7 and acceptance limits

For each transition, M7 = longest common **byte** prefix of previous and current
stdout / **current** stdout byte length. Both block lengths and the prefix length
are stored in [`turn-prefixes.csv`](https://github.com/Eris-Margeta/dctx/blob/main/docs/research/stability-20261105/turn-prefixes.csv); there are no empty blocks.
No whitespace, JSON fields, paths, timestamps or newlines are normalized.

The same-request trace is each case's five warm answers followed by its post-edit
answer: five consecutive transitions per case, 330 total. The additional
changing-question trace uses actual invocation order, separately per corpus and
format: the 11 query shapes in the order above, repeated over six sweeps; 65
transitions per stream, 390 total. Text and JSON are separate context-block streams.
These are recorded tool-answer traces, with one answer treated as one injected
block; there is no provider cache-hit or task-acceptance measurement.

The local unrelated-edit and five-run JSON gates pass. The low changing-question
ratios show why this result cannot establish §12.1's T0–T3 invariant or its recorded
tier-context M7 ≥ 0.9 gate. Tier layering remains **target**. No cross-platform,
thread-count, changed-answer-content, or full original benchmark gate is claimed.
No unrelated-edit instability was observed, so there is no instability reproducer.

The three text `read` pages actually include their selected source bodies. Text
`explain` still consists of header and id/kind/path, so §11's richer text-page
contract remains a target. Other text fields, JSON compaction and provenance-trailer
contracts were not promoted to landed by this measurement.

## Exact commands and exits

Run from `/path/to/workdir/codex/STAB-01R`:

```sh
export PATH=$HOME/.cargo/bin:$PATH
export CARGO_TARGET_DIR=$PWD/.target-stab
cargo --offline build --locked -p dctx-cli > /tmp/stab-01-build.log 2>&1
```

Exit **101**: configured `sccache` failed with `Operation not permitted`, before
compilation. A retry after `unset RUSTC_WRAPPER`, with output redirected to
`/tmp/stab-01-build-no-wrapper.log`, also exited **101**: Cargo's user config still
specified `build.rustc-wrapper = "sccache"`.

```sh
export PATH=$HOME/.cargo/bin:$PATH
export CARGO_TARGET_DIR=$PWD/.target-stab
export RUSTC_WRAPPER=
cargo --offline build --locked -p dctx-cli > /tmp/stab-01-build-direct.log 2>&1
python3 docs/research/stability-20261105/measure.py > /tmp/stab-01-measure.log 2>&1
python3 docs/research/stability-20261105/verify.py
git diff --check
```

Exits: build **0**; measurement **0**; independent raw-data audit **0**;
`git diff --check` **0**. Build logs and [`measurement.txt`](https://github.com/Eris-Margeta/dctx/blob/main/docs/research/stability-20261105/measurement.txt) are
preserved here. [`commands.md`](https://github.com/Eris-Margeta/dctx/blob/main/docs/research/stability-20261105/commands.md) lists every query shape and setup/control
command; [`runs.jsonl`](https://github.com/Eris-Margeta/dctx/blob/main/docs/research/stability-20261105/runs.jsonl) records all 405 exact argv arrays, shell-quoted
commands, working directories, environment overrides, exits, full stdout/stderr,
byte counts and SHA-256 hashes. `measure.py` records a fresh run only when no
`runs.jsonl` exists; use a fresh copy of the evidence directory to repeat it.

[`stability.csv`](https://github.com/Eris-Margeta/dctx/blob/main/docs/research/stability-20261105/stability.csv) is the new measurement. The adjacent
[`stability-original.csv`](https://github.com/Eris-Margeta/dctx/blob/main/docs/research/stability-20261105/stability-original.csv) is a byte-for-byte copy of the
historical CSV, SHA-256
`46b54db2fb469dc4df70eaa0bf22b458a8089f437cdee3294efe071314280266`.
Both files are retained to distinguish the new measurement from the unchanged
historical source.
