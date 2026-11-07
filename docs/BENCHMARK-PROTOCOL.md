<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Benchmark protocol: whole agent tasks and the template effect

Status: proposed owner-run experiment, 7 November 2026. This is a protocol and
an [offline harness](../tools/bench/README.md), not a new benchmark result.
No paid model calls are part of preparing it. Existing benchmark reports remain
historical records; this protocol does not rewrite them.

The question is whether dctx helps an agent finish an accurate task with less
total billed work than **grep plus file reads**. That is the only comparison.
A smaller tool reply is useful evidence, but it cannot establish a cheaper task.
We also test whether dctx's presentation helps a model use the same evidence.
That second test measures presentation, not retrieval or agent-session savings.

## Why measure the whole session?

A user team reported roughly 50,000 tokens of agent start-up context, a few
thousand tokens of tool results, and 18–23 calls instead of 2–7 when a large,
mostly-Markdown Windows tree encountered dctx's cache cap. Their reported total
increase was about 23%. Treat these as motivating observations to reproduce,
not a result of this protocol. In a session, system instructions, tool schemas,
conversation history and retained results can be billed again on every request.
They may receive a prompt-cache discount, but they do not disappear.

Other user observations point to a useful workload: reading one roughly 1.2 KB
function from a 434 KB file, or roughly 1.7 KB of find-and-read results instead of
a 30.7 KB file. Test these sizes as examples and include tasks that need the
surrounding file too. Grep and bounded file reads can also be economical. Do not
force the baseline to read whole files or force either arm to make extra calls.

## Freeze the design before running

Publish a manifest with the dctx commit and build features, repository revisions
or immutable corpus hashes, question-set hash, model IDs and revisions, agent
version, decoding settings, output and context limits, tool definitions, agent
instructions, OS, machine, storage, cache settings, run seed, stopping rules and
prices with their date. Record unavailable fields explicitly. Keep private source,
credentials and proprietary transcripts out of public artifacts; publish a
synthetic reproducer or permission-cleared selection when needed.

Choose the question set and gold facts before inspecting either arm's answers.
Use real questions covering lookup, explanation across files, negative findings,
and distractors, with explicit source paths/spans for each fact. A knowledgeable
reviewer checks the gold independently. Negative claims need a defined search
scope; absence from a returned page is insufficient. Do not tune the questions
or tool instructions after seeing treatment performance. Freeze a pilot set
separately and exclude it from the main comparison.

Preregister at least ten questions per scenario, two pinned models, and five
paired repetitions per question/model where practical; three repetitions is the
minimum exploratory design supported by the skeleton. If budget is smaller,
state the smaller sample before running and label the outcome exploratory.
Do not repeatedly rerun only losing or failed cases. Both arms share task,
model, settings, hardware, filesystem selection and resource/time budgets.
Use fresh conversations. Randomize paired arm order and interleave runs across
models and time so warm machines and provider load do not favor one arm.

## Experiment 1: grep/read versus dctx in an agent session

Baseline: the agent can search with ordinary grep and list/read source files,
including line ranges. Record the grep implementation and version; use GNU grep
or the platform's ordinary text-search equivalent, not a competing context tool.
Treatment: the same agent keeps those tools and gains dctx. Log every fallback
to grep or file reading and charge it to the treatment. Neither arm gets gold
paths, preselected evidence, a prepared answer, or a hidden helper search.

Keep common system instructions and tools identical. Give each arm neutral,
equally useful tool guidance. Include dctx's additional instructions and schemas
in its billed input; do not hide them behind an assumed equal start-up cost.
Shell integration and MCP integration are separate modalities: a shell comparison
can expose dctx through the existing shell, while MCP may add tool schemas.
Preserve the agent's real schema-discovery behavior and measure it. Equal tool
capabilities do not imply equal serialized context size.

Report each modality separately:

| Modality | What the agent does | What the measurement includes |
| --- | --- | --- |
| Short on-demand lookup | Grep/read or dctx map/find/read for one answer | Full start-up, exploration, final answer and failures |
| Long interactive session | Several related questions over big files in one conversation | Growing/reused history, retries, compactions and final answers |
| Pack for chat | Grep/read supplies evidence or dctx pack supplies a snapshot/diff | Preparation, all supplied evidence, repeated turns and edits |
| MCP versus shell | Same task through the declared integration | Actual schemas, discovery, process start and tool round trips |

Do not mix pack, map, MCP, or full/diff context into one unnamed “dctx” score.
The template experiment below covers static pack-style presentation only.

### Scenarios and scale controls

