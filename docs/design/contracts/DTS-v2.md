<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# DTS source v2 and compiled plan IR v1

Status: current versioned design sketch; proposed syntax is not a release promise.
Defines organization control of every mode under the accepted structure.
Depends on API, identity and parser contracts; shared by the facade and mode engines.
Preserves the v1 compiler/rendering contract while extending typed policy.

## Compatibility and typed plan

DTS v1 variables, partials, layouts, four marker definitions, static resolution
and runtime data injection remain valid with byte-identical fixture outputs.
A version declaration selects v2; never infer version by an unknown field.
Legacy defaults compile to an explicit pack plan. Every map/future mode request
also receives a compiled default/custom plan; there is no second hidden policy
language or engine path bypassing DTS.

```text
ContextPlanV1 = {
  source_language_version, ir_major:1, ir_minor:0,
  required_capabilities, selection_rules, extraction_rules,
  budget_rules, views, render_program,
  selection_digest, extraction_digest, render_digest, plan_digest
}
```

| Typed section | Responsibility | Validation |
|---|---|---|
| SELECT | Paths/languages/kinds and Include/Exclude/OmitContent/DebugVisible policy | Root-relative paths, bounded rule count, no grant expansion |
| INDEX | Requested facts, parser/dialect, extraction limits | Available parser capability; Rust default `syn`; optional native only explicit |
| BUDGET | Final bytes/estimated tokens, records/depth, parse/memory/work bounds | Nonnegative bounded quantities; intersect host/mode ceilings; overflow refused |
| VIEW | Pack/map/future projection naming selection/extraction/budget/render refs | All references resolved, acyclic, mode capability present |
| RENDER | Existing partials/layout/markers plus bounded record iteration | No arbitrary evaluation, filesystem/network/process access or loaded plugins |

Source examples in proposal §2 are illustrative; B2 freezes precise grammar
and source fixtures before parser implementation. The typed semantics above
are normative. Custom source is built with explicit `template build`; shipped
compiled defaults need no runtime source parse. Template *sources* remain
user-owned; compiled plans are reconstructible cache entries.

## Permission and budgets

`permission = host_grant ∩ organization_ceiling ∩ session_selection`.
Within that ceiling, preserve existing ordered last-match ignore/omit semantics.
A template can narrow access but never turn a host denial into Include.
DebugVisible authorizes metadata only. Read rule/config sources through explicit
capabilities; unreadable policy is `PolicyUnavailable`, never default-allow.
Compiled plans bind their policy inputs/revision; changed/removed policy sources
invalidate authorization before cached facts or render output can be served.

Selection/extraction digests change index validity independently. Rendering-only
changes do not reparse source; they still change the plan/render digest and
continuation compatibility. A parser capability/revision change alters FactKey.
Budget/depth narrowing cannot make previously unauthorized facts visible.

Final-output token estimate is `ceil(2 * UTF8_bytes / 7)` over the complete
encoded answer. Rendering reserves headers, delimiters, escaping and continuation
before choosing whole records. Loops traverse only authorized records under
iteration/work bounds. Cyclic partials, unknown variables/opcodes, missing
capabilities and impossible envelopes are typed errors; no fallback rendering
that violates the configured mode or privacy ceiling.

## Compiled IR envelope

IR magic `DCTXIR\0\0` (8 bytes), major/minor u16, exact total length u64,
required capability bitset u64, section_count u32, followed by bounded typed
sections. Each section has `kind:u16, schema:u16, length:u32, payload[length]`.
Trailing SHA-256 covers all preceding bytes. Integer encoding is little-endian;
strings have checked u32 byte lengths and UTF-8. Maximum plan 1 MiB, 64 sections,
10,000 instructions; caller may narrow. Unknown required section/opcode/version
refuses load. No native-memory layout or executable serialized pointers.

Canonical sections order SELECT/INDEX/BUDGET/VIEW/RENDER/provenance, sorted named
definitions with source rule order preserved inside each. Domain-separated
identity framing (identity v1 kind 8) defines plan_digest; render_digest uses
kind 8 with a distinct IR schema/domain payload identifying render-only input.
Selection uses kind 4, extraction kind 5. B2 supplies canonical encoding goldens
and corruption fuzzing before compiled IR is a release format.

## Owner tests and model

B2 owns v1 parity, v2 compile/load determinism, section/opcode rejection and
positive controls; P1 authorization-before-render, P5 final budget, P6 frozen
clock/reordered-input equality. Model compile→validated-plan→authorized-view→
bounded-render transitions and policy-epoch revocation. Mutation removing the
host intersection must fail a denied-content test. Mutation budgeting unescaped
strings must fail a Unicode/JSON envelope property. Unsupported parser tests
include a positive `syn` capability case and explicit optional tree-sitter case.
Future modes must add their typed VIEW capability and conformance fixture to
this same compiler/facade contract before release.
