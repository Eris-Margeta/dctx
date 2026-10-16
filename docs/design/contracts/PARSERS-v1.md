# Parser and precision contract v1

DX-A0b · sketch version 1.0 · applies decision 077 to DX-ARCH §3.
Defines bounded default extraction and independent edge-confidence labels.
Depends on identity v1 and PRODUCT-CONTRACTS C03/C05; used by B4/B5/A1/E6.
Implementation and measured corpus evidence remain future work.

## Common interface and provenance

```text
ParseInput = { authorized_bytes, ObjectKey, language, dialect,
               ExtractionPlan, Limits, Cancellation }
ParseResult = { file_local_facts, unresolved_references,
                parser_id, parser_revision, extraction_digest,
                coverage, unsupported_spans, diagnostics, completion }
completion = complete | partial(reason) | refused(typed_error)
coverage = syntax | structural | opaque
```

Coverage describes extraction, not type checking. Each fact has a byte span
`[start,end)` inside the input; line numbers are 1-based inclusive projections.
Spans must land on valid UTF-8 boundaries for text output. No out-of-range span,
panic, unbounded recursion or allocation from unchecked lengths. No filesystem,
environment, package execution, macro expansion, subprocess or network in a
parser. Parse trees are released per file; only bounded facts survive.

Capabilities specify dialect/edition, parser revision, supported constructs,
malformed-input behavior, max file bytes, depth, records and cancellation
granularity. Caller limits can only narrow these ceilings. Refuse oversized
input before allocating an AST. `syn`'s synchronous parse cannot promise prompt
mid-call cancellation: check before/after and bound accepted input; callers
requiring a hard deadline select explicit process isolation or receive
`UnsupportedCapability`. A worker thread is not killable isolation.

## Language contracts

| Language | decision 077 default and coverage | Extracted facts | Uncertain/unsupported cases and required tests |
|---|---|---|---|
| Rust | `syn` adapter, `syntax` for supported valid Rust; declared Rust edition and span capability | Items, modules, functions/methods, structs/enums/traits/impls, signatures, use/reexports, doc comments, test attributes, raw type/call references | No proc/declarative macro expansion, type inference, cfg evaluation without explicit environment, or dynamic dispatch proof. Malformed syntax returns partial opaque module + diagnostics; no silent tree-sitter switch. Corpus: raw strings, Unicode, nested generics, lifetimes, macros, cfg, async, impls, incomplete edits |
| JavaScript | First-party bounded lexer, `structural`; `.js`/`.mjs`/`.cjs` dialect capability | Recognizable function/class declarations, literal static import/export, JSDoc, conservative identifier/member call sites | Strings/comments/templates/regex must not create fake declarations. Regex/division ambiguity, computed/dynamic imports, eval, property dispatch and unsupported JSX yield unsupported spans/unresolved refs. Corpus: interpolation, escapes, minified code, ASI, regex, destructuring, incomplete braces |
| TypeScript | First-party JS lexer extended with TS tokens, `structural`; TSX explicitly advertised | JS subset plus supported interface/type/enum/namespace declarations, type-only imports/exports, conservative signature references | No type checking, overload selection, module augmentation or decorator execution. Generic/JSX ambiguity and unsupported advanced types produce partial/unknown scopes, not guessed exact facts. Corpus: nested generics, conditional/mapped types, overloads, decorators, JSX/TSX, ambient declarations |
| Markdown | First-party line/block lexer, `structural` | ATX/setext headings, section spans, fenced-block masking, first sentence summaries, backticked name/path references | No heading extraction inside fences/indented code; nested lists/HTML/extensions unsupported where not declared. Never parse fenced source as live modules implicitly. Corpus: fence lengths/indent, escaped hashes, duplicate/Unicode headings, CRLF, code blocks, incomplete fences |
| Other/unsupported dialect | `opaque` module record only | Permitted path/metadata and bounded first-comment summary only if content read is allowed and recognizer capability exists | Unknown language is not silently parsed as JS. Binary content yields explicit omitted/binary classification, no fabricated syntax |

`syn` exact version/features/span-mapping dependencies require A3/B4 review.
If reliable byte spans are unavailable, fail the requested span capability;
never approximate spans used by `read`. Default Rust cannot be substituted with
a hand-written Rust lexer to claim zero dependencies. A minimal port-only build
may report Rust syntax unsupported; the shipped default must include `syn`.

Tree-sitter plus grammars is an explicit `syntax-tree-sitter` feature and parser
selection. It has C/FFI dependencies, distinct FactKeys and declared grammar
versions. Its useful syntax recovery does not establish semantic exactness.
Unavailable requested parser returns `UnsupportedCapability`; never silently
spawn a CLI, use the network or select a native implementation.

## Confidence is separate from parser coverage

| Relation/evidence | Allowed label |
|---|---|
| Syntactically proven containment with complete local spans | `exact` |
| Import/reexport target uniquely established in the declared environment | `exact`, with resolver evidence; otherwise `name` or unresolved |
| Name/scope/import matched call or type reference | `name`; syntax alone is insufficient for semantic `exact` |
| Bare name heuristic across candidates, explicitly requested | `guess`; retain alternatives and reason |
| Ambiguous/dynamic/macro-generated target | `unresolved(name, reason)`; do not invent an edge |
| Validated optional SCIP evidence matching source/environment versions | May upgrade to `exact`; preserve source/provenance and invalidation rules |

Answers include parser coverage, completion, precision provenance and unresolved
counts for their queried scope. Mixed-parser results report per-record overrides
or a capability set, never a single inflated global precision label. `partial`
is completion status layered over coverage; `syntax + partial` is valid.
Stale summaries/edges cannot hide behind a precise parser label.

## Release tests and quantitative gates

For each language/dialect and fact family (declarations, imports, calls, doc
spans), publish `precision = correct_emitted / emitted` and
`recall = correct_emitted / expected` on a pinned, held-out, adjudicated corpus.
Zero-denominator metrics are `not_applicable`, not 100%. Abstentions reduce
recall. Record valid/incomplete/generated/minified/adversarial subsets separately.

Stage-0 targets inherited from the proposal, not measured results:

| Parser profile | Declaration gate |
|---|---|
| Default Rust `syn`, supported valid Rust | ≥99% precision and ≥99% recall |
| Default JS/TS/Markdown structural lexers | ≥98% precision and ≥85% recall per language, not pooled |
| Optional tree-sitter mixed valid/incomplete fixtures | ≥99% precision and ≥95% recall |

Calls/imports/docs have independent reported metrics; declaration thresholds
cannot stand in for them. Every emitted `exact` edge must have no known false
positive in the adjudicated corpus. Quality must preserve the five-verb recall
oracle (SPEC target recall@10 ≥80% over ≥50 questions) and disclose unsupported
scopes. General grammar DSL remains deferred pending a bounded prototype.

B4 owns corpus goldens, deterministic chunk/order properties, malformed-input
fuzzing, limits/cancellation refusal and positive controls. B5 owns cross-root
resolution and precision upgrade/invalidation tests. A1/E6 measure actual
release size, throughput and peak RSS with exact parser/compiler/features;
proposal size/rate envelopes remain hypotheses. Input bytes, fact counts,
worker count and AST limits must fit C07 reservations and operation memory
budgets. Keep elapsed-time performance thresholds out of normal unit suites.
