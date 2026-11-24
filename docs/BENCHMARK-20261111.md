<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Whole-session benchmark: grep plus file reads versus dctx

## Round two verdict

On 12 November 2026, the dctx harness-prefix pack (P) reached 70.91% median strict citation recall versus 19.34% for grep plus file reads across 20 questions and three repeats per arm (300 graded answers across five arms). Median list-price token cost per question was $0.195418 versus $0.179670 for grep (+8.8%). The pack missed the prespecified target of at least 15% lower cost per question.

### Method

- One pinned private Rust workspace, dctx 1.3.1 (`a6259da`), and `claude-sonnet-5-5` via Claude Code 2.1.319.
- Five arms ran the same 10 trace and 10 impact questions three times each. All 15 runs completed and all 300 answers were graded.
- Strict citation recall counts exact repo-relative path and line references, with a tolerance of ±3 lines. Each run pools matched and required references across its answers.
- Both pack arms supplied verified repo-relative paths and original line numbers alongside source excerpts. P retained grep plus file reads, and P2 also exposed the dctx CLI.
- Whole-session token counts include uncached input, cache creation and reads, and output. The cost column prices each token category separately at the 12 November list rates.

| Arm | Median strict recall [min, max] | Median total tokens per 20 questions | Median list-price USD per question |
|---|---:|---:|---:|
| A: grep plus file reads | 19.34% [13.07%, 23.17%] | 15,427,072 | $0.179670 |
| B: dctx via Bash (warm index) | 13.41% [8.01%, 25.61%] | 14,950,317 | $0.176401 |
| C: dctx via MCP (historical) | 13.07% [8.19%, 18.64%] | 14,287,157 | $0.158096 |
| P: dctx harness-prefix pack | 70.91% [63.76%, 73.87%] | 12,378,679 | $0.195418 |
| P2: dctx harness-prefix pack variant | 72.13% [62.02%, 75.78%] | 13,046,490 | $0.207701 |

P had to match or exceed A's median recall and cost at least 15% less per question. It met the recall requirement and missed the cost requirement. P2 was descriptive and cost about 15% more than A. The +8.8% for P refers to priced token usage. Cache reads and uncached input carry different prices, so the aggregate token count alone cannot determine cost.

The pack results apply to this harness, corpus and model. The Bash and MCP arms had lower median recall than grep in this round. Three repeats per arm limit generalization, and the private corpus and answers prevent independent reproduction from the public repository.

As a heuristic, start with grep and narrow file reads on repos under about 200 files. A shared dctx index may pay off on larger repos or multi-hop tasks when agents reuse it. The study used one fixed corpus and did not measure a repository-size crossover or the payoff from sharing an index across agents.

## Earlier CLI pilot

On 11 November 2026, the two-arm CLI pilot used 20 questions and three repeats per arm (120 answers). The warm dctx CLI arm reached 14.46% median strict citation recall versus 10.10% for grep. Median total tokens were 14,157,974 versus 14,904,014 (−5.01%). The pilot missed its separate prespecified threshold of a 25% reduction in total tokens with recall at least as high as grep.

### Method

- Corpus: a private 13-crate Rust workspace.
- Questions: 20, covering code location, call sites and multi-hop traces.
- Model: `claude-sonnet-5-5` via Claude CLI.
- dctx: 1.3.0 (`156e1bb`), not current HEAD.
- Three repeats per arm. Only tool exposure differs; prompt, model and question
  order are identical. Blind grading uses opaque ids without arm labels.

| Arm | Tools and cache conditions |
|---|---|
| A | Read + Grep/Glob: grep plus file reads baseline |
| B | Read + Bash restricted to `dctx *`; index warm before measurement |

These are whole-session measurements, including conversation replay and provider
cache usage, rather than estimates of tool stdout. The warm index in B excludes
its preparation cost; these results do not measure cold indexing.

### Cost and turns

Totals per arm cover all 20 questions; each cell is the median [min, max] of three runs.
Total tokens = uncached input (including cache creation) + cached input + output.
Each metric is summarized independently, so component medians need not sum to
the median total. Turns are CLI `num_turns`, an assistant-turn proxy, not exact
tool calls.

| Metric | A: grep plus reads | B: dctx CLI |
|---|---|---|
| Total tokens | 14,904,014 [14,604,149, 15,622,381] | 14,157,974 [11,221,908, 15,613,375] |
| Uncached input tokens | 286,289 [284,340, 290,043] | 250,479 [220,537, 258,313] |
| Cached input tokens | 14,543,028 [14,241,089, 15,246,829] | 13,833,705 [10,939,641, 15,278,757] |
| Output tokens | 78,720 [74,697, 85,509] | 73,790 [61,730, 76,305] |
| Wall time (s) | 700.87 [697.78, 811.59] | 719.84 [628.75, 768.06] |
| Turns (`num_turns`) | 172 [159, 175] | 130 [129, 137] |

### Accuracy: both metrics

The official metric is strict citation recall: exact repo-relative path and line,
with a tolerance of +/-3 lines, against 574 required line references per run.
The diagnostic metric is file-level recall: the required file named in full
anywhere in the answer, against 106 file/question pairs per run.

| Metric | A: grep plus reads | B: dctx CLI |
|---|---|---|
| Official strict citation recall, median [min, max] | 10.10 % [10.10, 20.73] | 14.46 % [8.36, 22.65] |
| Diagnostic file-level recall, median | 53.77 % | 71.70 % |

A grader audit found no defect. Low strict recall comes from answers
citing shorthand paths such as `store.rs:313`, and from line-level granularity:
10 trace questions hold 82 % of required lines. File-level recall is a
diagnostic, not a replacement metric. No file-level spread is supplied.

The [October tool-output measurements](BENCHMARK-20261030.md) used a separate protocol.
