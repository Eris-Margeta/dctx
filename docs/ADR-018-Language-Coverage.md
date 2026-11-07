<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# ADR-018: Language Coverage

**Status:** superseded — historical design and implementation record.

The original 13-family scope below is superseded by the [current language registry table](../README.md#language-tiers); confidence and completion principles remain relevant.

## Context

### The Problem

Map facts need explicit coverage and completion claims across source/configuration languages; precision is separate from edge confidence (`crates/dctx-map-model/src/domain/precision.rs`).

### Root Cause Analysis

Structural extraction is not a full grammar claim, and incomplete extraction must not silently become complete facts (`crates/dctx-map-model/src/domain/precision.rs`, `crates/dctx-map-parse/src/adapters/mod.rs`).

## Decision

Use pure structural extraction for 13 families: Rust, JavaScript/TypeScript, Markdown, Python, Go, Java, C/C++, configuration (JSON/TOML/YAML), C#, Kotlin, Ruby, PHP and Swift (`crates/dctx/src/index/extract.rs`, `docs/CRATE-MAP.md`).

The facade's default profile uses hand-written extraction, including line-oriented Rust and the first-party Markdown extractor. **Qualification:** `dctx-lex-rust` itself uses `syn`; it is optional behind facade `syntax-rust`, so “13 hand-written lexers” is not a claim about every lexer crate or map-parse Rust implementation (`crates/dctx/src/index/extract.rs`, `crates/dctx/Cargo.toml`, `crates/dctx-lex-rust/src/lib.rs`).

Keep one shared facade dispatch and fact-to-symbol conversion in `crates/dctx/src/index/lex_dispatch.rs`; Rust and Markdown retain specialized entry paths in `crates/dctx/src/index/extract.rs`.

Carry coverage (`syntax`, `structural`, `opaque`), completion (`complete`, `partial`, `refused`) and bounded parser identity separately from confidence (`crates/dctx-map-model/src/domain/precision.rs`).

## Implementation

### Phase 1: Pure Lexers and Facade Projection

The shared dispatch converts nodes without reading source or retaining bodies. New language branches expose only complete extraction; JavaScript/TypeScript preserve their existing partial-fact projection (`crates/dctx/src/index/lex_dispatch.rs`).

The facade rejects NUL/non-UTF-8 symbol extraction and assigns stable ids and containment after extraction (`crates/dctx/src/index/extract.rs`).

### Phase 2: Map-parse Adapters

`PureParser` dispatches the language adapters over caller-provided bytes; partial/diagnostic extraction is refused as `ParseError::Syntax`, and unknown languages have a module-only fallback (`crates/dctx-map-parse/src/adapters/mod.rs`).

Shared fact assembly records file precision and finalizes source-ordered facts with duplicate-id handling (`crates/dctx-map-parse/src/adapters/facts.rs`).

### Phase 3: Store Invariants Gate

The parser gate checks file ownership, unique/kind-correct ids, source-bounded spans, valid confidence/kind combinations, existing edge endpoints and containment reachability. It also simulates query insertion of Place/Package nodes to catch duplicate ids (`crates/dctx-map-parse/tests/lexers.rs`, `crates/dctx-map-parse/tests/fuzz_all.rs`).

### Phase 4: Fuzz Gate

Seeded bounded property tests exercise arbitrary bytes, Unicode, malformed inputs, deterministic results, successful-result store invariants and fixture prefixes without executing source (`crates/dctx-map-parse/tests/fuzz_all.rs`).

## Results

The gate source defines successful fixture controls as well as refusal/robustness cases; this record asserts test obligations, not a new passing execution (`crates/dctx-map-parse/tests/lexers.rs`, `crates/dctx-map-parse/tests/fuzz_all.rs`).

## Trade-offs

### Benefits

Structural facts can be produced without a default tree-sitter runtime, with explicit precision and conservative abstention (`crates/dctx-map-parse/src/adapters/mod.rs`, `crates/dctx/src/index/lex_dispatch.rs`).

### Costs

The facade's structural Rust profile and optional syn path differ; the shared facade dispatch also deliberately differs from PureParser on JavaScript partial facts (`crates/dctx/src/index/extract.rs`, `crates/dctx/src/index/lex_dispatch.rs`, `crates/dctx-map-parse/src/adapters/mod.rs`).

## Alternatives Considered

The earlier spec proposed tree-sitter as the parser dependency. Current adapters retain it only behind `syntax-tree-sitter` as an optional oracle; languages without an included grammar use pure adapters even in that oracle (`docs/SPEC-MAP-MODE.md`, `crates/dctx-map-parse/src/adapters/mod.rs`).

## References

Inventory: `docs/CRATE-MAP.md`; facade: `crates/dctx/src/index/lex_dispatch.rs`; adapters: `crates/dctx-map-parse/src/adapters/mod.rs`; robustness: `crates/dctx-map-parse/tests/fuzz_all.rs`.

## Lessons Learned

Language family count, structural coverage, grammar completeness and edge confidence are distinct claims; the Rust implementation qualification must remain visible (`docs/CRATE-MAP.md`, `crates/dctx-map-model/src/domain/precision.rs`, `crates/dctx-lex-rust/src/lib.rs`).

## Future Work

Keep tree-sitter optional and preserve precision/refusal and store-invariant gates when extending adapters; optional oracle coverage is limited to its included grammars (`crates/dctx-map-parse/src/adapters/mod.rs`, `crates/dctx-map-parse/tests/fuzz_all.rs`).
