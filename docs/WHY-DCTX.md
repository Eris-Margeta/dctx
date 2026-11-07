# Why dctx exists

What problem dctx solves, the rules it follows, and why "byte-identical output" matters even if you never think about bytes.

## The problem

An AI coding assistant only knows what is in its **context window**: the text sent to the model with your request. It cannot see the rest of your project.

Most projects are too large to send whole, so tools pick pieces. Often they pick badly: whole files when one function would do, text that looks similar but is the wrong code, code as it was before your last edit, or half a function.

Each of these costs you. Models read text as **tokens** (small chunks of text, roughly a short word or part of one), and providers bill per token. The model reads every token before it answers, so more tokens also means more waiting. And answers get worse as irrelevant text piles up. The [README](../README.md#the-core-belief-prompt-processing-efficiency) sums it up: every token in front of a model is paid for three times, to read it, again when a cache is missed, and again in attention.

## What dctx does

dctx is a command-line tool that reads your project on your machine and answers precise questions about it: where a function is defined, what calls it, what its exact source is. Each answer fits a size budget and is built from whole pieces of syntax; a piece that is too long is marked as cut, with a way to get the rest. The same question about the same code gives the same bytes every time. An assistant can run the commands itself or call them through the built-in MCP server (`dctx mcp`; MCP is a standard way for AI tools to call other tools). dctx can also pack a whole project into one structured block for a chat.

Think of photocopying random pages of a book versus using its index. dctx is the index.

### A small worked example

Run with dctx 1.1.0 on a copy of this repository's `crates/dctx-lex-go` folder. The output is real and complete.

```text
$ dctx map find Add
# dctx-lex-go find
s:tests/fixtures/basic.go#Add	fn	L43-45

$ dctx map read 's:tests/fixtures/basic.go#Add'
# dctx-lex-go read
s:tests/fixtures/basic.go#Add	fn	tests/fixtures/basic.go
func Add[T ~int](a, b T) T {
	return a
}

$ dctx map neighbors 's:tests/fixtures/basic.go#Add' --edge calls --dir in
# dctx-lex-go neighbors
calls:in:s:tests/fixtures/basic.go#main	fn	tests/fixtures/basic.go	L59
```

`find` returns a stable id, the kind of thing it is (a function) and its lines. `read` returns the exact source. `neighbors` says that `main` calls it, on line 59.

## The philosophy

The [specification](SPEC.md#2-the-core-belief-prompt-processing-efficiency) turns this into five testable rules.

**Bounded.** Every map answer has a size ceiling, reserved before anything is written, and a record is never split. The budget is up to 2,000 estimated tokens (1,000 for `explain`).
*Why it matters to you:* one question cannot flood the context window and crowd out your own instructions.

**Deterministic.** The same code and the same question give the same bytes.
*Why it matters to you:* results are repeatable, and repeated prompts can be served from a cache.

**Prefix-stable.** When context is sent again across turns, bytes that did not need to change do not change.
*Why it matters to you:* this is what keeps caching working across a conversation.

**Non-redundant.** Within one session, do not resend what was already sent unless it changed. This one is still a target.

**Honest.** Every answer states its freshness, precision and provenance, and dctx never asserts what it does not know. With `--json`, the `read` above carries `"freshness":{"mode":"strict","state":"verified"}`, a `precision` block, and `more` and `next` fields for anything that did not fit. Calls it cannot resolve are listed as `unresolved`, not guessed.
*Why it matters to you:* you and your assistant can tell a fact from a guess.

The docs add a few more commitments:

- **Offline.** No telemetry, no update checks, no model calls. A release check counted zero network connections across 16 commands. Nothing leaves your machine.
- **Private by default.** A file is classified before any byte of it is read. Secret-shaped files such as `.env`, `*.pem` and `id_rsa` are listed with content omitted, with no override in v1.
- **No embeddings.** dctx does not turn code into vectors and search by similarity. It ranks by names and syntax.
- **Features must earn their place.** A feature without a measured acceptance gate is not a dctx feature.

## Why byte-identical output matters

### Prompt caching, in plain words

Much of each request to an AI model repeats the last one: system instructions, project context, earlier turns. Providers keep a cache of prompt beginnings. If the **start** of a new prompt matches a cached one exactly, byte for byte, they reuse their earlier work on that part. Cached input costs a fraction of the normal price and is processed faster.

The match must be exact, and it is checked from the beginning. Picture a reader who remembers the first fifty pages you sent earlier. Send the same fifty pages and they skip ahead. Change one word on page two, and from that word on they must read everything again, even if pages three to fifty are unchanged.

So one changed byte early in a prompt (a timestamp, a file listed in a different order, a file size) breaks the cache for everything after it.

### Prefix-stable, simply

"Prefix" means the beginning of a text. "Prefix-stable" means the beginning stays the same unless the content there really changed. Edit one file, and only the bytes from that file onward should differ.

Timestamps are the classic problem. In 1.0.0, every line of the `dctx pack` file tree carried a modification time, and file lines a size. The tree sits near the top, so merely re-saving a file (a `touch`) changed almost the whole page as far as a cache is concerned. Since 1.1.0 tree lines carry only an id. A bare `touch` leaves the output byte-identical, and an edit keeps everything before the edited file as a shared prefix. `--tree-stamps=on` brings back the old lines.

In the same folder we appended one comment line to the last file in the tree, `tests/review_lexers_a.rs`, and compared `dctx pack .` before and after:

```text
default output:      first 117,215 of 122,437 bytes unchanged
--tree-stamps=on:    first   2,151 of 123,159 bytes unchanged
```

By default about 96% of the page is still a reusable prefix. With timestamps on, almost nothing is.

### The evidence

The [stability report](research/stability-20261105/REPORT.md) measured map answers on 6 November 2026, on three pinned local corpora with 11 kinds of question each. **33 of 33** text answers and **33 of 33** JSON answers were byte-identical after an unrelated edit elsewhere in the project, and all were identical over five separate runs.

The report states its limits too. It measures dctx's answers, not a provider's cache hits. It ran on Linux only. The layered design that would keep context prefix-stable across a whole conversation ([SPEC §12.1](SPEC.md#121-prefix-stable-tiers-target)) is still a target.

### An illustration of money and time

*Made-up numbers, not any provider's prices.* Assume:

- an agent sends the same 20,000 tokens of project context on each of 10 turns;
- normal input costs 10 units per 1,000 tokens, cached input 1 unit;
- the cache holds between turns.

If the context is byte-identical, turn one pays 200 units and the next nine pay 20 each: 380 units. If a timestamp near the top changes every turn, every turn pays 200: 2,000 units. The model also re-reads all 20,000 tokens every turn instead of reusing them, so each answer takes longer. Real ratios differ by provider; the mechanism is the same.

### Other reasons determinism matters

- **Reproducible reviews.** A colleague who runs the same command on the same code sees exactly what the assistant saw.
- **Meaningful diffs.** If saved context changes, the code changed, not a clock or a sort order.
- **Trust.** A tool whose answers shift for no reason is hard to rely on. In the spec's words, "trust is the product".

## What dctx is not

- **Not a type checker.** Call edges come from names, scopes and imports; ambiguous references are dropped.
- **Not exact token counting.** Budgets use an estimate, `ceil(2 × UTF-8 bytes / 7)`, not a model's tokenizer.
- **Not vector search or RAG.** No embeddings in the core, and the engine never calls a model.
- **Not a hosted service or a daemon.** One binary that runs when you call it. No telemetry, not even opt-out.
- **Not finished everywhere.** Linux x86_64 is the only tested platform for v1. Plain `dctx pack` is not bounded; it sends the whole project. The desktop app is a source-only preview with no download yet.

## Where to go next

- [README](../README.md): install, quick start, map mode.
- [Specification](SPEC.md): rules, metrics, roadmap.
- [Benchmark](BENCHMARK-20261030.md): dctx against Trail-Code and graphify, including where they win.
- [Stability report](research/stability-20261105/REPORT.md): protocol, raw answers and limits.
- [Map mode guide](guides/MAP-MODE.md): every verb and option.

## Sources in this repository

- [README.md](../README.md): core belief, map-mode budgets, MCP, `pack` tree lines, `--tree-stamps`, tested platform.
- [CHANGELOG.md](../CHANGELOG.md): 1.1.0 prefix-stable `pack` output, stability re-measurement.
- [docs/SPEC.md](SPEC.md): §2 rules PPE-1 to PPE-5, §3 defects R1 and R7, §4 privacy and no network, §11 page formats, §12.1 tiers, §13 token estimate, §32 non-goals.
- [docs/research/stability-20261105/REPORT.md](research/stability-20261105/REPORT.md): 33/33 results and limits.
- [docs/BENCHMARK-20261030.md](BENCHMARK-20261030.md): `unresolved` call records.
- [Release and website verification](results/DCX-SITE-02.md): zero network connections in the release check; desktop preview.
- Worked example and prefix figures: output of dctx 1.1.0 (`a4f6378`, built from [crates/dctx-cli](../crates/dctx-cli)) on a copy of [crates/dctx-lex-go](../crates/dctx-lex-go).
