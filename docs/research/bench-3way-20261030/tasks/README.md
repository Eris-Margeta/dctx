<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Task-level rerun (70 agent questions): dctx vs Trail-Code 0.28.0 vs graphify 0.9.97

Rerun of section 8 of `docs/research/trail-code-2026-10/REPORT.md` with the October protocol. October result:
Trail-Code 0.23.1 122/140 points, dctx 0.25.0 69/140. Date of this run: 2026-10-30, host build-host
(`/path/to/workdir/bench/dctx3`). No LLM and no API key for any tool. Trail-Code may still win, and it does.

## Result

dctx = `dctx-de06a3d` (`dctx-cli v0.25.0`), October mapping, with `dctx map hubs` for the overview question.
Score = points out of 2 per question (2 if at least 80% of the gold facts are present, 1 if any, 0 otherwise).
Tokens = ceil(stdout bytes / 3.5) summed over every command of the question, as in October.

| Corpus | dctx: calls / tokens / score / facts | Trail-Code 0.28.0 | graphify 0.9.97 (code-only) |
|---|---|---|---|
| flask | 15 / 3,873 / 13 of 20 / 34 of 48 | 10 / 5,511 / 16 / 39 of 48 | 13 / 10,447 / 14 / 34 of 48 |
| ripgrep | 15 / 2,959 / 10 / 18 of 32 | 10 / 25,974 / 13 / 23 of 32 | 13 / 18,709 / 12 / 22 of 32 |
| pocketbase | 15 / 3,380 / 12 / 28 of 36 | 10 / 4,026 / 18 / 33 of 36 | 13 / 18,240 / 13 / 21 of 36 |
| trailcode | 15 / 3,495 / 12 / 41 of 64 | 10 / 7,779 / 19 / 59 of 64 | 13 / 17,789 / 15 / 49 of 64 |
| dctx | 15 / 4,290 / 12 / 26 of 48 | 10 / 15,396 / 19 / 44 of 48 | 13 / 29,479 / 14 / 33 of 48 |
| django | 15 / 4,007 / 10 / 35 of 70 | 10 / 7,006 / 18 / 63 of 70 | 13 / 50,123 / 12 / 34 of 70 |
| core (private) | 15 / 3,925 / 8 / 13 of 56 | 10 / 23,231 / 19 / 51 of 56 | 13 / 52,416 / 16 / 45 of 56 |
| **All** | 105 / 25,929 / **77 of 140** / 195 of 354 | 70 / 88,923 / **122 of 140** / 312 of 354 | 91 / 197,203 / **96 of 140** / 238 of 354 |
| All except overview | 98 / 21,709 / 63 of 126 / 163 of 322 | 63 / 57,221 / 109 of 126 / 284 of 322 | 84 / 50,648 / 85 of 126 / 218 of 322 |
| Facts per 1k tokens (all / without overview) | 7.52 / 7.51 | 3.51 / 4.96 | 1.21 / 4.30 |

Supplementary row, NOT October protocol (`dctx-ext`: same dctx binary, but with the verbs added since October:
`map skeleton` for outline and exports, `map grep` for the config key, `--json` on neighbors):
96/140, 273/354 facts, 39,315 tokens (columns per corpus in `results/tasks_headline.md`).

By question type (points out of 14; seven corpora):

| Type | dctx | Trail-Code | graphify | dctx-ext (suppl.) | October dctx / Trail |
|---|---|---|---|---|---|
| definition and callers | 7 | 10 | 10 | 7 | 7 / 10 |
| impact | 7 | 13 | 12 | 7 | 7 / 13 |
| outline | 11 | 14 | 12 | 14 | 11 / 14 |
| config key | 0 | 14 | 6 | 12 | 0 / 14 |
| exports | 8 | 14 | 14 | 12 | 8 / 14 |
| source | 14 | 12 | 0 | 14 | 14 / 12 |
| callees | 10 | 8 | 8 | 9 | 2 / 8 |
| overview | 14 | 13 | 11 | 14 | 14 / 13 |
| collision | 6 | 12 | 13 | 7 | 6 / 12 |
| concept search | 0 | 12 | 10 | 0 | 0 / 12 |

Reading:

- Trail-Code reproduces October exactly: 122/140 and 312/354 facts again; tokens 88,923 versus 88,691 (0.3% more;
  0.28.0 prints a `[trail] tokens saved ...` line on stdout in every call, roughly 70 tokens each, counted here).
  Per corpus differences from October are within a few hundred tokens (trailcode 7,779 versus 7,529 on a corpus
  of the same tree).
