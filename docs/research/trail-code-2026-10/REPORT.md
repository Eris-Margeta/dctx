# dctx vs Trail-Code: head-to-head benchmark (October 2026)

Isolated, reproducible comparison of two code-context tools for coding agents on seven real corpora
and one synthetic adversarial corpus. Every number below comes from `data/raw/*.jsonl` and can be
regenerated with `scripts/`. Where Trail-Code is better, this report says so plainly.

- **dctx** at commit `17dff0b454c24f43848abad1c88ea982e02731f7` (branch `internal`), `dctx-cli v0.25.0`,
  agent interface `dctx map <map|find|neighbors|explain|read>`.
- **Trail-Code** `@trailhq/trail-code@0.23.1` from npm (repository HEAD `b1211efd`, whose
  `package.json` is also 0.23.1), CLI `trail` plus its MCP server.

## 1. Summary

| Area | Result |
|---|---|
| Call graph (callers, depth 1) | **Trail-Code wins decisively.** Trail precision 0.94 to 1.00, recall 0.10 to 0.96 per corpus. dctx returns **no call or import edges at all** (recall 0 on 140 targets): the CLI facade only emits `contains` edges. |
| Task-level answers (70 agent questions) | Trail 122/140 points, 312/354 gold facts in 88.7k tokens. dctx 69/140, 169/354 in 500k tokens (480k of them from the unbounded legacy tree used as the only overview). Without the overview question dctx uses 19.2k tokens for 137/322 facts (7.15 facts/1k tokens) versus Trail 57.0k for 284/322 (4.98 facts/1k): dctx is terser, Trail answers about twice as much. |
| Footprint | **dctx wins.** One 9.2 MB binary (libc, libm, libgcc_s) versus 405.6 MB of `node_modules` (8,567 files, 50 package versions) plus a Node runtime. |
| Cold index | **dctx wins** on all seven corpora: 0.09 to 6.05 s versus 2.44 to 94.9 s (2.8 to 69 times faster); peak RSS 8 to 567 MiB versus 181 to 2,111 MiB. Trail's index is smaller only on core (65 MB versus 158 MB). |
| Warm query latency | **dctx wins** on small and medium repos (30 to 260 ms versus 0.6 to 1.4 s; Node startup sets Trail's floor near 0.6 s). On the largest Rust corpus dctx re-reads a 158 MB generation per process (1.45 s, 370 MiB per query) and Trail's `callers` is faster (0.83 s). |
| Freshness | **Tie on correctness**: both see unstaged, untracked and staged edits and drop reverted symbols. Trail also adds the caller edge to the new function; dctx cannot. dctx's first query after an edit costs a full re-index (core: 7.5 s), and every edit adds a full new generation to the cache (core: 158 MB to 946 MB). |
| Cache-friendliness | **Trail wins.** Trail text outputs are byte-identical across runs and across unrelated edits. Every dctx answer starts with a header containing the snapshot digest, so any edit anywhere changes byte 21 to 50 of every answer; dctx JSON is never byte-identical twice (`age_s`). |
| Bounding | **dctx wins for map verbs** (hard 2,000-token cap, never exceeded; budgets below the minimum record size are refused with exit 1). Trail has no budget flag; single answers reached 142 KB (40.6k tokens). dctx's legacy tree and `pack` are unbounded (974 KB tree on django; 8.7 MB pack). |
| Privacy / network | **dctx wins.** Zero sockets in every audited dctx run and no writes to the repository in map mode. Trail does a daily npm version check that ignores `DO_NOT_TRACK`, records an install telemetry event in `postinstall` and tries to send it, and modifies `.gitignore` and adds `.ignore` on every `trail build`. dctx `pack` printed a fake `.env` secret; Trail never read `.env`. |
| Language breadth | Trail parses 23 languages, including Scala, Elixir, Lua, OCaml, Zig, Dart, Solidity and Vue; dctx parses none of those. dctx adds structure for Markdown, JSON, TOML and YAML, which Trail ignores. |

## 2. Methodology

### Host, isolation, contention

- VM `build-host`: 32 vCPU Intel Xeon E5-2680 v2 @ 2.80 GHz, 94 GB RAM, Debian 13, kernel
  6.12.111, Node v24.22.0, npm 11.19.1, rustc/cargo 1.99.0 (`data/env.txt`).
- The VM was also running an unrelated build and mutation batch. Every benchmark command was pinned with
  `taskset -c 24-31` (8 CPUs). The other batch was not pinned, so it could share those CPUs.
  1-minute load average across the 3,398 measured runs: min 1.04, median 2.31, p95 8.46, max 16.03
  (each row in `data/raw/*runs.jsonl` records load before and after; `phases.jsonl` records each phase).
  The ripgrep and pocketbase query phases ran at load 8 to 10, so treat their Trail `find` times
  (1.2 to 1.3 s) as contention-affected.
- Both tools ran as the dedicated unprivileged user `dctxbench`, with `HOME`, npm prefix, npm cache,
  `XDG_CACHE_HOME` and `DCTX_CACHE_DIR` inside `/path/to/workdir/dctx-vs-trail`, through `scripts/benv`
  (`env -i`, then `sudo -u dctxbench unshare -rn`, then `taskset`). After installs, clones and builds,
  every measured command ran in a new network namespace with only a down loopback.
- `DO_NOT_TRACK=1` (Trail's documented opt-out, `dist/telemetry/gate.js`) was set for every measured
  Trail run except the two deliberate "telemetry on" network audits.
- Timing: GNU time 1.9 (`/usr/bin/time -v`, 10 ms resolution) records wall, user+sys CPU and peak RSS for
  the tool process tree. Warm queries are medians of 5 runs, cold runs medians of 3.
- Output size: raw stdout bytes and `ceil(bytes / 3.5)` estimated tokens for both tools (identical to
  dctx's own `ceil(2*bytes/7)`). This is an estimate, not a tokenizer.
- Page cache was not dropped, to avoid disturbing the co-tenant batch. "Cold" therefore means "no tool
  index or cache", not "cold disk".

### Builds and versions

- dctx: source rsynced from the workstation (no `target/`), built with `cargo build --release --locked
  -p dctx-cli -j 8` without a compiler cache: 98.7 s wall, 389 s user CPU. Binary: 9,207,072 bytes,
  sha256 `26c3cb70...cbce`. `ldd`: `libgcc_s`, `libm`, `libc`, `ld-linux` only.
- Trail-Code: `npm install --save-exact @trailhq/trail-code@0.23.1` (39.96 s). npm 11.19 printed
  "install scripts not yet covered by allowScripts" warnings but still ran them (13 packages, including
  Trail's own `postinstall` and `node-gyp` builds; Node headers were downloaded into `~/.cache/node-gyp`).

### Corpora (pinned)

| Corpus | Language | Pin | Tracked files |
|---|---|---|---|
| flask | Python | tag 3.1.3, `22d92470` | 235 |
| ripgrep | Rust | tag 15.2.0, `e89fff89` | 222 |
| pocketbase | Go + JS | tag v0.40.5, `6f370ea7` | 924 |
| trailcode (Trail-Code repo) | TypeScript | `b1211efd` | 380 |
| dctx | Rust (+ fixtures in 15 languages) | `17dff0b4` | 1,749 |
| django | Python | tag 6.1.2, `2212534b` | 7,071 |
| core (private Rust repository) | Rust | `f5f2f48b` | 3,190 |
| adversarial (generated) | mixed | `scripts/gen_adversarial.py` | see section 10 |

`core` was copied without `target/` and `corpus/`; its `.agent/worktrees/` (copies of the tree) was also
removed. Its raw outputs, symbol names, paths and source lines are **redacted** from `data/`
(`scripts/redact_private.py`); only numbers are published.

### Gold sets and scoring

- **Targets.** `scripts/select_targets.py` picks 20 definitions per corpus with a fixed seed: 4 hubs
  (unique name, many calls), 4 rare (1 to 3 calls), 4 receiver-called methods, 4 colliding names
  (2 to 6 definitions), 4 interface/trait methods. Selection uses the independent oracle in
  `scripts/goldlib.py`, not either tool.
- **Oracle.** Comments and strings are blanked (a cross-line state machine for Rust and Go, per-line for
  JS/TS, triple-quote aware for Python). A line is a call site of `N` only if `N` is directly followed by an
  argument list and the line is not a definition. Each site is bound to a definition by scope rules (same
  file or package, explicit import, `self`/`this`, `Type::`); receiver calls to names that also exist on
  standard-library types, and anything else unresolvable, are **ambiguous** and excluded from both
  precision and recall. Interface/trait targets are scored at name level: every site bound to some
  in-repo definition of the name, plus `self` calls.
- **Adjudication.** A Trail `calls` hit is a true positive if its span contains a gold site of the target,
  unjudged if it only contains ambiguous sites, and a false positive otherwise. Recall counts gold sites
  inside any reported hit. Collisions were spot-checked by hand (all 28 collision targets: sample sites
  listed in `data/targets/*.targets.json`). The hand check found one oracle bug: a `/*` inside a Rust
  multi-line doc string started a fake block comment. After the fix the gold was rebuilt for the **same**
  targets (`scripts/regold.py`; one target changed, ripgrep `unwrap_switch`, 61 to 69 sites), and the ripgrep
  accuracy and task phases were rerun. The earlier rows are kept in `data/raw/*.pre-regold`. A remaining
  known oracle gap: calls inside Python f-strings and JS template literals are blanked, which made
  one true Trail hit (flask `dumps`) count as a false positive.
- **Tasks.** `scripts/phase_tasks.py` builds 10 questions per corpus from the same oracle with fixed
  command sequences per tool. Facts are scored by presence in the outputs (JSON string values are also
  searched); caller facts must come from a caller/impact answer and need the caller's path and one of its
  call-line numbers. Score per question: 2 if at least 80% of facts are present, 1 if any, 0 otherwise.

## 3. Footprint

| | dctx | Trail-Code |
|---|---|---|
| Install | one binary, 9,207,072 bytes | `node_modules` 405,601,700 bytes, 8,567 files |
| Dependencies | 46 external crates in `cargo tree -e normal` (see TC-F11) | 50 package versions (47 names) in `npm ls --all`; largest: `tree-sitter-wasm` 128 MB, `@trailhq/trail-code` 129 MB (bundled grammars), `tree-sitter-swift` 73 MB, two LLM-provider SDKs 34 MB and 14 MB (used only by `--deep`) |
| Runtime | libc, libm, libgcc_s | Node >= 20 plus native addons |
| Install side effects | none | `postinstall` records an anonymous install event and spawns a detached flush; `~/.graft/telemetry.json` and `~/.graft/update-check.json` created at install |
| MCP | none (listed as planned in README) | 6 tools; tool definitions 3,515 bytes (~1,005 tokens) per agent session |

## 4. Indexing (section 2 of the brief)

Cold = new copy, no tool state, median of 3. dctx has no index command, so its cold run is the first
`dctx map find main` with an empty `DCTX_CACHE_DIR`. The Trail cold run is `trail build .`. No-op = repeat
with nothing changed (dctx: same query; Trail: `trail build .` again and `trail build --check .`).

| Corpus | dctx cold s (CPU s) | Trail cold s (CPU s) | dctx peak RSS MiB | Trail peak RSS MiB | dctx cache MB | Trail index MB | dctx no-op s | Trail no-op build s | Trail `--check` s |
|---|---|---|---|---|---|---|---|---|---|
| flask | **0.09** (0.08) | 3.45 (3.88) | **8** | 181 | **0.85** | 4.39 | **0.04** | 0.88 | 3.29 |
| ripgrep | **0.45** (0.42) | 2.44 (3.40) | **18** | 282 | **1.77** | 8.36 | **0.05** | 1.35 | 1.97 |
| pocketbase | **0.72** (0.71) | 49.45 (51.42) | **39** | 969 | **6.69** | 41.83 | **0.17** | 2.65 | 49.52 |
| trailcode | **0.31** (0.30) | 17.83 (18.65) | **17** | 452 | **3.56** | 13.70 | **0.08** | 1.58 | 18.67 |
| dctx | **2.42** (2.38) | 9.74 (13.96) | **64** | 834 | **14.88** | 42.68 | **0.27** | 3.81 | 7.90 |
| django | **2.48** (2.46) | 94.89 (97.55) | **92** | 2,111 | **23.30** | 167.48 | **0.81** | 9.15 | 86.46 |
| core | **6.05** (7.02) | 16.88 (21.43) | **567** | 916 | 157.70 | **64.71** | **1.34** | 4.66 | 13.82 |

Notes:
- core's dctx cost is dominated by configuration extraction. `--profile` on a cold run: parse/lex 3.39 s,
  canonicalize/seal 1.43 s, persist 0.97 s; the slowest file is a 6.7 MB JSON test fixture that produced
  235,842 symbols. The 158 MB generation is 2.9 times the 54 MB working tree (TC-F07).
- Trail's `build --check` (the CI freshness check) costs as much as a full build on pocketbase and django.
- Trail writes its index into the repository: `graft/` plus a modified `.gitignore` and a new `.ignore` on
  every corpus, or `.trail/cache/` when the repo already has `.trail/` (Trail's own repo). dctx map writes
  nothing into the repository and nothing into `HOME` (verified by full manifests before and after).

## 5. Query latency and output size (section 3 and 4)

Warm medians of 5 (wall seconds; bytes of the answer). Command mapping:
find = `dctx map find N --limit 10` / `trail ask N`; callers = `dctx map neighbors <id> --dir in --edge calls`
/ `trail callers N`; outline = `dctx map map FILE --depth 2 --limit 40` / `trail skeleton FILE`;
read = `dctx map read <id> --json` / `trail ask N --source --full -n 1`; overview = legacy `dctx . -t`
(the map verbs have no root overview, TC-F05) / `trail map`; grep = none / `trail grep 'N\('`.

| Corpus | find dctx | find Trail | callers dctx | callers Trail | outline dctx | outline Trail | read dctx | read Trail | overview dctx | overview Trail | grep Trail |
|---|---|---|---|---|---|---|---|---|---|---|---|
| flask | 0.03 s / 848 B | 0.69 s / 917 B | 0.03 / 104 (empty) | 0.63 / 476 | 0.03 / 1,327 | 0.63 / 2,980 | 0.03 / 1,472 | 0.68 / 1,622 | 0.04 / 22,537 | 0.64 / 3,925 | 0.65 / 3,244 |
| ripgrep | 0.06 / 217 | 1.27 / 1,371 | 0.06 / 107 (empty) | 0.67 / 2,801 | 0.05 / 1,700 | 0.66 / 3,935 | 0.05 / 897 | 0.85 / 1,057 | 0.08 / 21,972 | 0.71 / 8,883 | 0.69 / 3,213 |
| pocketbase | 0.17 / 993 | 1.18 / 1,252 | 0.20 / 110 (empty) | 1.04 / 611 | 0.17 / 1,087 | 0.72 / 1,226 | 0.17 / 780 | 1.19 / 1,902 | 0.22 / 80,655 | 0.79 / 5,297 | 0.96 / 5,019 |
| trailcode | 0.08 / 1,405 | 0.90 / 1,792 | 0.08 / 109 (empty) | 0.67 / 1,624 | 0.08 / 1,740 | 0.66 / 4,166 | 0.08 / 1,691 | 0.92 / 2,332 | 0.41 / 30,347 | 0.67 / 1,687 | 0.74 / 3,476 |
| dctx | 0.26 / 222 | 1.40 / 3,020 | 0.26 / 105 (empty) | 0.79 / 6,790 | 0.26 / 1,653 | 0.76 / 4,329 | 0.26 / 1,452 | 1.41 / 2,643 | 0.24 / 199,371 | 2.81 / 34,030 | 1.38 / 8,375 |
| django | 0.82 / 196 | 3.28 / 1,093 | 0.82 / 107 (empty) | 1.39 / 3,116 | 0.81 / 1,719 | 1.32 / 2,575 | 0.81 / 835 | 3.30 / 876 | 0.98 / 974,082 | 1.52 / 4,151 | 5.43 / 6,249 |
| core | 1.45 / 534 | 1.65 / 3,817 | 1.45 / 106 (empty) | **0.83** / 1,366 | 1.45 / 1,600 | **0.81** / 1,196 | 1.44 / 1,914 | 1.69 / 4,159 | 0.70 / 350,226 | 10.00 / 52,978 | 1.50 / 4,368 |

- dctx warm latency grows with repository size because every process re-opens and loads the whole
  generation (core: 370 MiB RSS for any query). Trail's floor is Node startup (~0.6 s, ~100 MiB RSS).
- dctx text mode is thin: `find` prints `id kind path` without line numbers, and `read`/`explain` in text
  mode print only the id line (152 to 247 bytes, no source and no signature). Source needs `--json`
  (TC-F04).
- dctx `--json` repeats thirteen keys per record, many of them `null`. Under the same default budget a
  JSON `find` returned 4 records where the text `find` returned 10 (flask, `dispatch_request`).
- dctx cold query (empty cache, includes indexing, median of 3): flask 0.09 s, ripgrep 0.46 to 1.41 s,
  pocketbase 0.72 s, trailcode 0.31 s (`data/queries_cold_dctx.csv`). Trail cannot answer without a graph
  ("no graph found ... run `trail build` first"), so its cold first answer is the cold build plus a query.
- Trail prepends a banner to `callers`, `skeleton`, `grep`, `map` and source answers. The banner estimates
  "tokens saved" and instructs the agent to tell the user the total each turn: ~310 bytes (~89 tokens) per
  call, 6.3% of all Trail warm-query bytes and 54% of the `callees` answers.

## 6. Freshness and incremental updates (section 2)

Probe: append a new function that calls the hub target to its file (unstaged), then add a new untracked
file, then `git add` it, then revert everything (`data/freshness.csv`).

| Corpus | dctx: new fn found / first query s | Trail: new fn found / first query s | Trail caller edge to new fn | Untracked file found (dctx / Trail) | Staged found | Stale after revert |
|---|---|---|---|---|---|---|
| flask | yes / 0.11 | yes / 1.06 | yes | yes / yes | yes / yes | no / no |
| ripgrep | yes / 0.42 | yes / 1.43 | yes | yes / yes | yes / yes | no / no |
| pocketbase | yes / 0.76 | yes / 2.95 | yes | yes / yes | yes / yes | no / no |
| trailcode | yes / 0.32 | yes / 1.93 | yes | yes / yes | yes / yes | no / no |
| dctx | yes / 2.31 | yes / 4.50 | yes | yes / yes | yes / yes | no / no |
| django | yes / 2.70 | yes / 11.30 | yes | yes / yes | yes / yes | no / no |
| core | yes / 7.48 | yes / 5.45 | yes | yes / yes | yes / yes | no / no |

- Both tools are correct on every freshness check, including uncommitted, unstaged and untracked edits.
  dctx never reports the caller edge because it has no call edges.
- dctx re-indexes everything after any change: the first query after an edit costs about the cold
  time (core 7.48 s versus 1.45 s warm; django 2.70 s versus 0.82 s). Trail re-parses only what moved
  (django 11.3 s versus a 95 s cold build).
- Each changed snapshot adds a complete new dctx generation. After the query and freshness phases, the core
  cache held six 158 MB generations (946 MB); three of them were byte-identical copies of the original
  snapshot. `dctx cache status` reported 473 MB reclaimable, and `dctx cache gc --dry-run` reclaimed 0
  bytes (`blockers: unlink_grace`). Total dctx cache at the end of the run (all corpora including the
  adversarial ones): 1.36 GB. Trail's index sizes stayed constant.

## 7. Call-graph and definition accuracy (section 5)

Definition-level targets (hub, rare, receiver, collision; 16 per corpus), micro-averaged over call sites.
Interface/trait targets are scored at name level separately. Per-target rows: `data/accuracy_targets.csv`.
Misses and false positives: `data/raw/accuracy.jsonl` (redacted for core).

| Corpus | Gold sites | Trail precision | Trail recall | dctx recall | Interface name-level recall Trail / dctx | Find-def rank 1: dctx `find` / Trail `ask` | Def in results: dctx / Trail `ask` |
|---|---|---|---|---|---|---|---|
| flask | 122 | 1.00 | 0.30 | 0 | 1.00 / 0 | 14 / 14 of 20 | 20 / 19 |
| ripgrep | 144 | 1.00 (3 unjudged) | 0.68 | 0 | 0.50 / 0 | 13 / 10 | 20 / 12 |
| pocketbase | 212 | 1.00 | 0.10 | 0 | n/a (no resolvable sites) | 17 / 8 | 20 / 13 |
| trailcode | 105 | 1.00 | 0.96 | 0 | 0.13 / 0 | 14 / 12 | 20 / 14 |
| dctx | 328 | 1.00 | 0.52 | 0 | 1.00 / 0 | 14 / 11 | 20 / 15 |
| django | 297 | 1.00 | 0.31 | 0 | 0.33 / 0 | 17 / 12 | 20 / 16 |
| core | 179 | 1.00 | 0.69 | 0 | 1.00 / 0 | 14 / 12 | 19 / 20 |

By category over all corpora (Trail): hub P 1.00 / R 0.42 (1,187 sites), rare 1.00 / 0.93, receiver
1.00 / 0.56, collision 1.00 / 0.86, interface 0.94 / 0.42. dctx: 0 everywhere.

- **dctx has no call graph in the shipped CLI.** `neighbors --edge calls` returned empty on all 140
  targets; `--edge imports` on module ids was also empty in spot checks (ripgrep, pocketbase). The lexers do extract references (for example
  `EdgeKind::Calls` in `dctx-lex-*`, `dctx-map-parse/src/adapters/rust.rs`), but the facade index
  (`crates/dctx/src/index/extract.rs`) only emits `contains` edges, and `dctx-map-resolve` is linked only
  through `dctx-map-query`.
- **Trail is precise but misses a lot on some corpora.** Typical misses: Go methods called through
  variables (pocketbase `Record.Verified`, 0 of 39; `Record.ValidatePassword`, 0 of 36); Python methods
  called on module-level objects (`apps.get_app_configs()`, 3 of 69); calls through a re-exported module
  name (`flask.abort(...)`, 2 of 22); classmethods called on subclasses (`Index.as_view(...)`, 0 of 20).
  Trail itself says it drops cross-file callers of ambiguous names "rather than guessed".
- **Find definition:** dctx `find` ranked the definition first more often (103/140 versus 79/140 for
  Trail `ask`) and listed it in 139/140 first pages versus 109/140. Trail `callers N` also lists every
  same-name definition and matched the target definition in 136/140 cases.

## 8. Task-level information efficiency (section 6)

Ten questions per corpus (definition and callers, change impact, outline, config key, module exports,
source of a method, callees, repo overview, a colliding name, concept search). Questions and facts:
`data/targets/*.questions.json`; per-question rows: `data/tasks.csv`, `data/tasks_by_question.csv`.

| Corpus | dctx calls / tokens / score / facts | Trail calls / tokens / score / facts | Facts per 1k tokens dctx / Trail |
|---|---|---|---|
| flask | 15 / 9,227 / 11 of 20 / 30 of 48 | 10 / 5,529 / 16 / 39 of 48 | 3.25 / 7.05 |
| ripgrep | 15 / 9,048 / 9 / 16 of 32 | 10 / 25,974 / 13 / 23 of 32 | 1.77 / 0.89 |
| pocketbase | 15 / 25,644 / 11 / 27 of 36 | 10 / 4,026 / 18 / 33 of 36 | 1.05 / 8.20 |
| trailcode | 15 / 11,625 / 10 / 35 of 64 | 10 / 7,529 / 19 / 59 of 64 | 3.01 / 7.84 |
| dctx | 15 / 60,080 / 10 / 21 of 48 | 10 / 15,396 / 19 / 44 of 48 | 0.35 / 2.86 |
| django | 15 / 281,264 / 10 / 29 of 70 | 10 / 7,006 / 18 / 63 of 70 | 0.10 / 8.99 |
| core | 15 / 103,117 / 8 / 11 of 56 | 10 / 23,231 / 19 / 51 of 56 | 0.11 / 2.20 |
| **All** | 105 / 500,005 / **69 of 140** / 169 of 354 | 70 / 88,691 / **122 of 140** / 312 of 354 | 0.34 / 3.52 |
| All except overview | 98 / 19,156 / 55 of 126 / 137 of 322 | 63 / 56,989 / 109 of 126 / 284 of 322 | **7.15** / 4.98 |

By question type (score out of 14): outline dctx 11 / Trail 14; exports 8 / 14; source 14 / 12;
definition and callers 7 / 10; impact 7 / 13; callees 2 / 8; collision 6 / 12; config key 0 / 14 (dctx has
no text search); concept search 0 / 12 (multi-word `dctx map find` returned no records); overview 14 / 13
(dctx through the unbounded legacy tree: 480k tokens over seven corpora versus 31.7k for `trail map`).

## 9. Cache-friendliness (section 7)

Each warm query ran 5 times, then again after appending one comment line to an unrelated file in another
directory (`data/stability.csv`).

| Property | dctx | Trail-Code |
|---|---|---|
| Text output identical over 5 runs | yes (all text verbs) | yes |
| JSON output identical over 5 runs | **no**: `freshness.age_s` changes every second | yes |
| Identical after an unrelated edit | **no, every answer**: header `# <place> <verb> v1:sha256:<digest>:<bytes> g=<n>` changes; common prefix 21 to 50 bytes | yes for callers, callees, outline, grep, read, text find; `map` banner counts change; `ask --json` scores churn in the 9th to 16th digit |
| Absolute paths in output | none in map verbs; legacy `dctx .` prints the absolute path and a timing line | `ask --json` includes absolute paths of `.trail/learnings` files |
| Timestamps | `age_s` in JSON | none in answers |
| Ordering | stable | stable |

## 10. Robustness and privacy: adversarial corpus (section 10)

`scripts/gen_adversarial.py` plants: a non-ignored `.env` with `DCTXBENCH_FAKE_SECRET=sk-test-...`, a
gitignored directory and a gitignored `*.secret.py` with unique markers, an un-ignored `node_modules/`, a
NUL-containing `.py`, a 50 MB text file, a 5 MB single-line minified JS, a 5 MB generated Python module
with 50,000 functions, a 100 KB line, a symlink loop and a symlink escaping the repository, a non-UTF-8
file name, a file name with a space and one with a newline, 200 nested directories, a CRLF file, empty and
BOM-only files, and 13 files in languages dctx does not support. Run twice: as a git repository and as a
plain directory. Timeout 300 s per command (`data/raw/adversarial.jsonl`).

| Check | dctx | Trail-Code |
|---|---|---|
| Crash or hang | none in map verbs or `pack`; legacy `dctx . -t` exits 1 with `resource limit exceeded: Depth > 128` | none |
| Gitignored files (git repo) | not indexed, not in `pack` | not indexed (`trail grep` names the ignored paths in its footer) |
| Gitignored files (no `.git`) | **still excluded** (rules read from `.gitignore`) | **indexed**: the gitignored secret value is in `graft/.cache/extract.*.json` and in `trail grep sk-test` output |
| `.env` secret | **printed by `dctx pack .`** (not in map answers or the map cache) | never read |
| Symlink loop / escaping symlink | not followed | not followed |
| `node_modules/` (not ignored) | skipped | skipped |
| NUL-containing `.py` | skipped as binary | indexed |
| 5 MB minified JS, 5 MB generated `.py` | indexed (last of 50,000 functions found) | skipped |
| Non-UTF-8 file name | skipped silently | skipped |
| Newline in file name | skipped | indexed |
| CRLF file | correct outline, read and spans | correct skeleton and callers |
| 200-deep nesting | indexed by map | indexed |
| Unsupported languages (13 files) | 0 indexed | 8 indexed (Zig, Elixir, Lua, OCaml, Scala, Dart, Solidity, Vue) |
| Cost on this corpus | first query 2.6 s, 262 MiB RSS; cache 45.9 MB | build 1.5 s, 277 MiB RSS; index 0.13 MB |
| Repository writes | none (legacy `dctx .` writes `.dctx/`) | `.gitignore` modified, `.ignore` added |

## 11. Network and telemetry audit

`strace -f -e trace=socket,connect,sendto,sendmsg,bind` inside the network namespace (`data/raw/network.jsonl`).

| Run | Sockets | Notes |
|---|---|---|
| `dctx map find`, `dctx pack .`, `dctx plugin list` | 0 | no socket syscalls at all |
| `trail build .` (telemetry on, and with `DO_NOT_TRACK=1`) | 0 | |
| `trail callers` with the daily update-check stamp expired, **with and without `DO_NOT_TRACK=1`** | 30 | resolves `registry.npmjs.org` through systemd-resolved's varlink socket (which works from inside a network namespace), then tries TCP 443 on 104.16.x.34 and 2606:4700::6810:x; connects fail with `ENETUNREACH` |
| any Trail command with telemetry on, offline | 0 | events are queued in `~/.graft/telemetry-queue.ndjson`: app version, OS, arch, Node major, command, a random `repo_id` UUID, file-count bucket, languages, duration bucket |
| `npm install` (network allowed) | n/a | `postinstall` recorded an install event and spawned a detached flush (`flushedAt` set in `~/.graft/telemetry.json`); delivery cannot be confirmed |

Trail's documented telemetry gates (`DO_NOT_TRACK`, CI, `trail telemetry disable`) did stop the usage
pings. They do not gate the update check.

## 12. Files written by `trail init` (flask, `-y`)

- Repository (agent names replaced by `<agent>`): `.<agent>/settings.json`, `.<agent>/settings.local.json`,
  `.github/<agent>-instructions.md`, `.trail/.gitignore`, `.trail/README.md`, `.trail/hook.cjs`, `.trail/statusline.cjs`, the index under
  `.trail/cache/` (90 files).
- `HOME` (machine-wide): `~/.<agent>/settings.json` (SessionStart, UserPromptSubmit, PostToolUse, Stop and
  SessionEnd hooks), `~/.<agent>/skills/trail/SKILL.md`, `~/.<agent>/helpers/graft-hooks.cjs`,
  `~/.trail/README.md`, `~/.trail/install.json`, `~/.trail/repos/index.json`, `~/.trail/repos/known.json`.
  `--dry-run` lists all of them first; `--no-global` skips the home directory.
- `trail uninstall -y` removed the repository wiring and the `~/.<agent>` files, but left
  `.trail/README.md`, `.trail/.gitignore` and the four `~/.trail/` files behind.
- dctx has no init step. `dctx map` wrote nothing into the repositories or into `HOME`.

## 13. Bounding (section 8)

- dctx map verbs: `--budget` 1..2000 (explain up to 1000) and `--limit` up to 40 are enforced. All 49
  budgeted requests that returned output stayed within budget, as did every broad query at the 2,000-token
  cap. The other 35 of 84 budgeted requests (budgets 20 and 100) failed with exit code 1 ("budget too
  small: needs at least 102 bytes") instead of returning a shorter page (TC-F09).
  Worst map answer on any corpus: 6,996 bytes (1,999 tokens).
- dctx outside map: `dctx . -t` wrote 974,082 bytes (278k tokens) for django and failed on the deep tree.
  `dctx pack .` printed 1.59 MB for flask and 8.67 MB for the adversarial corpus to stdout.
- Trail has no budget option. Largest single answers: `skeleton` of a 140 KB generated `types.d.ts`
  142,197 bytes (40.6k tokens); `ask --source --full -n 50` 70,466 bytes; `map --max-dirs 100000` 53,870
  bytes on core (9.9 s). `grep` is capped internally (15 to 22 KB even for `grep e`), and `ask -n 1000`
  returned at most 2.2 KB.

## 14. Language coverage (section 9)

`data/coverage.csv`: Trail's numbers come from its full graph; dctx's from a seeded sample of up to 12 files
per extension (`dctx map map FILE --depth 1 --limit 1`).

- Source files: both tools give symbols to 83 to 100% of sampled `.py`, `.rs`, `.go` and `.ts` files
  (the rest are empty modules and `__init__`-style files). Trail gave symbols to 2,010 of 2,919 django
  `.py` files, 1,283 of 1,420 core `.rs`, 450 of 457 pocketbase `.go`, 292 of 316 trailcode `.ts`.
- JavaScript: Trail indexed 16 of 111 django `.js` files and 177 of 251 pocketbase `.js` files (vendored
  and large files skipped); dctx produced symbols for 10 of 12 and 8 of 12 sampled files.
- Docs and config: dctx gives structure to `.md`, `.json`, `.toml`, `.yaml` and `.yml` (for example 12 of
  12 sampled dctx `.json` and `.toml`); Trail gives none.
- Language list: Trail claims 23 languages and indexed 8 of the 13 extra languages in the adversarial
  corpus. dctx documents 15 language families and indexed none of those 13.

## 15. Where dctx wins / where Trail-Code wins / ties

**dctx wins**
- Footprint: 9.2 MB static-ish binary versus 405.6 MB of Node packages; no install scripts.
- Privacy: no sockets in any run; no repository or home writes in map mode; `.gitignore` honoured even
  without `.git`; gitignored secrets never indexed.
- Cold index: 2.8 to 69 times faster and 1.6 to 27 times less peak memory on all seven corpora.
- Warm latency on small and medium repositories: 30 to 260 ms versus 0.6 to 1.4 s.
- Hard output bounds on every map verb.
- Finding definitions: rank 1 in 103/140 cases versus 79/140 for `trail ask`.
- Cheap no-op verification (0.04 to 1.34 s) versus `trail build --check` (2 to 86 s).
- Structure for Markdown, JSON, TOML and YAML; large generated and minified files are still indexed.

**Trail-Code wins**
- Call graph: precise callers and callees with depth, recall 0.10 to 0.96, versus none in dctx.
- Task answers: 122/140 versus 69/140 points; config lookup, concept search, impact and callees questions.
- Text search (`trail grep`, grouped by enclosing symbol) and a bounded repository map (1 to 15k tokens).
- Cache-friendly output: answers do not change when unrelated files change.
- Incremental re-indexing; edits reach the graph without a full rebuild.
- Language breadth (23 languages) and MCP integration.
- On the largest Rust corpus, warm `callers` and `skeleton` are faster than dctx (0.8 s versus 1.45 s) with
  less memory per query.

**Ties**
- Freshness correctness on unstaged, untracked, staged and reverted edits.
- Neither follows symlink loops or escaping symlinks; both skip `node_modules/` and non-UTF-8 file names;
  neither crashed or hung on the adversarial corpus in its primary interface.
- Both produce byte-identical text output for identical repeated queries.

## 16. Findings

Each finding has an ID, severity, evidence and a recommendation that keeps dctx's philosophy: zero
dependencies, a pure Rust core, no Git or network in the core, bounded outputs, privacy, and external
effects only through consent-gated plugins.

**TC-F01 (blocker): the shipped map CLI has no call or import edges.**
Evidence: `neighbors --edge calls` empty for 140 of 140 targets across seven corpora (`data/accuracy.csv`);
`--edge imports` empty on module ids in spot checks; Trail precision 0.94 to 1.00 and recall up to 0.96 on the same gold.
In the task benchmark, dctx scored 7/14 on definition-and-callers questions, 7/14 on impact, 2/14 on
callees and 6/14 on colliding names. The facade (`crates/dctx/src/index/extract.rs`) only emits `contains`
edges, although the lexers already produce `Calls`/`Imports` references and `dctx-map-resolve` exists.
Recommendation: wire reference facts from the lexers through `dctx-map-resolve` into the facade segment,
with explicit confidence (`exact`, `name`, `guess`) on every edge and a record of dropped ambiguous
targets. Start with same-file, same-module and imported-name resolution, which is enough for Trail's
"rare" and "collision" recall (0.86 to 0.93). Gate any typed resolution behind the precision label
instead of adding a type checker. Use `scripts/phase_accuracy.py` as the acceptance test.

**TC-F02 (major): no incremental index, and generations grow without deduplication.**
Evidence: the first query after a one-line edit costs about a full cold index (core 7.48 s versus 1.45 s
warm; django 2.70 versus 0.82). The core cache grew from 158 MB to 946 MB after about five edits; three
generations were byte-identical; `cache gc --dry-run` reclaimed 0 bytes (`unlink_grace`). Total cache
at the end of the run: 1.36 GB.
Recommendation: store per-file fact segments addressed by content hash and assemble generations from
references, so an edit re-lexes one file and an unchanged snapshot reuses its predecessor. Reclaim
superseded unpinned generations of the same root right away (or at the next run) instead of only after the
grace and TTL windows.

**TC-F03 (major): outputs are not prefix-stable or deterministic enough for prompt caching.**
Evidence: every map answer starts with `# <place> <verb> v1:sha256:<digest>:<bytes> g=<n>`, so any edit
anywhere changes byte 21 to 50 of every answer (`data/stability.csv`, 0 of 63 dctx answers unchanged after
an unrelated edit versus 59 of 70 for Trail). JSON answers contain `freshness.age_s`, so no two JSON calls
more than a second apart are identical.
Recommendation: move snapshot identity and age to a trailer line or behind an opt-in flag; keep the first
bytes of an answer a pure function of the answered nodes. Report age in coarse buckets or not at all in the
default output.

**TC-F04 (major): text mode is too thin for agents.**
Evidence: `dctx map read <id>` and `explain <id>` in text mode print only the id line (152 to 247 bytes,
no source, no signature); `find` text has no line numbers; JSON carries thirteen keys per record, often
`null`, so a default-budget JSON `find` returned 4 records versus 10 in text.
Recommendation: print source for `read` and the signature and summary for `explain` in text mode; add
`path:start-end` to text records; drop `null` keys from JSON v2 (or add a compact JSON).

**TC-F05 (major): no bounded repository overview.**
Evidence: `dctx map map p:<place>` and `k:<dir>` answer `gone`, and directories are refused ("is a
directory; pass a file path or a stable id"), although the guide says `map` outlines "the project root".
The only overview is the legacy `dctx . -t`: 974 KB (278k tokens) on django, written into the repository
as `.dctx/<place>.llmcontext.txt`, and it fails on trees deeper than 128. Trail's `map` answered in 1.7 to
53 KB.
Recommendation: implement `map p:` and directory targets as a budgeted overview (directories, file
counts, top symbols by in-degree once TC-F01 lands), paging through `next`, written to stdout only.

**TC-F06 (major): no text search.**
Evidence: config-key questions scored 0/14 for dctx and 14/14 for Trail (`trail grep`).
Recommendation: add a bounded `dctx map grep` verb in the core (pure Rust, same ignore rules, no Git),
grouped by enclosing symbol id and capped by `--budget` with a continuation cursor.

**TC-F07 (major): large data files dominate cost.**
Evidence: on core one 6.7 MB JSON fixture produced 235,842 symbols; the generation is 158 MB (2.9 times
the tree); every warm query re-reads it (1.45 s, 370 MiB RSS). Trail answers the same corpus with a 65 MB
index and 0.8 s queries. The adversarial corpus needed 262 MiB for its first query because of its 5 MB source files.
Recommendation: cap configuration and data extraction per file (bytes or node count), mark capped files
as `opaque` in the precision label, and load only the segment parts a query touches instead of the whole
generation in every process.

**TC-F08 (major, privacy): `dctx pack` emits `.env` contents by default.**
Evidence: `dctx pack .` printed `DCTXBENCH_FAKE_SECRET=sk-test-...` from a non-gitignored `.env`
(`data/raw/adversarial.jsonl`, both variants). Map answers and the map cache did not contain it. Trail
never read `.env`.
Recommendation: ship a default sensitive-content omit list (`.env*`, `*.pem`, `*.key`, `id_*`,
`credentials*`, `*.p12`) that shows the path but never reads the content unless the user opts in. This
matches the README's "never read" promise for secrets.

**TC-F09 (minor): small budgets fail instead of degrading.**
Evidence: `--budget 20` and JSON `--budget 100` exit 1 ("budget too small: needs at least 336 bytes").
Recommendation: return the largest prefix that fits (ids only, then `more`/`next`), or clamp to the floor
and say so in the precision label.

**TC-F10 (minor): concept search finds nothing for multi-word queries.**
Evidence: all seven concept queries (multi-word, for example "wsgi app") returned an empty page (about
100 bytes, header only); Trail `ask` found the target in 6 of 7.
Recommendation: split queries and identifiers on case and `_` boundaries, rank by token overlap with names
and first-line doc summaries (already present as `summary`), all in the core without models.

**TC-F11 (info): "Zero Dependencies" does not describe the built CLI.**
Evidence: `cargo tree -p dctx-cli -e normal` lists 46 external crates, including `rusqlite` with bundled
SQLite C, `ed25519-dalek`, `landlock`, `seccompiler`, `serde_json` and `syn` 3, all compiled into the
release build (`target/release/deps`). The binary itself only needs libc, libm and libgcc_s at run time.
Recommendation: either feature-gate the plugin, sandbox and store crates out of the default build, or
change the badge and README to "no runtime dependencies" and list the audited crates.

**TC-F12 (minor, docs): `docs/guides/MAP-MODE.md` and `--version --verbose` are stale.**
Evidence: the guide says `.gitignore` rules are not loaded and `--lines` drops the start line; observed:
gitignored files are excluded on the map path even without `.git`, and `--lines 240-246` returned exactly
lines 240 to 246. The guide says `map` outlines the project root (it does not, TC-F05) and that the default
CLI extracts "conservative calls" (none are exposed, TC-F01). `dctx --version --verbose` lists
`languages: rust,javascript,typescript,markdown` and parser `dctx-lex-rust/syn-3.0.9`, while answers report
`dctx-structural/1` and Python, Go and others work.
Recommendation: regenerate the guide's limitation section and the verbose version block from tests.

**TC-F13 (minor): silent omissions.**
Evidence: files with a non-UTF-8 name or a newline in the name, and NUL-containing source files, were
skipped without any signal in the answer or the precision label (Trail also skips non-UTF-8 names).
Recommendation: count omitted paths by reason in `precision` (for example `omitted: {binary: 1,
unnamed: 2}`) so an agent knows the map is incomplete.

**TC-F14 (info): footprint, privacy and speed are real advantages; keep them.**
Evidence: 9.2 MB binary versus 405.6 MB; 0 sockets versus a daily update check that ignores
`DO_NOT_TRACK` and an install-time telemetry event; no repository writes versus `.gitignore`/`.ignore`
edits on every Trail build; cold index 2.8 to 69 times faster on all seven corpora; definition ranking 103/140
versus 79/140. Recommendation: keep these as non-negotiable acceptance gates while closing TC-F01 to F07.

**TC-F15 (info): do not copy Trail's in-band agent instructions.**
Evidence: Trail prepends a ~310-byte banner to most answers that tells the agent to report "tokens saved"
to the user (~89 tokens per call, 54% of `callees` output). It is prompt steering inside tool output and it
changes when covered files change.
Recommendation: keep dctx answers free of instructions; if savings metrics are useful, expose them in
`dctx cache status` or a JSON field the caller can drop.

**TC-F16 (info): MCP surface.**
Evidence: Trail's MCP server exposes 6 tools whose definitions cost ~1,005 tokens per session, and answers
match the CLI. dctx has no MCP server (planned).
Recommendation: add MCP as a consent-gated adapter over the same facade, with at most four tools and a
definition budget under 600 tokens, reusing map ids and budgets.

**TC-F17 (info): Trail's freshness check costs a full build.**
Evidence: `trail build --check` 49.5 s on pocketbase and 86.5 s on django, the same as cold builds;
dctx verifies freshness on every query in 0.04 to 1.34 s.
Recommendation: keep strict per-query verification as a dctx default and document it as a feature.

**TC-F18 (info): Trail indexes gitignored files when `.git` is absent.**
Evidence: in the no-git adversarial variant, Trail indexed `ignored_dir/` and `app.secret.py`; the fake
secret value appeared in `graft/.cache/extract.*.json` and `trail grep` output. dctx excluded both.
Recommendation: keep dctx's rule-based ignore handling independent of Git, and add this case to the dctx
privacy test suite.

## 17. Not tested, and deviations

- Trail `build --deep` (needs an LLM key) and `--lsp` (no language servers installed on the host);
  `trail blast`, `check`, `note`, `team`, `viz` and the learnings features; Trail cloud workspaces.
- Callees and depth-2 accuracy were not adjudicated (latency, size and task facts only).
- End-to-end agent outcomes (no model was run); exact tokenizer counts (estimator only).
- Page-cache-cold timings (caches were not dropped on a shared host); other operating systems.
- dctx plugins, TUI, templates and the optional `syntax-rust` build.
- Deviation: `DO_NOT_TRACK` was not set during `npm install`, so Trail's `postinstall` recorded an install
  event and attempted a flush while the network was available. All later Trail runs had the opt-out set
  and ran without network.
- Deviation: the network namespace does not block name resolution through systemd-resolved's varlink
  socket. Only the deliberate update-check audit (stamp removed) triggered a lookup; the stamp was
  restored before any other run, and runs with a fresh stamp made no socket calls.
- Deviation: the flask freshness, bounding and coverage rows come from the validation run of the final
  scripts; the other corpora ran in one chained pass (`data/raw/phases.jsonl`).
- dctx's overview row uses the legacy snapshot alias because the map verbs have no repository overview.

## 18. Reproducing

1. On a Linux host with passwordless sudo: `scripts/setup_host.sh` (user, workspace, dctx build, pinned
   Trail install, pinned clones). Copy `scripts/` to `$BENCH_ROOT/scripts`.
2. `scripts/run_all.sh` runs every phase. Each phase script appends JSONL under `$BENCH_ROOT/results`.
3. `python3 scripts/aggregate.py RESULTS data/` regenerates `data/*.csv` and `data/SUMMARY-TABLES.md`.
   For a private corpus, run `scripts/redact_private.py data/ <corpus>` before publishing.
4. Cleanup: `sudo userdel -r dctxbench && sudo rm -rf /path/to/workdir && sudo apt-get remove -y time strace`.

All scripts are POSIX sh or Python 3 standard library only.

### Data index

| Path | Content |
|---|---|
| `data/env.txt`, `data/tool-help.txt` | host, versions, pins, binary hash, `ldd`, CLI help of both tools |
| `data/raw/runs.jsonl` and `*_runs.jsonl` | every measured command: wall, CPU, RSS, exit code, output bytes and sha256, load |
| `data/raw/accuracy.jsonl`, `tasks.jsonl`, `freshness.jsonl`, `bounding.jsonl`, `coverage*.jsonl`, `adversarial.jsonl`, `network.jsonl`, `mcp.jsonl`, `init_writes.jsonl`, `writes.jsonl`, `index_size.jsonl` | per-phase results |
| `data/raw/strace.*.txt` | network audit traces |
| `data/targets/` | targets, gold call sites and task questions per corpus (core reduced to counts) |
| `data/outputs/` | first 16 KB of each raw answer (warm, unrelated-edit, accuracy and adversarial runs; core excluded) |
| `data/*.csv`, `data/SUMMARY-TABLES.md` | aggregated tables |
