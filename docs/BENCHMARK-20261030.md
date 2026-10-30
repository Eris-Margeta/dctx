# dctx vs Trail-Code vs graphify: three-way benchmark (30 October 2026)

One command, same host, same corpora, same CPU set, no network, pinned versions. Raw aggregate, headline tables
and the harness are in [`research/bench-3way-20261030/`](research/bench-3way-20261030/); the gap analysis behind
the work is [`GAP-MATRIX-20261030.md`](GAP-MATRIX-20261030.md). Where another tool wins, the tables say so.

- **dctx**: `main` at `ac8408c` (the following commit only changes `CHANGELOG.md`), release build,
  sha256 `82bd48760932305748a00d2b32901c667af948f602822b9816cc5c545de7ebbc`.
  Baseline for "before" columns: `main` at `4660f6d`, the state before this night's work.
- **Trail-Code** `@trailhq/trail-code` 0.28.0 from npm, `trail build .` (no `--deep`, no `--lsp`).
- **graphify** `graphifyy` 0.9.97 from PyPI, LLM-free code path only (`extract --code-only`,
  `GRAPHIFY_BACKEND=none`). Its LLM pass over docs, PDFs and images was **not** run, and nothing here measures it.

## Headline

| Area | Result |
|---|---|
| Callers (depth 1) against a tool-independent gold set, 80 targets on 4 corpora | **dctx leads on recall at equal precision**: P 0.993 / R 0.461 (first page), P 0.995 / R 0.706 following up to 5 pages. Trail-Code P 0.986 / R 0.436, graphify P 0.989 / R 0.196. Per corpus, Trail-Code wins ripgrep and trailcode, and dctx wins flask and pocketbase. Before tonight dctx recall was 0. |
| Cold index | **dctx fastest on all 6 corpora**: 0.16–1.16 s against 1.17–58.3 s for Trail-Code and 4.5–59 s for graphify. Peak RSS 19–90 MB against 121–1,020 MB and 74–394 MB. |
| Warm queries (median of 5, including process start) | **dctx fastest** on search, callers and skeleton everywhere: 44–226 ms against 0.98–1.9 s for Trail-Code and 0.38–1.4 s for graphify. graphify's overview is a 7 ms read of a pre-written file. |
| Answer size | dctx `map hubs` gives the smallest overview on 4 of 6 corpora (Trail-Code on trailcode and docs) (30–652 tokens; Trail-Code 18–2,538; graphify 3,827–11,591). graphify gives the smallest callers and skeleton answers. dctx's skeleton is the largest of the three on ripgrep (1,784 tokens). Before tonight dctx's only overview was a 0.45–2.6 M-token `pack`. |
| Cache-friendliness | dctx and Trail-Code tie. Both are byte-identical across runs, and after an unrelated edit only the overview changes. graphify changed find, skeleton and overview after `update`, and 1 of 6 ripgrep queries was not deterministic. |
| Network | dctx and graphify made 0 socket attempts in every variant. Trail-Code made 49 blocked connect attempts when `DO_NOT_TRACK` is unset, and 0 with it set. |
| Language coverage, mixed corpus (10 languages) | **Symbols: dctx 10 of 10, Trail-Code 7, graphify 5.** **Call edges: Trail-Code 7, graphify 5, dctx 0.** dctx's new outline tier is symbols only, labelled `guess`. Terraform and SQL get symbols only from dctx. |
| Plain-text docs (.md/.rst/.txt/.adoc corpus) | dctx indexes the corpus into headed sections (0.18 s). graphify's code-only mode produced no index, and Trail-Code indexes no doc nodes. |
| Privacy canary (git-tracked `.env` files) | **dctx still leaks on one path.** `pack`, `find` and `grep` are clean now; before tonight `pack` printed the canary. But an explicit `dctx map read m:.env` returns the file. Trail-Code and graphify leaked nothing. The fix is card DCX-30, which has not landed. |

## Run 2: call edges for the outline tier (main `99da638`, 30 October, 07:24 UTC)

Same harness, corpora and tool versions. The dctx binary's sha256 is `81b35b3eb5112921c4f773b2682c247ec667bfc3487ce0670aa5e3ec5a372d14`. The new commits are DCX-31a (`8859210`, brace family) and DCX-31c (`99da638`, infra family). Raw results are in `research/bench-3way-20261030/results/run2-99da638/`.

| Mixed corpus, ≥1 call edge | Run 1 | Run 2 |
|---|---|---|
| dctx | 0 of 10 | **6 of 10** (Dart, Zig, Solidity, Terraform, SQL, shell) |
| Trail-Code | 7 of 10 | 7 of 10 (no Terraform, SQL or shell) |
| graphify | 5 of 10 | 5 of 10 |

dctx is the only tool with call edges for Terraform and SQL. Scala still has none on real code, although the fixture passes; that is a follow-up. Lua, Elixir and OCaml call edges are card DCX-31b, still in flight. Cold index (0.13–1.06 s), callers precision and recall, and the canary result are unchanged from run 1. The `map read m:.env` leak remains until DCX-30 lands.

## Tables (run 1)

### Cold index (median wall s, peak RSS of the largest process, index size)

| corpus | dctx | Trail-Code | graphify |
|---|---|---|---|
| flask (Python) | 0.16 s, 19 MB, 1.1 MB | 4.17 s, 225 MB, 4.4 MB | 8.13 s, 102 MB, 4.5 MB |
| ripgrep (Rust) | 0.54 s, 25 MB, 2.2 MB | 3.01 s, 303 MB, 8.4 MB | 10.45 s, 93 MB, 8.6 MB |
| pocketbase (Go+JS) | 0.91 s, 63 MB, 9.9 MB | 50.63 s, 987 MB, 41.8 MB | 58.96 s, 243 MB, 26.3 MB |
| trailcode (TS) | 1.16 s, 90 MB, 14.8 MB | 58.34 s, 1,020 MB, 41.1 MB | 44.32 s, 394 MB, 11.9 MB |
| mixed (10 langs) | 0.24 s, 74 MB, 2.8 MB | 3.84 s, 470 MB, 12.8 MB | 4.46 s, 74 MB, 2.2 MB |
| docs | 0.18 s, 74 MB, 1.4 MB | 1.17 s, 121 MB, 0.0 MB | failed: no index |