- dctx rises from 69 to 77 points under the October mapping. All of the gain is callees (2 to 10, `neighbors
  --edge calls --dir out` now returns edges) and the facts that come with it; the other types are unchanged.
  Token use falls from 500,005 to 25,929 only because the overview now goes through `map hubs` (4,220 tokens over
  seven corpora versus about 480k for the legacy tree). Without the overview question dctx used 19.2k tokens
  in October and 21.7k now.
- dctx still scores 0 on config-key and concept questions under the October mapping (`map find` of a key or a
  multi-word phrase returns nothing). With `map grep` (dctx-ext) the config question reaches 12/14.
- Caller facts need the caller's path and a call-site line number. dctx neighbors lists the calling function
  (text: id, kind, path; JSON: the function's line span), not the call line, so most caller facts stay missing for
  dctx whatever its recall. That is the same scorer as in October and it caps dctx on definition/callers/impact/collision.
- graphify (no LLM, AST only) is second on points, 96/140, but the most expensive: 197k tokens, of which 146.6k
  are its `GRAPH_REPORT.md` as overview (it lists community numbers, not directories, on large repos; django 44.8k
  tokens for 2 of 5 facts). It returns no source text (0 of 14 on source), has no text search (config 6/14 by luck of
  `query` keyword hits), and is strong on collision (13/14) because its node ids resolve to the exact definition.
- Trail wins 122 versus 96 versus 77 on points and on facts found. dctx wins on tokens: 7.52 facts per 1k tokens
  versus 3.51 (Trail) and 1.21 (graphify); without the overview question 7.51 versus 4.96 and 4.30.

## Protocol (identical to October unless listed under deviations)

- Questions: ten per corpus built by the October generator (`oct-scripts/phase_tasks.py` `build_questions`, which
  uses the independent oracle `goldlib.py` and the October target files `../../trail-code-2026-10/data/targets/*.targets.json`).
  354 gold facts in total, 56 for core, matching the October counts. For flask, ripgrep, pocketbase, dctx and django
  the generated questions are byte-identical to the published `*.questions.json`. For trailcode they differ only in
  the published file's name scrubbing (`agentx_PROJECT_DIR` there, `CLAUDE_PROJECT_DIR` in the real tree); the
  real names were used. Core was regenerated from the private repository and verified by its per-question fact counts
  (2, 2, 25, 2, 8, 3, 8, 3, 2, 1).
- Scoring: `scripts/tasks_score.py` carries October's `search_text`, `fact_present` and the 2/1/0 rule verbatim.
  Caller facts must come from a non-"find" answer.
- Isolation (bench3 conventions): `nice -n 10 taskset -c 24-31 unshare -Urn env -i`, scratch `HOME`, caches under the
  bench root, `DO_NOT_TRACK=1`, `GRAPHIFY_BACKEND=none`; one process at a time; every run on a fresh copy of the
  corpus; Trail `trail build .` and graphify `extract . --code-only --max-workers 8` + `cluster-only . --no-viz
  --no-label` as untimed setup (django: Trail 99 s, graphify 260 s; all builds rc 0).
- Corpora (`corpora-oct/`, total scratch 1.2 GB): flask 3.1.3 `22d92470`, ripgrep 15.2.0 `e89fff89`, pocketbase
  v0.40.5 `6f370ea7` (same checkouts as the 3-way `corpora/`), django 6.1.2 `2212534b`, trailcode
  `b1211efd3eef0efa121b93800e5dd8e547aead77` (0.23.1 tree, October's; the 3-way `corpora/trailcode` is the 0.28.0
  commit `25ea4c74`, so it was not reused), dctx `17dff0b454c24f43848abad1c88ea982e02731f7` (git archive,
  re-committed, 1,749 tracked files), core `f5f2f48b` (git archive of the private repository, 3,190 tracked files,
  no `target/` or `corpus/` in that commit).

## Question-to-command mappings

| Q | Type | dctx (October mapping) | Trail-Code | graphify (closest LLM-free command) |
|---|---|---|---|---|
| 1 | definition and callers | `map find NAME --json --limit 10`, then `map neighbors ID --dir in --edge calls` | `callers NAME` | `explain ID` (definition site), `affected ID --relation calls --depth 1` |
| 2 | impact | find, then `neighbors ID --dir in --edge calls --depth 2` | `callers NAME -d 2` | `explain ID`, `affected ID --relation calls --depth 2` |
| 3 | outline | `map map FILE --depth 2 --limit 40` | `skeleton FILE` | `explain FILE_NODE --all` |
| 4 | config key | `map find KEY --limit 10` | `grep KEY` | `query KEY --budget 2000` (no literal-search verb) |
| 5 | exports | `map map FILE --depth 2 --limit 40` | `skeleton FILE` | `explain FILE_NODE --all` |
| 6 | source | find, then `map read ID --json` | `ask NAME --source --full -n 3` | `explain ID` (graphify has no source-reading verb) |
| 7 | callees | find, then `neighbors ID --dir out --edge calls` | `callers NAME --direction out` | `explain ID --all` (outgoing `calls` edges are `-->` lines) |
| 8 | overview | **`map hubs`** (October: `dctx . -t`, unbounded legacy tree, plus its output file) | `map` | `cat graphify-out/GRAPH_REPORT.md` |
| 9 | collision | find, then `neighbors ID --dir in --edge calls` | `callers Qualified.name` | `explain ID`, `affected ID --relation calls --depth 1` |
| 10 | concept | `map find "WORDS" --limit 10` | `ask "WORDS"` | `query "WORDS" --budget 2000` |

dctx ID: taken from the `find` JSON (record whose path and line span contain the gold definition; else the first
record), as in October. graphify ID: resolved from `graphify-out/graph.json` by label, file and line of the gold
definition within 5 lines (as in `bench3.py`); if no node matches the bare name is passed instead. The `explain`
step of questions 1, 2 and 9 is treated as the id-resolution ("find") step, so caller facts must come from `affected`.
File nodes for questions 3 and 5 are found by `source_file` and basename label.

## Deviations from October

1. Overview question for dctx uses `dctx map hubs` (bounded, default 20 hubs plus per-directory counts) instead of
   the unbounded legacy tree. This is the only change to the dctx mapping. It changes dctx's token total from 500k
   to 26k; the points for that question are unchanged (14/14).
2. Tool versions: dctx binary `dctx-de06a3d` (still `dctx-cli v0.25.0`), Trail-Code 0.28.0 (October 0.23.1),
   graphify 0.9.97 added. Host and corpora pins as above; the October host directory no longer exists, so corpora
   were rebuilt (checks above). Trail 0.28.0 adds the stdout `tokens saved` line (counted).
3. Trailcode questions use the real identifiers rather than the scrubbed published ones (see Protocol).
4. graphify mapping is new (no October counterpart): choices above are mine and `explain ... --all` was chosen for
   outline and exports because the default lists only the 20 most connected neighbours. Id resolution from
   `graph.json` by gold location favours graphify slightly (dctx and graphify get the gold location, Trail resolves
   by bare name, as in October).
5. Supplementary `dctx-ext` row is outside the October protocol and is not the headline for dctx.
6. Each question ran once (as in October, no repetitions); timings were not measured here.
7. Core is a private repository. Its questions, outputs and missing-fact lists are not in this directory
   (`scripts/tasks_score.py --sanitize` drops them); only per-question counts and tokens remain.

## Commands

```
# on build-host, in /path/to/workdir/bench/dctx3 (this directory's scripts/ go to $B3_ROOT/scripts, run-tasks.sh to $B3_ROOT,
# oct-scripts/ to $B3_ROOT/tasks/oct/scripts, October targets to $B3_ROOT/tasks/oct/targets)
python3 scripts/setup_oct_corpora.py                 # DCTX_REPO=... CORE_REPO=... for the two local exports
python3 tasks/gen_questions.py $PWD                  # rebuild and verify the questions (core goes to tasks/private/)
./run-tasks.sh bin/dctx-de06a3d                      # all corpora, tools dctx,dctx-ext,trail,graphify
./run-tasks.sh bin/dctx-de06a3d --corpora flask --tools dctx,trail    # subset
```

Output: `results/tasks-<UTC>/{tasks.jsonl, task_runs.jsonl, tasks_setup.jsonl, tasks_aggregate.json,
tasks_headline.md, tasks_rows_sanitized.jsonl, outputs/}`. The run used here is `tasks-20261030T095005Z`
(about 19 minutes wall for 28 tool-corpus runs).

## Files

- `run-tasks.sh`, `scripts/run_tasks.py` (driver and per-tool mappings), `scripts/tasks_score.py` (October scorer and aggregation),
  `scripts/setup_oct_corpora.py`, `scripts/gen_questions.py`, `oct-scripts/` (October `goldlib.py`, `phase_tasks.py`, `select_targets.py`, `benchlib.py`).
- `results/`: `tasks_rows_sanitized.jsonl` (one row per question and tool), `tasks_aggregate.json`, `tasks_headline.md`,
  `task_runs_nocore.jsonl` (every command, without core), setup rows, versions, log.
