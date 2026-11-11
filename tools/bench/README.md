<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Offline benchmark skeleton

Python 3.11+, standard library only. The build/cost commands do not invoke a
model, subprocess, network, dctx, provider account or API key. No tokenizer
package is required. CLI tests launch local child processes. See the public
[benchmark protocol](../../docs/BENCHMARK-PROTOCOL.md) for the experiment design.
These examples are synthetic fixtures, not measured performance claims.

The separate [synthetic CI guard](PERF-CI.md) measures the actual dctx and catalog
binaries against a committed performance baseline, with an intentional refresh
command. It does not invoke models or estimate whole-session savings.

For informational Windows per-call latency (also runnable on Unix), build the
release CLI and run:

```sh
cargo build --release --locked -p dctx-cli
python3 tools/bench/map_find_latency.py --binary target/release/dctx-cli.exe \
  --output target/map-find-latency.json
```

On Unix, omit `.exe`. The script creates a fixed two-function Rust fixture and
an isolated cache beside the report, warms the index once, then measures 50
separate `dctx map find latency_probe` processes, including startup and index
freshness checks. It prints p50/p95 milliseconds using nearest-rank percentiles
(samples 25 and 48 of 50) and saves every sample as JSON. The Windows CI test
job uploads that report per toolchain; measurement and upload are non-failing.

From the repo root (on Windows, use `python` if that is your interpreter name):

```sh
python3 -m unittest discover -s tools/bench/tests -v
python3 tools/bench/bench.py build --repo tools/bench/examples/corpus \
  --questions tools/bench/examples/questions.json --out target/bench-demo \
  --models model-a-pinned model-b-pinned --runs 5 --seed 7
python3 tools/bench/bench.py cost --spec tools/bench/examples/projection.json \
  --cached-discount 0.9 --input-price-per-million 1 --output-price-per-million 4
python3 tools/bench/bench.py cost --spec tools/bench/examples/usage.json
```

Prices above are arbitrary arithmetic examples, not provider prices. A 0.9
cached discount means a cached input token costs 10% of an uncached one. Omit both
prices for counts without a currency cost. Reusing an output directory is an
error; choose a new destination for each frozen experiment.

## Question set and outputs

Use the versioned JSON shape in [questions.json](examples/questions.json).
Scenarios are `short`, `long`, `huge`, and `windows`. Each question names explicit
UTF-8 files, optional inclusive `start_line` and `end_line`, and a withheld list
of gold facts with unique IDs and plain-language `fact` text. IDs are portable,
case-insensitively unique filename components. Paths always use root-relative
forward slashes, including on Windows. Duplicate file selections, invalid spans,
non-UTF-8 input, NULs, paths outside the root and oversize files fail loudly.
An empty file is allowed as distractor evidence; its range is 1–0. Multiple spans
of one file must be combined into a single selection before building.

Only named files are read; the builder does not walk the tree or apply ignore
rules. Supply the same approved selection in both experiments. The default
16 MiB **per source file** guard is adjustable with `--max-source-bytes`; exceeding
it never silently drops evidence. This is a harness memory guard, unrelated to
dctx's source limit or disk cache cap. Avoid naming entire huge trees: select
bounded evidence for template trials. Use real agent sessions for cap testing.

The builder writes `QUESTION.dctx.txt`, `QUESTION.plain.txt`, `pairs.json`, and
`gold.json`. Keep gold separate from model requests and blinded graders' treatment
labels. The pair manifest records source and selected-byte hashes, line ranges,
template and question-set hashes, byte counts, estimated token counts, padding,
and the randomized, balanced-within-one trial order. Record the repo revision,
OS and model settings alongside it before running. Whitespace padding occurs
only at the end; no source text is trimmed or truncated. CRLF and Unicode source
bytes survive in both arms. The shared question and answer instructions match.

The template arm uses the repository's bundled
[default v1 template](../../crates/dctx-core/templates/default.dctx_template),
including its format instructions, manifest, file IDs and preorder of sorted
paths. The renderer supports this template's section/variable subset; it is not
a general DTS compiler. It omits the CLI's changing snapshot/version envelope
and uses the constant project name `benchmark`. The plain arm retains the same
paths, selected text and order, with simple path headings and no ID manifest.
The selected file contents occur once per arm. An owner using another template
must validate the supported subset and record its hash via `--template`.
These are controlled rendering prompts, not captured dctx tool output.

## Token and cost limits

The default token counter is **ceil(UTF-8 bytes / 4)**. `--bytes-per-token` changes
the denominator. It is a deterministic estimate, not a real model tokenizer, a
bound, or provider usage. Equal UTF-8 byte lengths guarantee equal values under
this estimator; they do **not** guarantee equal model token counts. The manifest
states this limitation. Before a token-matched paid trial, use a local tokenizer
through the [TokenCounter interface](adapter.py), adjust neutral padding, and
verify equality for each model separately. Keep byte-matched and token-matched
results in separate strata; retain unpadded counts as a padding sensitivity test.
The CLI currently builds the byte-matched stratum only.

The `projection` cost shape is illustrated in [projection.json](examples/projection.json).
For call index `i` starting at zero, total input is
`startup_tokens + context_tokens + i * history_growth_tokens`. The initial request
is cold. Later requests have expected cached input
`cached_prefix_tokens * cache_hit_share`; this share is conditional on the declared
eligible prefix, **not** the final whole-task cache hit share. It may represent
fractional expected tokens. History growth should include retained tool results
and previous assistant output; startup is never added again to that growth.
All calls reuse the declared prefix; prefix changes or eviction require actual
per-call usage records instead. No context-window limit or cache policy is simulated.

The `usage` shape in [usage.json](examples/usage.json) instead sums complete
per-model-call provider usage: input includes cached tokens, output includes all
billed reasoning tokens exactly once. Include failed attempts, retries and
compaction calls that were charged. Cached usage must be known and in range; an
unknown value fails rather than becoming a fabricated zero. Report unavailable
cache usage separately until the adapter can supply it. Different price tiers
or cache-write charges need separate pricing strata or an extended owner adapter.
The simple calculator covers input, discounted cached input and output prices.

Output separates raw `total_billed_tokens` from
`effective_input_tokens_at_uncached_rate` and `modeled_cost`. The prompt-cache hit
share is summed cached input divided by summed total input, not the average of
call percentages. Zero input yields an undefined (`null`) hit share. The calculator
counts **model requests**; tool operations can be batched, and must be logged and
reported separately. Wall time and answer accuracy require the actual experiment;
the offline tool never manufactures them.

## Model adapter boundary

[adapter.py](adapter.py) defines `ModelAdapter`, request/response/usage records,
and `TokenCounter`. There is deliberately no provider implementation or CLI
switch to call one. A future owner-run adapter must implement the protocol,
persist usage and transcripts, freeze configuration, and return results for blind
gold-fact grading. The schedule requires two distinct model labels and at least
three paired runs per question/model; labels alone do not prove different models.
For the full session experiment, implement the agent/tool loop and usage capture
as a separate runner. The pair builder does not approximate that loop.