| Scenario | Required workload | Required checks |
| --- | --- | --- |
| Short question | Tasks normally answerable in 2–7 tool operations; small and medium repos | Allow batching, bounded reads and early completion; count the final model call |
| Long session over big files | Related questions over large source files, including a roughly 434 KB file and a roughly 30.7 KB file; a narrow function and broader dependencies | At least ten follow-ups in a continuous conversation, declared edits/diffs, rereads and history growth; score each checkpoint and whole session |
| Huge tree and cap behavior | A mostly-Markdown tree above 1 GB; a size sweep around the effective source limit and cache cap; a separate owner-scale 200 GB folder | Distinguish total disk bytes, selected source bytes, file count, index/cache bytes, exclusions and returned coverage; keep failures in results |
| Windows | Native Windows, not just Linux with Windows-looking paths; repeat short, long and cap cases | Record Windows build, filesystem, shell/grep, LF/CRLF, Unicode/spaces, long paths, permissions and case behavior; compare equivalent corpus bytes |

The default cache policy in this checkout declares a 2 GiB hard cap, 1.5 GiB
soft watermark and 1.2 GiB target in
[quota.rs](../crates/dctx-adapter-cache/src/domain/quota.rs). The current
[source-scan default](../crates/dctx/src/domain/source_ceiling.rs) is 1 GiB,
configurable through `DCTX_MAX_SOURCE_BYTES`; source limits and cache quotas
are different constraints. Record effective
settings from the tested build. A 200 GB folder does not imply 200 GB of selected
source or a 200 GB cache. Report all three figures, and do not silently narrow
the tree to make it fit. The offline builder's per-file memory guard is another,
unrelated limit; it does not exercise dctx's cap.

Run separate cold and warm strata. Cold means an empty, isolated dctx cache;
warm means the preregistered preparation sequence has run. Also record filesystem
cache state and provider prompt-cache state independently. Never erase a user's
real cache. Include cold index/setup time and cost in the cold total. Publish warm
preparation cost separately and show amortized totals at declared task counts.
Do not warm only the treatment's disk pages; counterbalance or control OS caches.

For cap testing, record cache size before/after each step, diagnostics, exit codes,
evictions, rebuilds, failed allocations and retries. Include a low-cap synthetic
stress reproducer where configurable, plus the unmodified default-cap run.
Keep source limits unchanged in the primary default run. A raised-source-limit
or raised-cache-cap run is a separate, clearly labeled sensitivity test. Verify
coverage with sentinel facts on both sides of the scanned tree and compare to
the approved file inventory. Record refusal, partial coverage or timeout as a
failure if the task cannot be answered. Do not assume which limit caused it.
The owner-scale run can be private, but must publish enough inventory, settings
and trace summaries to explain scope and limitations. Synthetic scale tests
complement real corpora; they do not prove real-world performance on their own.

### What to log and calculate

Instrument the agent/provider boundary, not just dctx stdout. Keep one record
per model request, including start-up instructions, tool descriptions, user text,
retained history, tool results, assistant/reasoning output usage, cache usage,
request ID, timestamps, model and price tier. Retain the serialized transcript
or its permitted audit artifact. Include charged failures, retries, hidden helper
model calls, summarization and compaction; record truncation and context resets.
Avoid double-counting provider totals and their reasoning-token subfields.
If the host cannot expose complete usage, label it incomplete and do not claim
a whole-session billed-token win from tool-byte estimates.

For every task, report these fields for both arms and the paired difference:

| Metric | Definition |
| --- | --- |
| Total billed tokens | Sum of all provider input tokens, including cached input, plus all billed output/reasoning tokens, across all requests |
| Input/output breakdown | Common start-up, arm-specific instructions/schemas, retained history, tool text and output; estimate category splits only if provider lacks them |
| Model calls | Number of provider requests, including the final answer, retries and compactions |
| Tool operations/round trips | Both logical operations and dispatched batches; one model request can request several tools |
| Prompt-cache hit share | Sum of cached input tokens divided by sum of total input tokens; unknown is unavailable, never zero-filled |
| Gold-fact accuracy | Correct required facts divided by required facts, plus all-facts-correct task rate and unsupported/contradicted claims |
| Wall time | Monotonic elapsed task time from start of preparation/request through accepted answer or stopping limit; split model, tool, indexing and setup time |
| Failures and scope | Timeouts, errors, exhausted budget, missing facts, truncation, excluded files and fallback operations |

Raw token counts and money answer different questions. For call `i`, let `I_i` be
all input tokens, `C_i` the cached subset, and `O_i` billed output. Let `d` be the
cached-input price discount (0 means no discount, 0.9 means cached input costs
10%). With uncached input price `p_in` and output price `p_out` per million:

