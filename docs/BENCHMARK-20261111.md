<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Whole-session benchmark: grep plus file reads versus dctx

Measured on 11.11.2026. **NOT STABLE**: dctx remains an advanced tool.
On this corpus and model dctx did not save 25 % tokens. The pre-registered
success rule was **not met**: CLI median total tokens were -5.01 % versus grep.
The claim supported here is **accuracy and bounded context**, not token savings.

## Method

- Corpus: a private 13-crate Rust workspace; it is not public.
- Questions: 20, covering code location, call sites and multi-hop traces.
- Model: `claude-sonnet-5-5` via Claude CLI.
- dctx: 1.3.0 (`156e1bb`), not current HEAD.
- Three repeats per arm. Only tool exposure differs; prompt, model and question
  order are identical. Blind grading uses opaque ids without arm labels.

| Arm | Tools and cache conditions |
|---|---|
| A | Read + Grep/Glob: grep plus file reads baseline |
| B | Read + Bash restricted to `dctx *`; index warm before measurement |
| C | Read + dctx MCP only; fresh cache, with start-up and indexing measured |

These are whole-session measurements, including conversation replay and provider
cache usage, rather than estimates of tool stdout. The warm index in B excludes
its preparation cost; C includes start-up and indexing. This difference limits
direct comparisons between the two dctx modalities.

## Cost and turns

Totals per arm cover all 20 questions; each cell is the median [min, max] of three runs.
Total tokens = uncached input (including cache creation) + cached input + output.
Each metric is summarized independently, so component medians need not sum to
the median total. Turns are CLI `num_turns`, an assistant-turn proxy, not exact
tool calls.

| Metric | A: grep plus reads | B: dctx CLI | C: dctx MCP |
|---|---|---|---|
| Total tokens | 14,904,014 [14,604,149, 15,622,381] | 14,157,974 [11,221,908, 15,613,375] | 15,922,125 [15,513,198, 16,700,029] |
| Uncached input tokens | 286,289 [284,340, 290,043] | 250,479 [220,537, 258,313] | 256,015 [247,681, 256,302] |
| Cached input tokens | 14,543,028 [14,241,089, 15,246,829] | 13,833,705 [10,939,641, 15,278,757] | 15,579,598 [15,190,063, 16,365,955] |
| Output tokens | 78,720 [74,697, 85,509] | 73,790 [61,730, 76,305] | 77,772 [75,454, 86,512] |
| Wall time (s) | 700.87 [697.78, 811.59] | 719.84 [628.75, 768.06] | 910.99 [899.31, 974.98] |
| Turns (`num_turns`) | 172 [159, 175] | 130 [129, 137] | 239 [221, 240] |

B versus A median total tokens: **-5.01 %**. C versus A: **+6.8 % tokens**
and **+30 % wall time**.

## Accuracy: both metrics

The official metric is strict citation recall: exact repo-relative path and line,
with a tolerance of +/-3 lines, against 574 required line references per run.
The diagnostic metric is file-level recall: the required file named in full
anywhere in the answer, against 106 file/question pairs per run.

| Metric | A: grep plus reads | B: dctx CLI | C: dctx MCP |
|---|---|---|---|
| Official strict citation recall, median [min, max] | 10.10 % [10.10, 20.73] | 14.46 % [8.36, 22.65] | 13.41 % [12.02, 66.72] |
| Diagnostic file-level recall, median | 53.77 % | 71.70 % | 76.42 % |

A grader audit found no grader defect. Low strict recall comes from answers
citing shorthand paths such as `store.rs:313`, and from line-level granularity:
10 trace questions hold 82 % of required lines. File-level recall is a
diagnostic, not a replacement metric. No file-level spread is supplied.

## Where savings would come from

dctx on its own does not save tokens by default. Token savings come from
systems that use dctx: a harness or agent system built around its cache-hit
philosophy, reusing byte-identical, prefix-stable context across long sessions
over a large body of work. In that setting it behaves like any sufficiently
advanced caching mechanism, with one difference: you have complete control
over every byte of the context. The system must arrange that reuse; merely
exposing dctx tools does not do it. There is no measured, citable savings figure
for such a system yet. See [the philosophy](WHY-DCTX.md#savings-belong-to-the-system).

## Verdict and limitations

The pre-registered rule required B to save at least 25 % median total tokens
**and** match or exceed A's recall. **NOT MET on tokens (-5.01 %)**, even though
B's median official recall exceeded A's. On this corpus and model dctx did not
save 25 % tokens.

B and C named more of the required files than grep at about equal (B) or higher
(C) token cost. This supports an accuracy and bounded context claim, not a
whole-session token-savings claim. The bounded context behavior is described in
[Choosing how to use dctx](MODALITIES.md); these results do not establish a
general accuracy advantage.

Limitations: one corpus, one model, three repeats, and no significance claim.
The corpus and raw answers are private and cannot be published; these aggregate
results cannot be independently reproduced from the public repository alone.
dctx 1.3.0 is not current HEAD. The guidance **Under ~200 files, use grep** stays.

The [earlier measurement evidence](BENCHMARK-20261030.md) preserves the historical
tool-output protocols separately from this whole-session study.