### Callers accuracy (micro precision / recall, depth 1)

| corpus | dctx before (4660f6d) | dctx now | Trail-Code | graphify |
|---|---|---|---|---|
| flask | – / 0.000 | 1.000 / 0.836 | 0.962 / 0.328 | 0.933 / 0.195 |
| ripgrep | – / 0.000 | 0.981 / 0.246 | 0.987 / 0.327 | 1.000 / 0.032 |
| pocketbase | – / 0.000 | 1.000 / 0.667 | 1.000 / 0.099 | 1.000 / 0.383 |
| trailcode | – / 0.000 | 0.983 / 0.332 | 0.989 / 0.902 | 1.000 / 0.215 |
| all | – / 0.000 | **0.993 / 0.461** | 0.986 / 0.436 | 0.989 / 0.196 |

### Warm latency and answer size (median ms, estimated tokens)

| corpus | query | dctx | Trail-Code | graphify |
|---|---|---|---|---|
| flask | search | 81 ms, 218 | 1,066 ms, 262 | 564 ms, 1,845 |
| flask | callers | 47 ms, 479 | 988 ms, 136 | 381 ms, 32 |
| flask | skeleton | 44 ms, 866 | 991 ms, 852 | 514 ms, 474 |
| flask | overview | 45 ms, 491 | 1,009 ms, 1,122 | 7 ms, 5,930 |
| pocketbase | search | 213 ms, 259 | 1,573 ms, 358 | 1,393 ms, 97 (exit 3) |
| pocketbase | callers | 214 ms, 464 | 1,195 ms, 175 | 777 ms, 24 |
| pocketbase | overview | 226 ms, 652 | 1,232 ms, 1,514 | 8 ms, 11,591 |
| trailcode | callers | 200 ms, 472 | 1,180 ms, 1,064 | 715 ms, 196 |
| trailcode | overview | 195 ms, 500 | 1,159 ms, 499 | 7 ms, 10,076 |

All corpora and the edit-stability, network and coverage tables are in
[`research/bench-3way-20261030/results/headline.md`](research/bench-3way-20261030/results/headline.md).

## Method

- Host build-host (Debian 13, Xeon E5-2680 v2), shared with other jobs. Load was 3.2 at the start, so absolute
  latencies are noisy. Every measured command ran as `nice -n 10 taskset -c 24-31 unshare -Urn env -i …`, with no
  network, scratch HOME and caches, and `DO_NOT_TRACK=1`. One heavy process ran at a time. The network audit is a
  ptrace socket tracer, because strace is not installed.
- Corpora are pinned by SHA in `research/bench-3way-20261030/corpora-shas.json`: flask 3.1.3, ripgrep 15.2.0,
  pocketbase v0.40.5, Trail-Code at the 0.28.0 release commit, a mixed corpus of one public repo for each of 10
  languages, and a docs corpus built from python/devguide, rust-lang/book and asciidoctor.
- Tokens are `ceil(stdout_bytes / 3.5)` for every tool. That is an estimate, not a tokenizer.
- Cold runs use 3 repetitions, except 1 for any tool whose first run took more than 30 s (Trail-Code and graphify
  on pocketbase and trailcode). Warm runs take the median of 5 after one discarded warm-up.
- The callers gold set is the regex oracle of the October protocol (`goldlib.py`) on 20 seeded targets per corpus.
  A hit is true when its span contains a gold call site.
- Query mapping: dctx `map find` / `map neighbors --dir in --edge calls` / `map skeleton` / `map hubs`.
  Trail-Code `ask` / `callers` / `skeleton` / `map`. graphify `query` / `affected --relation calls` /
  `explain <file>` / `GRAPH_REPORT.md`.

## Not claimed

- Task-level answer quality, meaning the 70-question protocol with a frozen model. It was not re-run, and in
  October Trail-Code led it, 122 to 69 points.
- Anything about graphify's LLM semantic graph, or Trail-Code's learnings and LSP edges.
- SWE-bench or agent-run numbers.
- Equal language depth. dctx's 26 new formats are outline tier: symbols, not call edges.

## What changed in dctx tonight (all on `main`, not pushed)

Call and import edges (`19e90a1`), byte-stable answers (`ce18608`), `.env` omission in pack (`6d4deec`),
outline symbols for 26 formats and plain-text sections (`9c61ac9`, `e0821e3`, `839bd45`, `a32dad3`),
neighbors depth 4 (`0de5ce1`), `hubs` (`6bc0cbc`), `grep` (`5691234`), `skeleton` (`2243c38`), `path` (`670e9e3`),
`export` (`ac8408c`), `dctx mcp` (`50bbce4`) and a 1 GiB source ceiling (`5d26781`). Every card's own check, and
then all 15 checks together on `ac8408c`, passed on build-host before the benchmark ran.

## Next

1. DCX-30: map mode must never serve env-shaped files. This is the remaining privacy leak.
2. Call edges for the outline tier, to close the 0-of-10 call-edge coverage on the mixed corpus.
3. Recall on ripgrep and trailcode (0.25 and 0.33 against Trail-Code's 0.33 and 0.90): method calls through
   receivers and re-exports.
4. Re-run the 70-question task protocol with a frozen model.