```text
raw total billed tokens = sum(I_i + O_i)
effective input at the uncached rate = sum(I_i - d * C_i)
cost = (p_in * sum(I_i - d * C_i) + p_out * sum(O_i)) / 1,000,000
cache hit share = sum(C_i) / sum(I_i)
```

Add cache-write/storage charges or other provider-specific fees separately;
the simple harness does not price them. For projections with constant context,
input alone is `model_calls * context_per_request`. With a 50,000-token start-up,
7 requests replay 350,000 start-up tokens and 23 replay 1,150,000, before tool
history or output. This arithmetic is not a measured saving or loss. Cache
eligibility, exact prefix matches, minimum sizes, expiry and routing must be
measured; stable bytes alone do not establish cache hits. Evaluate at least no
discount, the measured policy and a declared favorable-cache sensitivity case.
Never assume all repeated history is cached. Do not equate model calls with
the user team's reported calls until their definition is confirmed.

## Experiment 2: does the template help with identical evidence?

Use at least two actual pinned models and several independent runs per pair.
Each model sees both renderings in **separate fresh conversations**, one prompt
per trial, with no tools and no previous answer. Balance and randomize arm order.
Keep identical model settings, question, task instructions, selected file paths
and source bytes. Gold facts must all be answerable from that selection, with
matched distractors. This controlled test can use gold-informed evidence chosen
before running; the agent experiment must discover evidence itself.

Arm A uses the bundled dctx v1 template's instructions, directory manifest, IDs,
file blocks and deterministic file order. Arm B uses plain path headings and the
same source content in the same order. Both include file contents exactly once.
The [builder](../tools/bench/README.md) reads the actual bundled template and
records its hash. Its constrained renderer omits the CLI version envelope and
uses a constant project name. It is explicitly not a CLI capture or a general
DTS implementation. Validate renderings before adopting another template.

The offline primary pair matches total UTF-8 byte length with trailing spaces
on the shorter arm. Record padding size and unpadded counts. Bytes/4 is only a
deterministic token estimate; equal bytes need not mean equal model tokens.
Before a token-matched trial, use that model's local tokenizer through the
[adapter boundary](../tools/bench/adapter.py), match length separately per model,
and save the final prompt hashes and exact counts. Do not truncate evidence to
make the lengths match. If exact matching cannot be established, publish the
byte-matched stratum with its token imbalance instead of calling it token-matched.
Verify what the provider actually receives; if it strips trailing spaces,
padding has not established a match. Publish an unpadded sensitivity comparison
to expose padding effects. Keep these strata separate.

The primary pair holds source order fixed and estimates the combined effect of
IDs, manifest, delimiters and format instructions. It cannot attribute a change
to any one of those features. A separately preregistered order ablation can shuffle
the same evidence in the plain arm with a saved permutation; cross it with both
renderings to distinguish order from formatting. The current skeleton does not
implement that secondary factorial experiment. No presentation-only result
establishes lower retrieval cost, cache savings or better whole-session accuracy.

### Grade and report without favoring an arm

Strip treatment/model labels from answers before grading. Two reviewers score
each fact correct/incorrect against the frozen rubric and its source evidence;
resolve disagreements with a recorded decision. Equivalent wording counts.
Record unsupported claims separately and count a task as accepted only when
all required facts are correct and no material unsupported claim remains.
An empty answer or failure has zero accuracy. For long sessions, grade every
checkpoint; preserve full-session totals even if some checkpoints fail.
Gold text and grader explanations must never enter the answering model's prompt.
An automated grader, if added later, needs independent human calibration, its
own disclosed model/cost and disagreement analysis; it is not implemented here.

Publish per-task/per-run rows, answer scores, usage, wall time, arm settings and
failure reasons, plus paired aggregates for each scenario, model, OS, modality
and cache stratum. Show medians, ranges and paired differences; report confidence
intervals by resampling questions (sessions for the long-session stratum), not
by pretending repeated runs are independent tasks. Report accuracy changes
alongside cost/time changes. Preregister a maximum acceptable accuracy loss
(default zero observed loss, with uncertainty reported); claim a win only where
the accuracy guard holds. Include costs of failed tasks and a cost-per-accepted-task
view. If no task succeeds, that ratio is undefined, not zero.

Release the frozen manifests, harness revision, grading rubric, redacted answers,
per-call usage, cache logs, preparation times, analysis commands and exclusions
needed to reproduce every reported number. Record all deviations from this
protocol. Keep the whole-session comparison and template effect in separate
tables, and name the scenarios where dctx loses, wins or remains inconclusive.
The intended result is evidence about when to use each dctx modality.
