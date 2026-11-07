<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# The dctx Context Management Specification

Version 1.0 · draft 2 · 21.10.2026 · Apache-2.0 · measured against
`docs/research/trail-code-2026-10` (dctx `17dff0b`, Trail-Code 0.23.1)

This document specifies **dctx**, a context engine for AI-assisted software development.
It is normative where it uses **MUST**, **MUST NOT**, **SHOULD** and **MAY** (RFC 2119).
Every claim it makes about the product is either marked **landed** (present in the
repository today), **contract** (a versioned contract in `docs/design/contracts/` that is
partly implemented) or **target** (specified here, not yet built), and every target
carries an acceptance gate. Nothing in this document is a marketing number: measured
values cite the pinned benchmark in `docs/research/trail-code-2026-10/REPORT.md` or the
contract that defines them.

---

## Contents

- **Part I — Principles**
  1. Purpose and the workflows served
  2. The core belief: prompt-processing efficiency
  3. Zero RAG defects
  4. Privacy and authority
- **Part II — Model**
  5. Identity
  6. The graph: nodes, ids, outcomes
  7. Edges, confidence and provenance
  8. Precision labels
  9. Freshness
- **Part III — Answers**
  10. The verbs
  11. Page formats and byte stability
  12. Tiers, deltas, deduplication and compression levels
  13. Token estimation
- **Part IV — Policy**
  14. One language: DTS v2
  15. The seven-layer policy lattice
  16. Organization bundles, signing and enrollment
  17. Explain and receipts
- **Part V — Index**
  18. Segments, manifests and the per-file fact store
  19. Freshness check and incremental rebuild
  20. Cold build
  21. Ranking
- **Part VI — Extension**
  22. Core, surface or plugin: the decision procedure
  23. Plugin protocol and its v1.x extensions
  24. Surfaces: CLI, library, MCP, agent hooks, bindings, desktop
  25. Blast radius
  26. Learnings
- **Part VII — Assurance**
  27. Dependencies and build profiles
  28. Conformance suite
  29. Formal models
  30. Versioning and compatibility
- **Part VIII — Delivery**
  31. Roadmap and acceptance gates (31.0 Measured baseline · 31.1 Phases)
  32. Non-goals
- Appendices: A. Lineage · B. Comparison with Trail-Code · C. Reserved names · D. Document map

---

# Part I — Principles

## 1. Purpose and the workflows served

dctx turns a directory of source code into **the smallest, most cache-stable, provably
authorized set of bytes that lets a model or a human act correctly** — and it can prove
each of those four adjectives with a test.

It is one engine for every way software is built today:

| Workflow | dctx instrument | What "efficient" means there |
|---|---|---|
| Hand-coding, reading a new codebase | `dctx map` outline, `find`, `read`; desktop app (free) | Fast orientation without opening forty files |
| Copy-paste into a chat assistant | `dctx pack` (full / diff), templates, clipboard | Fewest bytes that still let a human and a model share one view; re-send only what changed |
| Tab completion and IDE assistants | library facade (`crates/dctx`), bindings | Freshness in ≤50 ms, span reads in milliseconds, for the editor's own context budget |
| Agents with a shell tool | `dctx map <verb>` | One line per record, copyable ids, bounded pages, always a next step |
| Agents with a tool protocol | MCP surface (`dctx serve --mcp`) | The same records and budgets as the CLI, byte for byte; small tool schemas |
| Software factories and CI | facade + organization policy + plugins | Deterministic bytes under one signed policy; receipts; no network unless granted |

dctx is not a code search engine, not a vector store, not an IDE and not a model. It
never calls a model and never sends a byte off the machine on its own.
The measured baseline for every claim below is §31.0.

## 2. The core belief: prompt-processing efficiency

Every token placed in a model's context is paid for three times:

| Cost | What it is | What dctx controls |
|---|---|---|
| **Prefill** | Input tokens the model must process before it answers. Billed per token; latency grows with it | Bytes per answer; spans instead of whole files; structural compression |
| **Cache misses** | Providers serve repeated prompt prefixes from a cache at a fraction of the price and latency. One changed byte early in a prompt invalidates everything after it | Byte-stable rendering, canonical order, stable ids, prefix-stable tiers |
| **Attention dilution** | Answer quality falls as irrelevant tokens grow | Ranked, bounded answers; one span per read; no repeats within a session |

dctx treats these as engineering constraints, not goals. A conforming implementation
MUST satisfy the five **PPE** constraints:

