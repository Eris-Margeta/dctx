<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# dctx measurement: grep plus file reads first

The baseline for choosing dctx is **grep plus reading files**. Count the whole
agent session before claiming a cost saving. Output bytes and estimated output
tokens measure only the returned context; they exclude startup instructions,
tool descriptions, conversation replay, model output and provider cache effects.

## Grep plus file reads

The [11 November whole-session benchmark](https://github.com/Eris-Margeta/dctx/blob/main/docs/BENCHMARK-20261111.md) measured grep plus reads against dctx CLI: CLI median total tokens -5.01 %; the pre-registered 25 % savings rule was not met.
Official strict citation recall medians (A/B/C): 10.10 % / 14.46 % / 13.41 %; diagnostic file-level recall: 53.77 % / 71.70 % / 76.42 %. The claim is accuracy and bounded context, not token savings.
On this corpus and model dctx did not save 25 % tokens. The linked report gives median [min, max], methods and limitations; corpus and raw answers are private.

## Secondary historical evidence: other-tool protocols

Everything below retains the October/November research record, including its
older limitations and plans. These other-tool comparisons are historical notes,
not the product's headline baseline or a recommendation between tools. All
"tokens" in these tables mean estimated **tool stdout tokens**, calculated as
`ceil(stdout_bytes / 3.5)`, never whole-session tokens. Task scores are fact
coverage in raw outputs without a model, not agent completion rates.

One command, same host, same corpora, same CPU set, no network, pinned versions. Raw aggregate, headline tables
and the harness are in [`research/bench-3way-20261030/`](https://github.com/Eris-Margeta/dctx/tree/main/docs/research/bench-3way-20261030); the current goals are in the [specification](https://www.dctx.internet.technology/docs/spec/). Where another tool wins, the tables say so.

- **dctx**: `main` at `ac8408c` (the following commit only changes `CHANGELOG.md`), release build,
  sha256 `82bd48760932305748a00d2b32901c667af948f602822b9816cc5c545de7ebbc`.
  Baseline for "before" columns: `main` at `4660f6d`, the state before this night's work.
- **Trail-Code** `@trailhq/trail-code` 0.28.0 from npm, `trail build .` (no `--deep`, no `--lsp`).
- **graphify** `graphifyy` 0.9.97 from PyPI, LLM-free code path only (`extract --code-only`,
  `GRAPHIFY_BACKEND=none`). Its LLM pass over docs, PDFs and images was **not** run, and nothing here measures it.

## Historical three-way summary

| Area | Result |
|---|---|
| Task-level questions, October protocol (70 questions) | **Run 7: dctx 140 of 140, Trail-Code 122, graphify 96**, with dctx using 29.3k tokens against Trail-Code's 88.9k and graphify's 197k. See "Run 7" below. |
| Callers (depth 1) against a tool-independent gold set, 80 targets on 4 corpora | **Run 5: dctx P 0.995 / R 0.851 (first page), R 0.968 following 5 pages.** Run 1 below: dctx P 0.993 / R 0.461 (first page), P 0.995 / R 0.706 following up to 5 pages. Trail-Code P 0.986 / R 0.436, graphify P 0.989 / R 0.196. Per corpus, Trail-Code wins ripgrep and trailcode, and dctx wins flask and pocketbase. Before tonight dctx recall was 0. |
| Cold index | **dctx fastest on all 6 corpora**: 0.16–1.16 s against 1.17–58.3 s for Trail-Code and 4.5–59 s for graphify. Peak RSS 19–90 MB against 121–1,020 MB and 74–394 MB. |
| Warm queries (median of 5, including process start) | **dctx fastest** on search, callers and skeleton everywhere: 44–226 ms against 0.98–1.9 s for Trail-Code and 0.38–1.4 s for graphify. graphify's overview is a 7 ms read of a pre-written file. |
| Answer size | dctx `map hubs` gives the smallest overview on 4 of 6 corpora (Trail-Code on trailcode and docs) (30–652 tokens; Trail-Code 18–2,538; graphify 3,827–11,591). graphify gives the smallest callers and skeleton answers. dctx's skeleton is the largest of the three on ripgrep (1,784 tokens). Before tonight dctx's only overview was a 0.45–2.6 M-token `pack`. |
| Cache-friendliness | dctx and Trail-Code tie. Both are byte-identical across runs, and after an unrelated edit only the overview changes. graphify changed find, skeleton and overview after `update`, and 1 of 6 ripgrep queries was not deterministic. |
| Network | dctx and graphify made 0 socket attempts in every variant. Trail-Code made 49 blocked connect attempts when `DO_NOT_TRACK` is unset, and 0 with it set. |
| Language coverage, mixed corpus (10 languages) | **Symbols: dctx 10 of 10, Trail-Code 7, graphify 5.** **Call edges, run 3: dctx 10 of 10, Trail-Code 7, graphify 5** (run 1: dctx 0). dctx's new outline tier is symbols only, labelled `guess`. Terraform and SQL get symbols only from dctx. |
| Plain-text docs (.md/.rst/.txt/.adoc corpus) | dctx indexes the corpus into headed sections (0.18 s). graphify's code-only mode produced no index, and Trail-Code indexes no doc nodes. |
| Privacy canary (git-tracked `.env` files) | Run 3: **clean for all three tools** after sensitive-file read protection landed. In run 1, `pack` had stopped printing the canary, but explicit `dctx map read m:.env` still returned the file. Trail-Code and graphify leaked nothing in that run. |

## Run 7: 140 of 140 (main `53f8b6b`, 31 October, 06:20 UTC)

New since run 6:
- unresolved-call output: `neighbors --dir out` also lists the calls the index cannot resolve, as `calls:out:?<name>` records after the resolved callees. These are receiver methods, builtins and external package calls.
- dotted config keys: dotted config key matching (`dotted_key`), not yet wired into `find`.
- PowerShell edges: PowerShell symbols and command-position call edges.

All landed checks passed together on the Linux benchmark host before the run. Raw results are in `research/bench-3way-20261030/tasks/results/run5-53f8b6b/` and `research/bench-3way-20261030/results/run7-53f8b6b/`.

**Task-level protocol** (70 questions, 354 gold facts; generator, mapping and scorer unchanged):

| Tool | Points /140 | Gold facts /354 | Tokens | Facts per 1k tokens |
|---|---|---|---|---|
| dctx `53f8b6b` | **140** (run 6: 136) | **353** (342) | **29,340** | **12.0** |
| Trail-Code 0.28.0 | 122 | 312 | 88,923 | 3.51 |
| graphify 0.9.97, code-only | 96 | 238 | 197,203 | 1.21 |

dctx scores every point on all ten question types. Callees went from 10 to 14 of 14 (37 of 38 facts). It uses a third of Trail-Code's tokens and a seventh of graphify's.

**Three-way run 7**: callers P / R on the first page 0.995 / 0.851 (all pages 0.968), the same as run 6. The language coverage table is unchanged at 10 of 10 with symbols and 9 of 10 with call edges; shell is still `n`. Its per-language edge counts rise (for example Scala 1 to 8) because the probe counts every `calls` record, and the new unresolved-call output includes the `calls:out:?` names. A count above zero says nothing new about resolved edges, so this run does not claim more resolved edges.

## Run 6 (main `82ec474`, 31 October, 02:04 UTC)

New since run 5:
- declaration-definition pairing: a declaration-only symbol (`.mli`, C/C++ header, `.d.ts`) answers neighbors for its paired definition.
- Erlang support: Erlang (not in the mixed corpus).
- export-list extraction and outgoing-call extraction: `export_list` and `out_calls`, which are not yet wired into answers.

All landed checks plus these four passed together on the Linux benchmark host before the run. Raw results are in `research/bench-3way-20261030/results/run6-82ec474/` and `research/bench-3way-20261030/tasks/results/run4-82ec474/`.

- Call-edge coverage on the mixed corpus: **9 of 10** (run 5: 8). OCaml now has edges through the `.mli` pairing. Shell still has none, because the probe samples bats `@test` blocks that call only external commands. Trail-Code has 7 and graphify 5.
- Callers P / R, first page: 0.995 / 0.851, unchanged (all pages 0.968).
- Task protocol: **136 / 140**, unchanged. Trail-Code has 122 and graphify 96. The 4 lost points are still callees questions; the next unresolved-call output improvement wires `out_calls` into neighbors.

## Latest: run 5 and task-level re-run 3 (main `a906884`, 30 October, 16:39 and 17:05 UTC)

dctx `a906884` adds four improvements on top of run 4. Caller-diversity improvements spread callers over files, makes bare calls skip methods, and ranks data-file keys below code. Declaration ranking puts exported declarations first in outlines, gives read sites for config keys, adds line numbers to find, and ranks exact-word concepts first. Caller paging orders callers by call-site count and fits pages to the budget. Language outline improvements add deeper Scala, Zig, Lua, Elixir and OCaml symbols. All 36 landed checks passed together on the Linux benchmark host before the run. The binary's sha256 starts with `76d8e9b59821`. Raw results are in `research/bench-3way-20261030/results/run5-a906884/` and `research/bench-3way-20261030/tasks/results/run3-a906884/`.

**Task-level protocol** (70 questions, 354 gold facts; generator, mapping and scorer unchanged):

| Tool | Points /140 | Gold facts /354 | Tokens | Facts per 1k tokens |
|---|---|---|---|---|
| dctx `a906884` | **136** (re-run 2: 102) | **342** (243) | **28,472** | **12.0** |
| Trail-Code 0.28.0 | 122 | 312 | 88,923 | 3.51 |
| graphify 0.9.97, code-only | 96 | 238 | 197,203 | 1.21 |

**dctx now wins the task-level test.** It scores 14 points more than Trail-Code with a third of Trail-Code's tokens. By question type, out of 14:

| Type | dctx | Trail-Code | graphify |
|---|---|---|---|
| definition and callers | **14** | 10 | 10 |
| impact | **14** | 13 | 12 |
| outline | **14** | **14** | 12 |
| config key | **14** | **14** | 6 |
| exports | **14** | **14** | **14** |
| source | **14** | 12 | 0 |
| callees | **10** | 8 | 8 |
| overview | **14** | 13 | 11 |
| collision | **14** | 12 | 13 |
| concept | **14** | 12 | 10 |

dctx leads or ties on every type. The 4 points it still loses are all callees questions: ripgrep, pocketbase, django and core lose 1 each. In the three public corpora every missing fact is a call that the index could not resolve: a receiver method (`line_terminator`, `lookup_allowed`), a builtin (`callable`) or an external package (`Validate`). The core rows are sanitized. Unresolved-call reporting exposes these calls.

**Three-way run 5** (the same harness as runs 1-4):

| Callers P / R (first page) | Run 4 | Run 5 | Trail-Code | graphify |
|---|---|---|---|---|
| flask | 1.000 / 0.977 | 1.000 / 0.977 | 0.962 / 0.328 | 0.933 / 0.195 |
| ripgrep | 0.990 / 0.527 | 0.990 / 0.801 | 0.987 / 0.327 | 1.000 / 0.032 |
| pocketbase | 1.000 / 0.815 | 1.000 / 0.842 | 1.000 / 0.099 | 1.000 / 0.383 |
| trailcode | 0.991 / 0.738 | 0.992 / 0.852 | 0.989 / **0.902** | 1.000 / 0.215 |
| all | 0.995 / 0.725 | **0.995 / 0.851** | 0.986 / 0.436 | 0.989 / 0.196 |

Following up to 5 pages, dctx reaches P 0.996 / R 0.968 (run 4: 0.982; the site-count order moves some callers past page 5). Trail-Code still wins the trailcode corpus on page 1.

The rest of run 5, against the same Trail-Code and graphify runs:
- Cold index: dctx 0.13-1.09 s, Trail-Code 1.42-55.5 s, graphify 4.3-61.5 s.
- Peak memory: dctx 19-90 MB, Trail-Code 121-1,015 MB, graphify 74-393 MB.
- Warm search, callers and skeleton: dctx 43-195 ms, Trail-Code 0.97-2.0 s, graphify 0.41-2.1 s.
- No tool leaked the canary.
- Network: dctx and graphify made 0 attempts. Trail-Code made 1 blocked connect when `DO_NOT_TRACK` was unset.

Call-edge coverage on the mixed corpus is **8 of 10** by the probe; Trail-Code has 7 and graphify 5. The probe samples the first 25 symbols of the six largest files per language. It now follows every skeleton page, so its symbol counts are comparable with Trail-Code's full listing. Two languages miss for reasons outside the call resolver:
- OCaml: the first 25 symbols all come from `cmdliner.mli`, an interface file with no bodies. On `cmdliner_arg.ml` itself, `str_of_pp` lists 3 callers with their call lines. Declaration-definition pairing makes an interface declaration answer for its definition.
- Shell: the first 25 symbols are bats `@test` blocks in `test/bats.bats`, which call external commands only.

## Task-level questions: the October protocol, re-run on 30 October

This is the same protocol as October: 70 questions over seven corpora, 354 gold facts, and October's generator and scorer. It uses no model, and an answer scores when the tool's raw output contains the gold facts. Commands, question mappings and deviations are in `research/bench-3way-20261030/tasks/README.md`.

| Tool | Points /140 | Gold facts /354 | Tokens | Facts per 1k tokens |
|---|---|---|---|---|
| Trail-Code 0.28.0 | **122** (October: 122) | **312** (312) | 88,923 | 3.51 |
| graphify 0.9.97, code-only | 96 | 238 | 197,203 | 1.21 |
| dctx `de06a3d`, October mapping | 77 (October: 69) | 195 (169) | **25,929** (500,005) | **7.52** |

**Trail-Code still wins the task-level test.** dctx gained 8 points and needs a fifth of October's tokens, because its overview question now uses `map hubs` instead of the unbounded legacy tree. dctx's answers are the densest: 7.5 facts per 1,000 tokens against Trail-Code's 3.5. Where dctx still loses:
- Caller facts need the call line, and `neighbors` returns the calling function's span.
- Config-key and concept questions score 0 under the October mapping.
- A supplementary run that uses the verbs added since October reached 96 points (`map grep` for config keys, `map skeleton` for outlines). It is not the October protocol, so it is not counted above.

### Task-level re-run 2 (dctx `5e1b12b`, 30 October, 13:12 UTC)

dctx `5e1b12b` adds call-site lines on caller records (call-site lines), the config-key literal fallback (config-key fallback) and multi-word concept ranking (concept ranking). Protocol, mapping and scorer are unchanged. The headline is in `research/bench-3way-20261030/tasks/results/run2-5e1b12b/`.

| Tool | Points /140 | Gold facts /354 | Tokens | Facts per 1k tokens |
|---|---|---|---|---|
| Trail-Code 0.28.0 | **122** | **312** | 88,923 | 3.51 |
| dctx `5e1b12b` | **102** (run 1: 77) | 243 (run 1: 195) | **28,742** | **8.45** |
| graphify 0.9.97, code-only | 96 | 238 | 197,203 | 1.21 |

By question type, out of 14:

| Type | dctx | Trail-Code | graphify |
|---|---|---|---|
| definition and callers | **12** | 10 | 10 |
| impact | **14** | 13 | 12 |
| source | **14** | 12 | 0 |
| callees | **10** | 8 | 8 |
| overview | **14** | 13 | 11 |
| collision | 10 | 12 | **13** |
| outline | 11 | **14** | 12 |
| exports | 8 | **14** | **14** |
| config key | 7 | **14** | 6 |
| concept | 2 | **12** | 10 |

dctx now leads on five types and comes second overall, ahead of graphify. Trail-Code still wins outline, exports, config key and concept questions, and is 20 points ahead in total.

## Run 4: callers recall wave (main `de06a3d`, 30 October, 09:17 UTC)

The dctx binary's sha256 is `0a41481ffc6f9ecd5713be5af561bef9f1c13f980bfa69748d84540520b4086e`. New since run 3:
- compact neighbor records `42a3364`: compact neighbor records, so a page carries about 40 callers.
- macro-argument calls `2812e28`: Rust calls inside macro arguments.
- receiver calls `baebc3a`: receiver calls resolve to a lone method, and self-edges are listed in both directions.
- brace, indent and ML outline improvements `fff4f82`, `37db6a0`, `aa681ea`: deeper outline symbols.
- cache ceilings `de06a3d`: per-root cache ceilings of 768 MiB.

All 28 checks passed together before the run. Raw results are in `research/bench-3way-20261030/results/run4-de06a3d/`.

| Callers P / R (first page) | Run 3 | Run 4 | Trail-Code | graphify |
|---|---|---|---|---|
| flask | 1.000 / 0.836 | 1.000 / 0.977 | 0.962 / 0.328 | 0.933 / 0.195 |
| ripgrep | 0.981 / 0.246 | 0.990 / 0.527 | 0.987 / 0.327 | 1.000 / 0.032 |
| pocketbase | 1.000 / 0.667 | 1.000 / 0.815 | 1.000 / 0.099 | 1.000 / 0.383 |
| trailcode | 0.983 / 0.332 | 0.991 / 0.738 | **0.989 / 0.902** | 1.000 / 0.215 |
| all | 0.993 / 0.461 | **0.995 / 0.725** | 0.986 / 0.436 | 0.989 / 0.196 |

Following up to 5 pages, dctx reaches P 0.996 / R 0.982.

Call-edge coverage on the mixed corpus is **9 of 10** in this run. The brace outlines brace rewrite cost Zig its call edges on real code (the fixture still passes), which Zig call-edge repair is fixing. Trail-Code has 7 of 10 and graphify 5 of 10.

The coverage table's symbol counts come from bounded `map map` pages. They are therefore not comparable with Trail-Code's full graph counts.

## Run 3: all outline families with call edges, `.env` leak closed (main `9c578fa`, 30 October, 08:02 UTC)

The dctx binary's sha256 is `72c6a31f0ee807ddd8840c7d8b85c9d1652dcb1001b948fec9de8eabaf277d6d`. New since run 2:
- sensitive-file read protection `45a2c06`: map mode never reads env-shaped files.
- ML and do/end call edges `df6ab96`: Lua, Elixir, Julia, R, OCaml and Haskell calls.
- Scala call edges `9c578fa`: Scala spans and calls.
- parser cache invalidation `8ac04e4`: parser id bump, so caches from before call edges are rebuilt.

All 20 focused checks passed together on the Linux benchmark host before the run. Raw results are in `research/bench-3way-20261030/results/run3-9c578fa/`.

| | Run 1 | Run 2 | Run 3 |
|---|---|---|---|
| dctx: languages with ≥1 call edge (mixed corpus of 10) | 0 | 6 | **10** |
| Trail-Code | 7 | 7 | 7 |
| graphify | 5 | 5 | 5 |
| dctx: canary from a git-tracked `.env` in any output | yes (`map read`) | yes (`map read`) | **no** |

Callers precision and recall on the gold corpora are unchanged (0.993 / 0.461). Cold index stays at 0.13–1.31 s.

**Still behind:**
- **Symbol depth in the outline tier:** on the mixed corpus's 6 largest files per language, dctx finds 5–25 symbols per language. Trail-Code finds 83–2,559, because its tree-sitter grammars see every member.
- **Callers recall on ripgrep and trailcode:** 0.25 and 0.33 against Trail-Code's 0.33 and 0.90.

## Run 2: call edges for the outline tier (main `99da638`, 30 October, 07:24 UTC)

Same harness, corpora and tool versions. The dctx binary's sha256 is `81b35b3eb5112921c4f773b2682c247ec667bfc3487ce0670aa5e3ec5a372d14`. The new commits are brace-family call edges (`8859210`, brace family) and infrastructure call edges (`99da638`, infra family). Raw results are in `research/bench-3way-20261030/results/run2-99da638/`.

| Mixed corpus, ≥1 call edge | Run 1 | Run 2 |
|---|---|---|
| dctx | 0 of 10 | **6 of 10** (Dart, Zig, Solidity, Terraform, SQL, shell) |
| Trail-Code | 7 of 10 | 7 of 10 (no Terraform, SQL or shell) |
| graphify | 5 of 10 | 5 of 10 |

dctx is the only tool with call edges for Terraform and SQL. Scala still has none on real code, although the fixture passes; that is a follow-up. Lua, Elixir and OCaml call edges are still being implemented. Cold index (0.13–1.06 s), callers precision and recall, and the canary result are unchanged from run 1. The `map read m:.env` leak remains until sensitive-file read protection lands.

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
[`research/bench-3way-20261030/results/headline.md`](https://github.com/Eris-Margeta/dctx/blob/main/docs/research/bench-3way-20261030/results/headline.md).

## Method

- Host: a Linux benchmark host (Debian 13, Xeon E5-2680 v2), shared with other jobs. Load was 3.2 at the start, so absolute
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

These exclusions apply to the historical tool-output protocols below the whole-session summary above.

- Whole-session cost or task-level answer quality from these historical protocols. Historical fact scores describe raw tool outputs only; the [11 November study](https://github.com/Eris-Margeta/dctx/blob/main/docs/BENCHMARK-20261111.md) measures whole sessions with a frozen model.
- Anything about graphify's LLM semantic graph, or Trail-Code's learnings and LSP edges.
- SWE-bench scores or agent-run numbers from these historical tool-output protocols.
- Equal language depth. dctx's 25 new formats are outline tier: symbols, not call edges.

## Changes included in the October baseline

Call and import edges (`19e90a1`), byte-stable answers (`ce18608`), `.env` omission in pack (`6d4deec`),
outline symbols for 25 formats and plain-text sections (`9c61ac9`, `e0821e3`, `839bd45`, `a32dad3`),
neighbors depth 4 (`0de5ce1`), `hubs` (`6bc0cbc`), `grep` (`5691234`), `skeleton` (`2243c38`), `path` (`670e9e3`),
`export` (`ac8408c`) and a 1 GiB source ceiling (`5d26781`). Each feature's focused check, and
then all 15 checks together on `ac8408c`, passed on the Linux benchmark host before the benchmark ran.

## Next

1. sensitive-file read protection: map mode must never serve env-shaped files. This is the remaining privacy leak.
2. Call edges for the outline tier, to close the 0-of-10 call-edge coverage on the mixed corpus.
3. Recall on ripgrep and trailcode (0.25 and 0.33 against Trail-Code's 0.33 and 0.90): method calls through
   receivers and re-exports.
4. Re-run the 70-question task protocol with a frozen model.

## Independent benchmark, 31.10.2026, run uncapped on the Linux benchmark host

Seeded selection of 3 repos the tools never saw (zoxide, requests, gin), gold from language servers and repo history, published regardless of outcome. Tasks: dctx 22/28 points, 31/36 facts, 6.85 facts per 1k tokens (Trail-Code 19/28, graphify 14/28). Callers against LSP gold, first page: dctx P 0.730 / R 0.341, Trail-Code P 0.731 / R 0.313, graphify P 0.920 / R 0.264. Honest read: dctx leads on task answers and token efficiency; caller recall on unseen repos is far below the in-house corpus (0.851), so the caller floors above hold only for the curated corpora. Full table and raw JSON: `docs/research/bench-independent/results/20261031T191225Z/`.