| Id | Constraint | Status |
|---|---|---|
| **PPE-1 Bounded** | Every answer has a declared byte and estimated-token ceiling, reserved before records are written; records are never split | landed (`dctx_core::budget`, API-PORTS-v1 `BoundedSink`) |
| **PPE-2 Deterministic** | Same source snapshot, policy, plan, ranking revision and request ⇒ identical bytes, on every OS and thread count | landed on Linux (every text verb identical over 5 runs; property P6, canonical payload); cross-OS gate open (§11) |
| **PPE-3 Prefix-stable** | Context injected across turns is layered so that bytes that did not need to change do not change (§12) | tier layering remains target (§12.1); on 2026-11-06, `0495f5f` preserved 33/33 text and 33/33 JSON answers after unrelated comment edits on three pinned local corpora; all identical over five runs, paired-answer M7 = 1.0 ([protocol and limits](https://www.dctx.internet.technology/docs/stability/)); historical baseline 0/63 (§31.0) |
| **PPE-4 Non-redundant** | Within one session a record already delivered is not re-sent unless its content changed (§12.4) | target |
| **PPE-5 Honest** | Every answer states its freshness, precision and provenance; the engine never asserts what it does not know | landed (`precision.*`, `freshness.*` in the wire format) |

Seven metrics measure them. M1–M5 come from the map-mode design; M6–M7 make prompt
caching first-class:

| Metric | Definition |
|---|---|
| M1 | Input tokens per accepted task |
| M2 | Exploration tokens per task (results of read-only tool calls, including dctx's own output) |
| M3 | Tool calls before the first edit |
| M4 | Task acceptance rate — the guard: no saving counts if M4 falls |
| M5 | dctx's own output tokens (counted inside M2, so the tool cannot hide its cost) |
| M6 | Provider prefix-cache hit ratio: cached input tokens ÷ input tokens |
| M7 | Prefix stability: common byte prefix of consecutive injected context blocks ÷ block length. Measured today as answers identical after an unrelated edit: 0/63 (comparison tool 59/70) |
| M8 | Task recall: gold facts found ÷ gold facts on the published 70-question set — the offline proxy for M4. Baseline 169/354 = 0.48; comparison tool 312/354 = 0.88 |

**Rule.** A feature without an acceptance gate stated in M1–M8, or in a latency,
memory or correctness bound measured by the suites in §28, is not a dctx feature.
M8 (offline) or M4 (online) MUST accompany any token-saving claim: a tool that
answers nothing is infinitely efficient.

## 3. Zero RAG defects

Retrieval-augmented generation over embedded chunks fails in characteristic ways. dctx
names ten defect classes, eliminates each by a specific mechanism, and ships a
**defect corpus** — fixtures with a positive control per class — that `dctx conformance`
runs (§28). A build that fails any row MUST NOT call itself dctx-conformant.

| # | Defect class | Symptom | dctx mechanism | Status |
|---|---|---|---|---|
| R1 | Fragmentation | Chunks cut through declarations; signature without body | Records are syntax-owned spans; `read` returns the whole node or a marked prefix with a continuation; records are never split | landed |
| R2 | Non-determinism | Same question, different chunks (approximate search, insertion-order ties) | Integer scores, order (score desc, id asc), frozen ranking revision, canonical payload | landed |
| R3 | Staleness | Index built from old bytes, served as current | There is no incremental index and no stale flag. Every CLI process re-walks the tree and re-hashes the selected files; a persisted generation is reused only when every content digest still matches, and any change rebuilds the whole generation. Inside one session `read` re-hashes the bytes against the snapshot digest and refuses with `SourceChanged` when they differ. The CLI always takes strict snapshots, so `freshness.state` is always `"verified"` (the library reports `"metadata"` only for a `FreshnessMode::Metadata` request) | landed (re-verify on every process; no stale flag) |
| R4 | Positional identity | Chunk ids that shift when a file is added | Path + qualified-name ids with ordinals; explicit `moved`, `gone`, `ambiguous` outcomes | landed |
| R5 | Semantic drift | "Similar" but wrong code | Lexical ranking over split identifiers, exact-name boost, kind filters, then graph edges with explicit confidence | contract — lexical ranking landed; graph edges are **not emitted by the shipped CLI** (TC-F01); the resolver is landed in `crates/dctx-map-resolve` but not wired (§7); `typed` rung target |
| R6 | Authority leak | Secrets, ignored and generated files reach the prompt | Selection precedes content: excluded files are never opened; policy rechecked at emission; plugins never receive excluded bytes | landed for map, pack and MCP: the strict secret-shaped deny of §4 item 3 has no override |
| R7 | Unbounded context | Top-k chunks of unknown size crowd out instructions | Hard per-verb caps; byte ceiling derived from tokens; envelope reserved first | landed for map verbs; the legacy tree (`dctx . -t`) and `pack` are unbounded (974 KB and 8.7 MB measured) and are not conformant surfaces |
| R8 | Opaque provenance | The model cannot tell a fact from an inference | Every record carries parser and coverage; every edge carries confidence and source | landed for records; vacuous for edges until §7 lands |
| R9 | Hidden tool cost | The retrieval tool's own output is not counted | M5 inside M2; `dctx bench tokens` (§28) | target |
| R10 | Summary drift | Model-written summaries describe code as it was | The engine never calls a model; imported summaries are served only while their body fingerprint matches | landed (rule); import target |

Embeddings never enter the core. If lexical recall is ever proven insufficient on the
public corpus, ranked candidates MAY arrive as a plugin overlay with provenance
`embedding:<model>` — labeled, optional, never default.

## 4. Privacy and authority

1. **Selection before content.** A path is classified (`Include`, `Exclude`,
   `OmitContent`, `DebugVisible`) before any byte of it is read, hashed, parsed or
   cached. `Exclude`, `OmitContent` and `DebugVisible` never authorize content. Landed;
   evidence: read-spy tests in `crates/dctx-adapter-fs/tests/privacy.rs` and
   `crates/dctx-map-query/tests/source_privacy.rs`.
2. **Rules.** Built-in defaults, repository `.gitignore` files (read as plain text),
   `.dctxignore` and policy (`privacy.ignore`) combine. Landed for
   pack and map.
3. **Secret-shaped files (landed for v1).** A strict, built-in deny covers `.env`,
   `.env.*`, `*.env`, `.envrc`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.jks`, `*.keystore`,
   `id_rsa`, `id_dsa`, `id_ecdsa`, `id_ed25519` (and `_`/`-` suffixed variants), `.netrc`,
   `_netrc`, `.npmrc`, `.pypirc`, `.pgpass`, `.htpasswd`, `.git-credentials`, `credentials`,
   `credentials.<data>`, `*.credentials.json` and `secrets.<data>` (data = json, yaml, yml,
   toml, ini, xml); ADR-014 "SENSITIVE" is the normative list. Such files appear in trees with
   content omitted and are never read, hashed or indexed by pack, any map verb or the MCP tools.
   v1 has no override: no `--allow-sensitive` flag, and no local rule can negate the deny.
   Policy MAY later add named classes (`aws`, `gcp`, `ssh`, `jwt`). dctx never scans contents
   to decide. **Gate:** the adversarial corpus's planted `.env` value is
   absent from `pack` output and from the map cache; the path is listed. Checked for every
   shape and every command by `<repo>/scripts/checks/V1-1.zsh` (TC-F08 is closed).
4. **No VCS in the core.** The core never reads `.git/`, never spawns `git`, never uses
   VCS object ids. Identity is dctx's own digest (§5). VCS-aware behaviour is a plugin.
   Landed (`crates/dctx-map-query/tests/no_git.rs`).
5. **No network in the core.** No telemetry, no update checks, no model calls. The only
   legal telemetry value in policy is `"none"`. Measured: zero socket system calls in
   every audited dctx run (§31.0).
6. **Re-check at emission.** Policy is checked again before bytes leave the engine, even
   for a pinned snapshot. A policy change invalidates cached authorization.

---

# Part II — Model

## 5. Identity

Identity is defined by `docs/design/contracts/IDENTITY-v1.md` and summarized here.

| Key | Definition |
|---|---|
| `ObjectKey` | Algorithm-tagged first-party SHA-256 of the exact authorized bytes, plus byte length (`crates/dctx-hash`) |
| `FactKey` | `ObjectKey` + language + parser identity and revision + extraction-rule digest |
| `SourceSnapshot` | Sorted manifest of (path, stamp, `ObjectKey`) for the authorized view |
| `ViewKey` | Snapshot digest + selection-policy digest + plan digest |
| `view` digest (target) | SHA-256 over the `ViewKey` framing and the canonical request bytes; printed in the provenance trailer (`--provenance`, §11) so a caller can name what it already holds |
| Policy digests | One per layer plus the effective digest (§15); part of `ViewKey` |

Git hashes are never an input and never a fallback.

## 6. The graph: nodes, ids, outcomes

Nodes: `place`, `package`, `module`, `symbol` (with kinds `fn`, `method`, `struct`,
`enum`, `trait`, `impl`, `const`, `macro`, `class`, `interface`, `type`, `test`) and
`doc` sections. Ids are stable, human-copyable strings:

```text
m:crates/dctx/src/lib.rs              module
s:crates/dctx/src/lib.rs#Session      symbol (qualified name; ~n ordinal on collisions)
d:docs/SPEC.md#the-graph              document section
@3f9c1a                               alias for ids longer than 120 bytes
```

A request for an id that no longer resolves returns an **outcome**, never a silent
empty answer: `moved` (with the new id, from body-fingerprint evidence), `gone`, or
`ambiguous` (with candidates).

## 7. Edges, confidence and provenance

Edge kinds: `contains`, `imports`, `calls`, `implements`, `uses-type`, `tests`,
`documents`, `reexports`.

**Status.** Kinds and the confidence lattice are landed in `dctx-map-model`;
reference extraction is landed in the lexers; resolution is landed in
`crates/dctx-map-resolve`. **The facade index (`crates/dctx/src/index`) emits only
`contains` edges today**, so `neighbors --edge calls|imports` was empty on 140 of 140
benchmark targets (§31.0). Wiring the resolver into the index is P0 (§31).

Confidence is a four-rung lattice. Every edge carries exactly one rung and a source:

| Rung | Meaning | Source |
|---|---|---|
| `exact` | Bound by syntax and scope, or by an import that resolves to a file in the view, or supplied by a compiler-grade overlay. Measured expectation: a comparison tool reaches precision 0.94–1.00 at recall 0.10–0.96 with syntax-and-scope binding alone; dctx MUST reach precision ≥ 0.95 before its edges may be shown without a `guess` label | lexer + resolver; `lsp:<server>@<version>` overlays |
| `typed` (target) | Method call resolved through the receiver's inferred type (§7.1) | binder |
| `name` | Unique declaration with that name in scope or in the view | resolver |
| `guess` | Bare-name match among several candidates | resolver |

An edge labeled `exact` that is wrong is a **defect**, not a statistic.

### 7.1 The binder (target: `dctx-map-bind`, pure crate)

A scope graph sits between parse and resolve. Lexers add three cheap per-reference
fields: `receiver` (head identifier of the receiver expression), `arity`, and
`binding_hint` (declared annotation, constructor name or import alias that produced a
local). Resolution walks:

1. lexical scopes outward (shadowing honored) → `exact` on a unique declaration;
2. imports, resolved to files from the view inventory and manifests already lexed by
   `dctx-lex-cfg` (`tsconfig` paths, `go.mod`, `Cargo.toml`, `composer.json`,
   `pyproject`, SwiftPM targets) → `exact`;
3. the receiver's type by shallow, deterministic rules — `self`/`this`, constructor,
   annotation, field type, import alias, annotated return type of the last call in a
   chain — then the method among that type's members and its declared supertypes →
   `typed`;
4. a unique declaration in the view with a compatible kind → `name`;
5. otherwise `guess` or unresolved.

No unification, no cross-assignment inference, no generics. Anything deeper is the job
of the compiler-grade overlay plugin (§23.2), whose edges carry provenance and never
replace first-party facts.

**Gates.** Measured on the published gold (`docs/research/trail-code-2026-10/data/targets/`,
140 definition targets in five categories, micro-averaged over call sites):

| Phase | Requirement |
|---|---|
| P0 (resolver wired) | precision ≥ 0.95; recall ≥ 0.88 on *rare* (comparison 0.93), ≥ 0.81 on *collision* (0.86), ≥ 0.35 on *hub* (0.42), ≥ 0.25 on *receiver* (0.56); zero wrong `exact` |
| P3 (binder) | receiver recall ≥ 0.56, interface name-level recall ≥ 0.42, hub ≥ 0.42; `exact ∪ typed` precision ≥ 0.95; zero wrong `exact`; callees question ≥ 10/14 (today 2/14) |

The same corpus is run against other tools for any published comparison. A held-out
edge corpus of ≥50 labeled call sites per language (TypeScript, Python, Go, Java first)
extends it before the `typed` rung is promoted.

## 8. Precision labels

Every answer carries `precision.coverage` (`structural` | `syntax` | `opaque`),
`precision.completion` (`complete` | `partial` | `refused`) and `precision.parser`
(for example `dctx-structural/1`, `syn/2`). Unsupported files are `opaque`: listed,
never invented. Landed.

**Omissions (target, P1).** `precision.omitted` counts paths excluded for reasons other
than policy — `{binary, unnamed, oversize, capped}` — so an agent knows the map is
incomplete (TC-F13: non-UTF-8 names, names containing a newline and NUL-containing
sources were skipped silently). Files whose facts exceed
`extraction.max_facts_per_file` are labeled `partial` (TC-F07: one 6.7 MB JSON fixture
produced 235,842 symbols and a 158 MB generation, 2.9 times the working tree).

## 9. Freshness

| Mode | Meaning |
|---|---|
| `metadata_validated` | Stamps (size, mtime, inode where available) agree with the manifest; touched files are digest-verified |
| `strict_live` | Every authorized file in scope is digest-verified before answering |
| `pinned` | The answer is served from a named snapshot digest |

`read` always verifies its own bytes, in every mode (landed). Answers carry
`freshness.mode` and `freshness.state`. The age in seconds (`age_s`) and the snapshot
digest are **opt-in** (`--provenance`, §11), because either in the default page defeats
prefix caching (PPE-3). **Status:** target; today both are in every default answer.
Measured: both compared tools saw unstaged, untracked and staged edits and dropped
reverted symbols; dctx's first query after a one-line edit costs a full re-index (core
7.48 s against 1.45 s warm; django 2.70 s against 0.82 s) until §19 lands.
**Shipped behaviour (v1):** the modes above are a target. The CLI always takes a strict
snapshot, so `freshness.state` is always `"verified"`; there is no stale flag (R3, §3).

---

# Part III — Answers

## 10. The verbs

```text
dctx map <map|find|neighbors|explain|read|grep|status> [TARGET] [OPTIONS]
```

| Verb | Use | Budget cap (tokens) |
|---|---|---|
| `map [TARGET]` | Outline a node or file (`--depth 1..3`) — landed; the project root, `p:`, `k:` and directory targets as a paged overview — **target, P1** (today refused or `gone`, TC-F05; the only overview is the unbounded legacy tree) | 2,000 |
| `find TARGET` | Search names and concepts (`--kind`, `--in`) | 2,000 |
| `neighbors TARGET` | Relations (`--edge`, `--dir in\|out\|both`, `--depth 1..4`) | 2,000 |
| `explain TARGET` | One node's details: signature, doc line, edges summary | 1,000 |
| `read TARGET` | Exact source (`--lines A-B`, `--full` up to 400 lines, `--level`) | 2,000 |
| `grep PATTERN` (target, P1) | Literal or simple-glob text search over authorized files under the same ignore rules, grouped by enclosing symbol id, budgeted, paged (today: no text search; config-key questions scored 0/14) | 2,000 |
| `status` (target) | Freshness, generation, policy digest; reads the manifest only; ≤50 ms | 200 |

Paging uses an opaque `next` cursor, never offsets. Every page ends with a next-step
hint. Planned additions: `dctx map plan` (budget planner, §12.5), `dctx blast` (§25),
`dctx check` (§26).

**Degradation (target, P1).** A budget below the smallest full record MUST return ids
only, then `more`/`next`, never exit 1 (today 35 of 84 small-budget requests fail with
"budget too small", TC-F09).

**Concept queries (target, P1).** `find` tokenizes the query and identifiers on case,
`_`, `-` and whitespace and ranks by token overlap with names, paths and `summary`;
today multi-word queries return no records (0 of 7 concept questions, TC-F10).

## 11. Page formats and byte stability

Two formats: the **text page** (default; one record per line; ids first) and **JSON v2**
(`--json`; no insignificant whitespace; LF-terminated; field order fixed). Both obey:

- no timestamps, snapshot digests, absolute paths, process ids, random values or
  locale-dependent text in the default output; provenance (snapshot digest,
  generation, age) is a **trailer line** behind `--provenance`, or a JSON field behind
  `--json --provenance` (P0 header prerequisite measured on 2026-11-06: all 33 tested
  text answers start with `# <place> <verb>`; all 33 tested default JSON answers omit
  snapshot, generation and `age_s`, and are identical over five runs and after an
  unrelated comment edit. Shipped opt-in flag: `--meta`; the specified provenance
  trailer and `--provenance` interface remain target. [Evidence](https://www.dctx.internet.technology/docs/stability/), TC-F03);
- the text page carries content: `find` and `map` records are
  `id<TAB>kind<TAB>path:L1-L2<TAB>summary`; `explain` prints signature, summary and edge
  counts by kind; `read` prints the source span (P0 status, 2026-11-06: text `read`
  carried source in all three measured cases; text `explain` still printed only
  id/kind/path. Richer text fields remain target; [evidence](https://www.dctx.internet.technology/docs/stability/), TC-F04);
- JSON v2 omits `null` fields (target, P0: today thirteen keys per record, many null,
  so a JSON `find` fit 4 records where the text page fit 10);
- canonical order (score desc, id asc; adjacency grouped by edge kind);
- paths are root-relative with `/` separators on every OS;
- the same request through CLI, library and MCP yields the same canonical page bytes.

**Note (1.0.1, legacy source page).** The `dctx pack` / `dctx .` text page (the `[DCTX_LLM_CONTEXT_SNAPSHOT_V1]` envelope) follows the same no-timestamp rule by default: its tree and root lines carry the `ID` only, with no `MOD:` and no `SIZE:` (a size changes on every ordinary edit, and the tree precedes the file blocks), so a bare `touch` leaves the page byte-identical and an edit keeps the bytes before the edited file as a common prefix. `--tree-stamps=on` reproduces the 1.0.0 page with `MOD:<ms>` and `SIZE:` on every tree line; `{{{node.mod}}}` and `{{{node.size_part}}}` remain available to custom templates.

**Gates.** (P0) ≥ 95 % of text answers byte-identical after an unrelated edit (0 of 63
today; comparison tool 59 of 70); JSON identical over 5 runs. (P1) The map page for a
pinned tree is byte-identical on Linux, macOS and Windows and across thread counts.

## 12. Tiers, deltas, deduplication and compression levels

### 12.1 Prefix-stable tiers (target)

Context that a host injects into a prompt is rendered as tiers, ordered from least to
most volatile, each terminated by a fixed marker line:

| Tier | Content | Changes when |
|---|---|---|
| T0 | Verb reference and conventions | dctx version changes |
| T1 | Project outline (`map --depth 1`) | The outline changes |
| T2 | Focus outline, relevant guardrails | The task changes |
| T3 | Turn-local answers (`find`, `read`, …) | Every turn |

Prerequisite: §11's header rule. Tiers cannot be prefix-stable while the first line of
every answer names the snapshot.

Invariant: a change confined to tier *k* never alters a byte of tiers < *k*. Policy
`cache.breakpoints = ["T0","T1","T2"]` makes tier boundaries the cache breakpoints a
host maps onto its provider's mechanism. **Gate:** rendered twice with a change in tier
*k*, the common prefix length ≥ the byte offset of marker *k*; M7 ≥ 0.9 across turns on
recorded traces.

### 12.2 View digests

The provenance trailer (`--provenance`) names the answer's `view` digest (§5). A caller
that keeps it can ask for a delta later.

### 12.3 Delta answers (target)

`--since <view-digest>` recomputes the answer and emits only records whose content
digest changed, `-<id>` lines for removed records, and a header
`base=<view> changed=<n> removed=<m> unchanged=<u>`. An unknown base yields the full
answer with `base=none`, never an error. The engine stores record ids and digests per
view (not bytes) in a small, user-private, TTL-bound ledger. `dctx pack --since
<snapshot>` applies the same rule to whole files: unchanged files appear as one tree
line with their digest.

### 12.4 Session deduplication (target)

With `--session <id>` (implicit for an MCP session), a record already emitted in the
session renders as `=<id>` unless its digest changed or `--full` is given. Session
ledgers are per user, ≤1 MiB, ≤24 h.

### 12.5 Budget planner (target)

`dctx map plan --budget N <requests…>` allocates a total budget by marginal value —
named reads and explains first, then neighbors, then the find tail — and reports
`planned`, `used`, `left`. Deterministic greedy over already-scored records.

### 12.6 Compression levels (target)

`--level signature | skeleton | body` for `read` and for pack blocks:

| Level | Content |
|---|---|
| `signature` | Declaration line(s) and the first sentence of the doc comment |
| `skeleton` | Signature, nested declarations and control-flow keywords; bodies elided as `… N lines` |
| `body` | The exact span |

The level is part of the view digest. Defaults: map records use `signature`, `read`
uses `body`; DTS MAY set levels per selection (for example `skeleton` for `tests/**`).

## 13. Token estimation

The portable estimate is `ceil(2 × UTF-8 bytes / 7)` (landed). A host MAY supply an
exact tokenizer through a port capability for budgeting; dctx never ships a tokenizer
in the core. Reports always state raw bytes next to estimates.

---

# Part IV — Policy

## 14. One language: DTS v2

The dctx Template System is the only configuration and policy language. DTS v1
templates keep rendering byte-identically. DTS v2 compiles typed sections into one
sealed `ContextPlan` IR with per-section digests
(`docs/design/contracts/DTS-v2.md`, `crates/dctx-template`). Every mode — pack, map,
templates, cache, plugins, agent surfaces, learnings — consumes a compiled plan; there
is no second, hidden policy path.

| Section | Controls | Status |
|---|---|---|
| `SELECT` | Paths, languages, kinds; Include / Exclude / OmitContent / DebugVisible | contract |
| `INDEX` | Requested facts, parsers, extraction limits (`max_facts_per_file`) | contract |
| `BUDGET` | Bytes, tokens, records, depth, parse/memory/work bounds | contract |
| `VIEW` | Pack, map and future projections; compression levels | contract |
| `RENDER` | Partials, layouts, markers, bounded record iteration | landed (v1) |
| `PLUGINS` / `PLUGIN` | Enablement, order, pinned versions and digests, grants, typed config | landed |
| `POLICY` | Organization-wide ceilings (§15) | target |
| `TENANT` | Named narrowing of an organization policy for a team or group | target |

Example of one file controlling everything:

```ini
[DCTX v2]

[POLICY:acme]
privacy.ignore      += ["*.pem", "*.key", "secrets/", ".env*"]
privacy.secret_classes = ["aws", "gcp", "ssh", "jwt"]
telemetry            = "none"
freshness.min        = "metadata_validated"
cache.max_bytes      = 1_073_741_824
cache.root_ttl_days  = 14
budget.map.max_tokens = 1500
parsers.allow        = ["syn", "structural", "opaque"]
agents.hosts.allow   = ["shell", "mcp"]
learnings.kinds      = ["decisions", "guardrails"]
learnings.max_bytes  = 262_144

[PLUGINS:acme]
enabled = ["dctx/git"]
denied  = ["dctx/ci-github"]
network = "none"

[PLUGIN:dctx/git]
version = "1.0.0"
digest  = "sha256:…"
allow   = ["file.read:.gitignore", "file.write:.gitignore"]
config.stage = false

[TENANT:payments]
languages             = ["rust", "typescript"]
budget.map.max_tokens = 1200
learnings.kinds       = ["decisions"]
```

Compiler obligations: known keys only; `+=` only on append-only list fields; every
value within the parent layer; an unknown section or field is refused; the compiler
emits a digest per layer and the effective digest. The compiled artifact is the
existing `DCTXIR` envelope with section kinds `POLICY = 16`, `TENANT = 17` (18–31
reserved).

## 15. The seven-layer policy lattice

Today's rule — `effective_permission = host_grant ∩ organization_ceiling ∩
session_selection` (product contract C03) and the plugin grant lattice
`M ∩ U ∩ O ∩ H ∩ S(t) ∩ A` — generalizes to seven layers. Each layer can only
**narrow** the layers above it.

| Layer | Written by | Signed by | Typical content |
|---|---|---|---|
| L0 built-in | The release | The release key | Hard caps (2,000 tokens, 8 MiB per file, 2 GiB cache), `network = none`, `telemetry = none` |
| L1 organization | Security / platform team | Organization key (enrolled once) | Parser, plugin and agent-host allowlists; privacy rules; budgets; cache caps; learnings kinds |
| L2 tenant | Team lead | Organization key or a delegated team key | Narrower languages, plugin set, default templates, budgets |
| L3 machine | IT / device management | Machine key or a root-owned file | Cache root, offline mode, threads and memory, installer proxy |
| L4 user | The developer | — (user-private file) | Personal defaults, consented plugins, installed agent hosts |
| L5 project | The repository | Optional organization signature for "trusted project policy" | `.dctx/policy.dts`: selections, omit rules, templates, learnings location |
| L6 session | The invocation | — | Flags, environment, library builder arguments |

Rules:

1. **Monotone narrowing.** `effective = fold(narrow, L0 … L6)` per field: set
   intersection; numeric minimum; enums ranked toward the most restrictive; booleans
   AND for permissions and OR for `required` / `deny`. **A widening attempt is a typed
   compile error** naming the layer and field — never a silent clamp.
2. **Unreadable policy refuses.** A present bundle that fails verification blocks every
   mode except `dctx policy explain`.
3. **Policy epoch.** The effective digest is part of `ViewKey` and of every plugin
   admission; any layer change invalidates cached authorization before serving.
4. **Tenants.** A tenant is a named narrowing selected by L3 or L4, or requested by a
   project from those the organization lists. A project cannot invent a tenant.

**Property P9 (monotone policy).** For random layer stacks, the effective value is
within every layer field-wise, and a widening at layer *k* fails naming *k*. Mutation
witness: removing the intersection makes the property fail.

### 15.1 Locations

| OS | L3 machine | L4 user | L1/L2 organization bundles |
|---|---|---|---|
| Linux | `/etc/dctx/policy.d/*.dts` | `${XDG_CONFIG_HOME:-~/.config}/dctx/policy.dts` | `/etc/dctx/org/*.dctxpol` |
| macOS | `/Library/Application Support/dctx/policy.d/` | `~/Library/Application Support/dctx/policy.dts` | `/Library/Application Support/dctx/org/` (MDM-deployable) |
| Windows | `%ProgramData%\dctx\policy.d\` | `%APPDATA%\dctx\policy.dts` | `%ProgramData%\dctx\org\` |
| Project | `.dctx/policy.dts` | | |

`$DCTX_ORG_POLICY` MAY point at a bundle. The library never discovers locations
itself; the CLI and desktop pass them explicitly (product contract C01).

## 16. Organization bundles, signing and enrollment

A `.dctxpol` bundle reuses the plugin offline-bundle container (bounded table of named
payloads with lengths and digests; no compression, no links): `policy.dts`,
`tenants/<id>.dts`, `trust/keys.json`, `manifest.json`, `manifest.sig`. The signature
is Ed25519 over `dctx-org-policy-v1 ‖ manifest bytes`, verified by `crates/dctx-ed25519`.

- **Enrollment.** `dctx policy enroll --fingerprint <hex> <bundle>` pins the
  organization key once, interactively.
- **Updates.** A monotonic `sequence` is accepted forward only; downgrades are refused;
  a revocation list is honored. These mechanisms already exist for plugin distribution.
- **Distribution** is any file channel: MDM, configuration management, a repository the
  organization controls, or the installer's own download. dctx itself fetches nothing.

## 17. Explain and receipts

- `dctx policy explain [field]` prints each effective value, the layer that set it and
  the signer fingerprint. Byte-stable.
- Every operation records the effective policy digest, the per-layer digests and signer
  fingerprints in the local, user-private receipt log. `dctx policy receipts --since <t>`
  lists them. Nothing is uploaded.

---

# Part V — Index

## 18. Segments, manifests and the per-file fact store

The index is a set of immutable, checksummed segments (`docs/design/contracts/SEGMENTS-v1.md`,
`crates/dctx-map-segment`, `crates/dctx-adapter-store`) under the cache root. No SQLite,
no daemon.

```text
<cache root>/v2/<root-id>/
  facts/<FactKey-prefix>/<FactKey>.seg   per-file fact segments; a changed file reparses one segment
  views/<generation>/view.seg            nodes, forward and reverse adjacency, postings, dictionary
  views/<generation>/manifest.man        sorted path → (stamp, ObjectKey, FactKey)
  current.ptr                            atomic pointer to the published generation
  answers/<session>.ledger               delta and dedup ledger (§12.3–12.4)
```

- Identical files parse once across roots (shared `FactKey`).
- Publication uses a writer lock, staging, pointer swap, leases and a previous pointer
  for recovery (landed protocol; formal model in `docs/formal/dctx-cache`).
- Cache lifecycle: time-based expiry, a size cap and a bounded automatic cleaner
  (landed defaults: 7 days idle overlays, 30 days unpinned roots, 2 GiB hard cap).
- Readers validate section checksums lazily on first touch; the envelope checksum is
  mandatory at publication.
- Generations are content-addressed: an unchanged snapshot digest publishes no new
  bytes (today three byte-identical 158 MB generations of one root were stored).
  Superseded, unpinned generations of the same root are reclaimable at the next
  invocation; the unlink grace period applies to orphaned objects, not to superseded
  generations (today a one-day grace let a root grow from 158 MB to 946 MB while
  `cache gc --dry-run` reclaimed 0 bytes, TC-F02).

## 19. Freshness check and incremental rebuild

**Status: target, not implemented.** The shipped engine has no stamp-only pass, overlay or
per-file rebuild: each process re-walks and re-hashes, and any change rebuilds fully (R3, §3).

1. **Stamp pass.** Walk the inventory (names, sizes, mtimes; excluded directories are
   never entered) and compare with the manifest's sorted path table.
2. **Digest pass** only for entries whose stamp differs, plus the query's dependency
   scope under `strict_live`. `read` always verifies its own bytes.
3. **Rebuild.** Changed files reparse one fact segment each; their nodes and edges are
   masked by an overlay; references that named symbols they declare (or now declare) are
   re-resolved through the reverse unresolved-name index; the result publishes as an
   overlay generation. Overlays compact into the base past a threshold. An unchanged
   snapshot digest reuses the published generation without writing.

| Gate | Target | Measured today (§31.0) |
|---|---|---|
| `dctx map status` | ≤50 ms p95 | no verb |
| Warm `find` / `neighbors` / `read`, small and medium corpora | ≤30 ms engine p95, ≤80 ms process | 0.03–0.26 s process (comparison 0.69–1.40 s) |
| Warm `find` / `neighbors` / `read`, largest corpus (core, 3,190 files) | ≤0.8 s process, ≤150 MiB | 1.45 s, 370 MiB (comparison `callers` 0.83 s) |
| First query after a one-line edit | ≤2× warm | ≈ cold (core 7.48 s against 1.45 s warm; django 2.70 s against 0.82 s) |
| Cache per root after 10 edits | ≤1.5× one generation | 6× (158 MB → 946 MB after about five edits) |
| Single-file reindex | ≤50 ms | n/a |
| Correctness | Answers byte-identical to a cold rebuild | — |

## 20. Cold build

Parallel parsing with a bounded parser thread pool (`std::thread::scope`, ≤ available
parallelism); each thread owns its fact segments; a single sealer merges sorted outputs
and hashes incrementally without materializing a joined payload. Policy caps facts per
file (`extraction.max_facts_per_file`, default 10,000); excess configuration and JSON
leaves collapse into one `partial` module node.

**Gate:** cold build ≤5 s and peak RSS ≤150 MiB on the pinned corpus; persistent bytes
≤40 MiB. Measured today (§31.0): core (3,190 files) 6.05 s / 567 MiB; django (7,071
files) 2.48 s / 92 MiB; flask (235 files) 0.09 s / 8 MiB. Cold indexing is 2.8 to 69
times faster than the comparison tool on all seven corpora with 1.6 to 27 times less
peak memory, and MUST stay at or below half of its time per corpus.

## 21. Ranking

Lexical scoring over identifiers split on case and separators, path segments and the
first doc line; exact-name boost; kind and path filters; confidence-aware boosts for
graph neighbors of the focus set. Integer scores; ties broken by id; the scoring
revision is versioned and part of the view digest.

---

# Part VI — Extension

## 22. Core, surface or plugin: the decision procedure

```text
Does it read or write anything other than authorized source bytes, the dctx cache root
or a caller-supplied sink?
  no  → core: engine or adapter (first-party crate, std-only target)
  yes → does it only transport answers the core already produced, deciding nothing about content?
          yes → surface adapter (compiled in behind a feature: CLI, MCP, hook-file generator)
          no  → plugin (out of process, brokered, consented, receipted)
Does it need a socket, a subprocess or a foreign runtime (language server, compiler, model)?
  yes → plugin; never core; never a default-on effect
Does it change selection, identity, freshness, ranking or budgets?
  yes → refused as a plugin capability
```

| Component | Placement |
|---|---|
| Ids, spans, budgets, selection, digest, DTS compiler, segment codec, resolver, binder | Pure core |
| 13 lexer crates covering 15 language variants (Rust, JS, TS, TSX, Markdown, Python, Go, Java, C/C++, JSON/TOML/YAML, C#, Kotlin, Ruby, PHP, Swift); `syn` adapter | Core engine |
| Filesystem source, cache, segment store | Adapters |
| CLI, MCP server, agent-hook generator, statusline | Surfaces |
| Git (`.gitignore` protection, staging, `vcs.changes`) | First-party plugin (bundled) |
| Language-server / SCIP edges | First-party plugin, edge overlay |
| CI and pull-request comments | Plugin with the v2 network capability |
| Telemetry | Does not exist |
| Model calls | Never in dctx; summaries only by fingerprint-bound import |

## 23. Plugin protocol and its v1.x extensions

The plugin host is landed: a grant lattice with explicit denies, policy epochs with
re-checks at admission and commit, receipts and a ledger, liveness by progress (not
timers), signed distribution with pinned keys and revocation, Linux confinement, and
TLA+ models with mutation witnesses (`docs/design/PLUGINS.md`, `docs/formal/dctx-plugins`).
Properties PP1–PP10 hold for every extension below.

### 23.1 New capabilities and hooks (target, negotiated minor versions)

| Addition | Kind | Contract |
|---|---|---|
| `vcs.changes {scope: staged \| worktree \| range}` | Broker request (git plugin) | Bounded `{path, added, removed}` hunks; paths filtered by current selection |
| `facts.overlay {generation, segment}` | Broker request (overlay plugins) | Uploads an edge-only overlay segment with provenance; the host checks every endpoint exists and no row touches an excluded path; never merged into fact segments |
| `after-query` | Advisory hook | Verb, record ids, bytes and budget use — never record contents |
| `on-source-changed` | Advisory hook | Fired when a generation publishes; payload is the changed path list after selection |
| `network.post {origin, max_bytes}` | Capability (protocol v2) | Separate consent, organization egress allowlist, receipts with response digest; off in every default |

### 23.2 The compiler-grade overlay (`dctx/lsp`, `dctx/scip`)

Spawns a configured language server under confinement (no project writes, no network),
asks for definitions and references of the view's `name`-confidence and unresolved
references in bounded batches, and uploads an edge overlay labeled
`exact (lsp:<server>@<version>)`. `--no-overlay` disables it per request. SCIP import is
the batch variant for CI. Neither is a default; neither changes `FactKey`.

## 24. Surfaces

### 24.1 CLI

One binary: `dctx` (pack, map, template, cache, plugin, policy, agent, learn, check,
blast, serve). Interactive TUI when run without arguments.

### 24.2 Library facade

`crates/dctx` is the stable API for every surface (`docs/design/contracts/API-PORTS-v1.md`).
Bindings for Python, Node and a C ABI wrap the facade; none re-implements an engine.

### 24.3 MCP server (target, feature `mcp`)

`dctx serve --mcp` speaks JSON-RPC over stdio only. It is a surface, not a plugin: it
renders answers the facade already produced, so a broker hop would add cost without a
privacy gain. Tool schemas are prompt bytes on every call, so the set is small and the
schemas short:

| Tool | Maps to |
|---|---|
| `dctx_map` | `map` |
| `dctx_find` | `find` |
| `dctx_neighbors` | `neighbors` |
| `dctx_explain` | `explain` |
| `dctx_read` | `read` (always byte-verified) |
| `dctx_status` | `status` |

`dctx_blast` and `dctx_check` appear only when their plugins are enabled and granted.
Tool results carry the same canonical text page as the CLI, so prefix stability holds
across surfaces. A network listener is a policy-gated capability, never a default.

**Gate.** Tool definitions ≤ 600 estimated tokens per session (comparison tool: 6
tools, 3,515 bytes, about 1,005 tokens); results are the canonical text page. Answers
MUST NOT contain instructions to the agent: no banners, no "report savings" text (the
comparison tool spends about 310 bytes, roughly 89 tokens, per call on one).

### 24.4 Agent hooks (target)

`dctx agent install <host> [--project]` renders, from the effective policy, the host's
own configuration fragment: a statusline that runs `dctx map status`, a session-start
injection of tiers T0–T2 (≤1,000 tokens, placed after the host's stable prompt prefix),
and the guidance "use `dctx map find` / `read` before grep and cat". Installation is a
user-consented file write with a receipt, idempotent (marker line) and reversible
(`dctx agent uninstall`). Organization policy can allow, require or forbid hosts. No
daemon, no watcher.

### 24.5 Desktop

The desktop app is **free, forever**. It is a surface over the same facade: snapshots,
settings, recent projects, plugin management. It has no engine or cache behaviour of
its own and is never part of the CLI or library dependency closure.

## 25. Blast radius (target)

`dctx blast` traverses reverse adjacency (callers, implementors, tests, documents) from a
set of changed spans, bounded like `neighbors`. The graph side is core. The change side
has two sources, neither of which puts a VCS in the core:

1. **Snapshot diff (default).** `dctx blast --from <snapshot>` compares the current
   source snapshot with a retained one. Works in any directory.
2. **VCS supply.** The git plugin's `vcs.changes` returns hunks for `--staged`,
   `--branch <name>` and similar; the core maps hunks to node spans.

Formats: text records, JSON, and `--format pr-comment` (Markdown). Posting the comment
is a network plugin. **Gate:** ≤100 ms warm for a 10-hunk change on the pinned corpus.

## 26. Learnings (target)

Teams lose decisions and guardrails to chat logs. dctx keeps them as plain files next to
the code, without becoming a memory product.

| Aspect | Rule |
|---|---|
| Location | `.dctx/learnings/<kind>/<slug>.md`; kinds `decisions`, `guardrails`, `preferences`, `skills`; policy may narrow kinds and size |
| Format | Markdown; first heading is the title; a fenced `dctx` block with `applies:` ids, `since:` snapshot, `status: active \| retired`, optional `enforce: true` |
| Indexing | Ordinary `d:` doc nodes with `documents` edges to the ids they name; `find --kind learning`; boost when `applies` meets the focus set |
| Authoring | `dctx learn add <kind> "<title>" [--applies <id>…]`. The core writes only into `.dctx/learnings/` and only on an explicit user action |
| Sharing | It is content: shared through version control like any file; the git plugin may stage it when granted |
| `dctx check` | For each changed node (§25), lists guardrails whose `applies` covers it or its neighbors at depth 1. Advisory; exit non-zero only with `--strict` and an enforced active guardrail |
| Freshness | A learning that applies to a `gone` id is reported `orphaned`; nothing is deleted automatically |
| Model-written text | Only via `dctx learn import` with body-fingerprint binding; labeled `src=import:<tag>`; served only while fingerprints match |

---

# Part VII — Assurance

## 27. Dependencies and build profiles

All first-party code and documentation are Apache-2.0, copyright Eris Margeta,
including the desktop app and planned GPUI rebuild. This section records build
engineering goals, not an edition, license tier or certification of suitability.
The current dependency inventories are [THIRD_PARTY.md](https://github.com/Eris-Margeta/dctx/blob/main/THIRD_PARTY.md) and
[THIRD_PARTY_NPM.md](https://github.com/Eris-Margeta/dctx/blob/main/THIRD_PARTY_NPM.md); inspect manifests and features for an
exact build's closure. Earlier 0.25.0 dependency counts are historical measurements.

### 27.1 The rules

- **Core crates are std-only.** `dctx-hash`, `dctx-core`, `dctx-ports`, `dctx-map-model`
  (without `wire-json`), `dctx-map-segment`, `dctx-template`, `dctx-pack`,
  `dctx-map-resolve`, `dctx-map-bind`, `dctx-plugin-protocol`, the hand-written lexers,
  `dctx-adapter-fs`, `dctx-adapter-store`, `dctx-adapter-cache` and `dctx-map-scan`
  have zero third-party normal dependencies and forbid `unsafe`; CI checks
  `cargo tree -e normal` per crate.
- **Every third-party crate in a default build is listed** in `docs/DEPENDENCIES.md`
  with version, licence, `unsafe` count, reason and the plan for it. CI fails if the
  closure grows beyond the list.
- **Ed25519 stays for security and trust.** `ed25519-dalek` (exact pin, `std` feature
  only, verify-only) is a vetted, widely audited security crate and the **single
  disclosed exception** to the zero-third-party rule, behind feature `trust`. It
  verifies the signatures on plugins and organization policy bundles. A build without
  `trust` has no third-party crate and cannot verify signatures; it refuses unsigned
  bundles rather than trusting them. Audited cryptography is not replaced by something
  less proven; the search for the best long-term solution continues, and any successor
  must clear the same bar (RFC 8032 and Wycheproof vectors, differential fuzzing,
  independent review).

### 27.2 Planned removals

| Remove | First-party replacement | Gate |
|---|---|---|
| `syn` from the `strict` feature | Today `strict` implies `syntax-rust`; a minimal feature set MUST NOT (structural Rust extractor with `precision.parser = dctx-structural/1`) | Build proves the closure |
| `serde`, `serde_json`, `thiserror`, production `sha2` | The facade's hand-written canonical JSON; std `Error`; `dctx-hash` | Goldens byte-identical |
| `rusqlite` and bundled SQLite | The immutable segment index (§18) | Segment index beats the SQLite baseline on two of latency, disk and refresh with no quality loss |
| `syn` stack from the default build | First-party Rust structural extractor + binder (§7.1) | Held-out precision ≥ `syn` − 1 point; zero wrong `exact` edges |
| Linux confinement crates | Kept as a disclosed, target-specific adapter dependency outside the minimal closure (plugins unavailable in that build), or a reviewed first-party syscall boundary in `dctx-plugin-platform` | Confinement property tests; written `unsafe` review |

The desktop app is a separate surface over the facade and is never part of the CLI or
library closure.

### 27.3 Maintenance

dctx is Apache-2.0 open source, including the desktop app, with no license fee. Organizations that depend on dctx can sign a maintenance agreement for security response targets, long-term support and priority work; see [MAINTENANCE.md](https://www.dctx.internet.technology/maintenance/). Build-closure targets are engineering goals.

## 28. Conformance suite

| Suite | What it proves |
|---|---|
| Verb goldens (`docs/design/contracts/GOLDEN-REQUESTS-v1.json`) | Byte-identical pages across OS, thread count and surface |
| Property tests P1–P9 | Privacy, publication, recovery, determinism, bounds, policy monotonicity |
| Defect corpus R1–R10 | Each RAG defect class is absent, with a positive control per class |
| Edge corpus | Precision and recall per language and confidence rung (§7.1) |
| `dctx bench tokens` | M1–M7 from recorded traces (plain JSONL from the host); dctx never contacts a provider |
| Cross-tool benchmark | The isolated, pinned protocol in `docs/research/trail-code-2026-10/` (seven corpora and one adversarial, 140 targets, 70 questions, stability, freshness, adversarial, network audit). Rerun at every phase gate; numbers in §31.0 |

Published numbers MUST come from these suites with pinned corpus digests.

## 29. Formal models

TLA+ models with mutation witnesses cover the cache lifecycle (`docs/formal/dctx-cache`)
and the plugin host (`docs/formal/dctx-plugins`). New models are required for the policy
lattice (P9) and for delta and dedup ledgers (no record lost, none duplicated across a
base change).

## 30. Versioning and compatibility

Independent version axes: library API, wire format (text page, JSON v2), DTS source,
plan IR, identity, segments, cache layout, plugin protocol, policy bundle. Each axis
has a major/minor rule; readers refuse unknown required sections; a version
declaration selects behaviour — it is never inferred from an unknown field.

---

# Part VIII — Delivery

## 31. Roadmap and acceptance gates

### 31.0 Measured baseline

dctx `17dff0b` (`dctx-cli v0.25.0` development line) against Trail-Code 0.23.1 on
seven pinned corpora (flask, ripgrep, pocketbase, trailcode, dctx, django, core) and
one generated adversarial corpus; Linux, eight pinned CPUs, isolated network namespace;
full method, raw data and scripts in `docs/research/trail-code-2026-10/`. Every gate in
this section names the row it must move.

| Measure | dctx | Trail-Code | Source |
|---|---|---|---|
| Call-graph precision / recall (callers, depth 1; 140 definition targets) | — / 0.00 (no call or import edges emitted) | 0.94–1.00 / 0.10–0.96 per corpus | report §7, `data/accuracy.csv` |
| Recall by category: hub / rare / receiver / collision / interface | 0 / 0 / 0 / 0 / 0 | 0.42 / 0.93 / 0.56 / 0.86 / 0.42 | `data/accuracy_by_category.csv` |
| Definition ranked first (`find` against `ask`) | 103 of 140 | 79 of 140 | report §7 |
| 70 agent questions: score / facts / estimated tokens | 69 of 140 · 169 of 354 · 500,005 (480k from the unbounded legacy tree) | 122 of 140 · 312 of 354 · 88,691 | `data/tasks.csv` |
| Facts per 1,000 tokens, excluding the overview question | 7.15 | 4.98 | report §8 |
| Question types (score of 14): config key / concept / callees / impact / definition+callers / collision | 0 / 0 / 2 / 7 / 7 / 6 | 14 / 12 / 8 / 13 / 10 / 12 | report §8 |
| Cold index, seven corpora: wall / peak RSS | 0.09–6.05 s / 8–567 MiB | 2.44–94.89 s / 181–2,111 MiB | `data/indexing.csv` |
| Warm `find`: small and medium corpora / largest corpus (core) | 0.03–0.26 s / 1.45 s | 0.69–1.40 s / 1.65 s (`callers` 0.83 s) | `data/queries_warm.csv` |
| First query after a one-line edit: core / django | 7.48 s / 2.70 s | 5.45 s / 11.30 s | `data/freshness.csv` |
| Cache growth after about five edits (core) | 158 MB → 946 MB; `gc --dry-run` reclaimed 0 bytes | constant 65 MB | report §6 |
| Text answers identical after an unrelated edit | Historical: 0 of 63 (`17dff0b`); 2026-11-06: 33 of 33 (`0495f5f`, three pinned local corpora, unrelated comment edits); JSON 33 of 33, also identical over five runs; paired-answer M7 = 1.0 | Historical: 59 of 70 (not rerun) | Historical: `data/stability.csv`; [new CSV](https://github.com/Eris-Margeta/dctx/blob/main/docs/research/stability-20261105/stability.csv), [protocol and limits](https://www.dctx.internet.technology/docs/stability/) |
| Budget cap honoured | 2,000-token cap never exceeded (worst answer 6,996 bytes); 35 of 84 small budgets fail with exit 1 | no cap; one answer 142,197 bytes | report §13 |
| Install footprint | one 9,207,072-byte binary | 405,601,700 bytes of `node_modules` plus Node | report §3 |
| Sockets in audited runs | 0 | 30 in a daily update check that ignores the documented opt-out | report §11 |
| Repository writes in map mode | none | `.gitignore` modified and `.ignore` added on every build | report §4 |
| Gitignored secret with no `.git` present | excluded | indexed | report §10 |
| Non-ignored `.env` in `pack` / `build` | **printed by `pack`** | never read | report §10 |
| MCP | none | 6 tools; 3,515 bytes (about 1,005 tokens) of schema per session | report §3 |

Where dctx answered, it was the terser tool; where it scored lower, the answers were
empty or thin (no edges, id-only text pages, no bounded overview, no text search, no
multi-word search). The roadmap below closes those holes first, with code that already
exists, before the architectural work.

### 31.1 Phases

All gates are measured by rerunning the benchmark scripts on the same pinned corpora and
gold; values in parentheses are today's (§31.0). E1 and E2 run in parallel with P0–P4
because they touch `crates/dctx-template`, manifests and CI, not the index.

| Phase | Scope | Acceptance (all must hold) |
|---|---|---|
| **P0 Close the holes** (§4 item 3, §7, §11) | Wire reference resolution into the facade index; text pages carry content; provenance moves to an opt-in trailer; secret-shaped omit defaults | `neighbors --edge calls --dir in` non-empty on ≥ 120 of 140 targets; precision (`exact ∪ name`) ≥ 0.95; recall ≥ 0.88 on *rare* (comparison 0.93), ≥ 0.81 on *collision* (0.86), ≥ 0.35 on *hub* (0.42), ≥ 0.25 on *receiver* (0.56); zero wrong `exact`. Text answers identical after an unrelated edit ≥ 95 % (0 of 63); JSON identical over 5 runs. Task score ≥ 95 of 140 (69). The adversarial `.env` value absent from `pack` output; path listed |
| **P1 Answer the questions agents ask** (§8, §10) | Bounded overview for `map` without a target, `p:` and directory targets; `grep`; `find` tokenization; small budgets degrade; omission counts in `precision` | Overview question 14 of 14 with ≤ 15,000 tokens over seven corpora (480,000); config-key ≥ 12 of 14 (0); concept ≥ 10 of 14 (0); every budgeted request returns a page (35 of 84 failed); task score ≥ 115 of 140 at ≤ 100,000 tokens (comparison 122 at 88,691); M8 ≥ 0.80 (0.48) |
| **P2 Persistent incremental index** (§18–20) | Per-file fact segments keyed by `FactKey`; content-addressed generations; superseded same-root generations reclaimed at the next invocation; two-level freshness; lazy per-section loads; parallel cold build; per-file fact cap; `status` | First query after a one-line edit ≤ 2× warm on every corpus (core 7.48 s → ≤ 3.0 s; django 2.70 s → ≤ 1.7 s); cache per root ≤ 1.5× one generation after 10 edits (946 MB → ≤ 240 MB); `gc` reclaims superseded bytes; warm queries on core ≤ 0.8 s and ≤ 150 MiB (1.45 s / 370 MiB; comparison 0.83 s); cold core ≤ 5 s / ≤ 150 MiB (6.05 s / 567 MiB); cold time ≤ 0.5× comparison on every corpus; `status` ≤ 50 ms |
| **P3 Binder** (§7.1) | `dctx-map-bind`: scope graph, per-language import resolution from lexed manifests, receiver-type rules, `typed` rung, edge corpus | Receiver recall ≥ 0.56, interface name-level ≥ 0.42, hub ≥ 0.42; `exact ∪ typed` precision ≥ 0.95; zero wrong `exact`; callees question ≥ 10 of 14 (2) |
| **P4 Token economics** (§12) | Tiers T0–T3, view digests, `--since` deltas, session dedup, compression levels, planner, `dctx bench tokens`, agent seeding | M7 ≥ 0.9 across turns; on ≥ 30 accepted tasks per route: M2 −50 %, M1 −25 %, M3 −50 %, M4 unchanged; task tokens ≤ 0.8× comparison at M8 ≥ 0.85; M6 reported |
| **E1 Organization control** (§14–17; parallel) | `POLICY` / `TENANT`, seven layers, `.dctxpol`, enrollment, explain, receipts | P9; one bundle demonstrably controls every mode; an unverifiable bundle refuses |
| **E2 Minimal build profile** (§27; parallel) | Removals in §27.2; `docs/DEPENDENCIES.md` with CI diff; truthful `--version`; version strings matching the actual release or development version | `cargo tree -e normal` for a minimal feature set lists only `dctx-*` crates and the Ed25519 verifier closure behind `trust`; the `strict` feature no longer implies `syn` |
| **P5 Surfaces** (§24–26) | MCP, agent hooks, `blast`, learnings and `check`, `vcs.changes` | MCP pages byte-identical to CLI; tool definitions ≤ 600 tokens (comparison 1,005); hooks idempotent with receipts; `blast` ≤ 100 ms warm |
| **P6 Overlays and D3** (§23.2, §27.2) | LSP and SCIP overlays; network capability; CI comment plugin; first-party Rust structural extractor replaces `syn` in the default build | Overlays never alter first-party facts; PP1–PP10 still modeled; `syn` removed only at corpus parity |

P0 and P1 turn a 69-of-140 tool into a ≥ 115-of-140 tool with changes measured in days,
using lexers and a resolver that already exist. P2 is the one architectural piece, and
it is also where dctx loses to the comparison tool on the largest corpus. P3 is the
comparison people will make next. P4 pays only once answers are complete and stable.
E1 and E2 are engineering targets for organization policy and a reduced dependency
closure. They do not introduce a paid edition or certify production readiness.

## 32. Non-goals

| Not doing | Why |
|---|---|
| Embeddings or a vector index in the core | Non-deterministic, model-dependent, hides provenance (R2, R8) |
| Calling a model from the engine | Breaks determinism; spends the tokens the tool exists to save |
| A daemon or file watcher | One binary, no runtime; a per-invocation stamp check meets 50 ms |
| VCS in the core | Privacy and dependency reasons; everything needed is a plugin or dctx's own snapshot diff |
| A second configuration language | Two policy engines diverge; DTS already compiles to a sealed IR |
| Telemetry, even opt-out | Trust is the product |
| A hosted index or SaaS | Changes the threat model |
| Parser injection or selection override by plugins | Would make identity and privacy unverifiable |
| Chasing language counts | 13 lexers covering 15 language variants, each with precision labels, beat 23 with silent errors (the comparison tool indexed 8 of 13 planted extra-language files; dctx indexed none and said so); each new language needs a held-out corpus |
| mmap or FST before measurement | Requires a ≥20 % measured gain first |
| In-band agent instructions in answers | Tool output is data; a banner costs tokens on every call and changes with the covered files |

---

# Appendix A — Lineage

| Year | Generation | What it solved |
|---|---|---|
| 2025 | **dircontxt**, pure C (v1.0.0, 15.11.2025; `legacy-c/`) | Give a model the whole project without pasting forty files: one structured `.llmcontext.txt`, a binary state file, version-aware snapshots and diffs, a three-tier ignore hierarchy, global configuration. Zero dependencies. |
| 2026 | **dctx**, Rust | Same privacy promise (ignored files never opened), pack diffs, the template system, the plugin host, map mode for agents, the cache lifecycle, the desktop app |
| Next | **dctx 1.0** (this specification) | One engine and one policy for every workflow, from hand-coding to software factories, measured by prompt-processing efficiency |

# Appendix B — Comparison with Trail-Code

The isolated, reproducible head-to-head is in
[`docs/research/trail-code-2026-10/REPORT.md`](https://www.dctx.internet.technology/docs/benchmark-oct/).
Summary of the design difference, independent of the measurements:

| | dctx | Trail-Code |
|---|---|---|
| Runtime | One Rust binary; std-only core; zero third-party crates in the tagged 0.25.0 `dctx-core`/CLI; 46 in the development line (§27) | Node.js package with an npm dependency tree (405.6 MB) |
| Network | None in the core; no telemetry; 0 sockets in every audited run | Telemetry on by default (opt-out); daily update check not gated by the opt-out; writes `.gitignore`/`.ignore` on every build |
| Agent interface | CLI verbs today; MCP and hooks are surfaces (§24) | MCP server, hooks, statusline |
| Call precision | Lexers and resolver landed; edges not emitted by the shipped CLI (P0); `typed` rung planned (§7.1) | Scope-aware resolution with receiver typing for several languages; optional language servers |
| Measured | §31.0 | §31.0 |
| Organization control | Signed seven-layer policy (§15–16) | Not specified |
| Team memory | Plain-file learnings, policy-governed (§26) | Learnings shared through git |
| Output contract | Bounded, deterministic, precision-labeled pages | Token-budgeted map; signature views |

# Appendix C — Reserved names

Id prefixes `m:`, `s:`, `d:`, `k:`, `p:`, `@`; DTS section kinds 16–31; plugin
namespace `dctx/`; file names `.dctx/`, `.dctxignore`,
`.dctxpol`; environment variables `DCTX_*`.

# Appendix D — Document map

| Topic | Document |
|---|---|
| Accepted architecture decisions | `docs/design/ARCHITECTURE-DECISIONS.md` |
| Product contracts C01–C11, properties P1–P8 | `docs/design/PRODUCT-CONTRACTS.md` |
| Versioned contracts | `docs/design/contracts/` |
| Plugin system | `docs/design/PLUGINS.md`, `docs/design/plugin-api/` |
| Map-mode user guide | `docs/guides/MAP-MODE.md` |
| Crate inventory | `docs/CRATE-MAP.md` |
| Formal models | `docs/formal/` |
| Head-to-head benchmark and raw data | `docs/research/trail-code-2026-10/` |
| Original map-mode design (historical) | `docs/SPEC-MAP-MODE.md` |
