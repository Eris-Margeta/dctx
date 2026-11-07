<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Lexer provenance

Where the language knowledge in dctx map mode comes from: whether any keyword table or grammar was copied or derived from tree-sitter or another project. This provenance record supports license review.

## Method

Searched the sources and tests of every `crates/dctx-lex-*` crate, `crates/dctx-map-parse` and the generic outline families in `crates/dctx/src/index/generic/` (plus `crates/dctx/src/index/out_calls.rs`, which holds the call-keyword list) for:

- `tree-sitter`, `treesitter`, `TreeSitter`;
- URLs (`http://`, `https://`), other than test inputs such as `https://x.y`;
- `derived from`, `copied from`, `ported from`, `adapted from`, `based on`, `grammar`, `textmate`, `tmLanguage`, `pygments`, `ctags`, `linguist`;
- `copyright`, `license`, `SPDX` in test fixtures;
- grammar files (`*.scm`, `*.tmLanguage*`, `grammar.js`).

The search finds references and copied headers; it cannot prove that no table was retyped from elsewhere without attribution.

## Result per crate

| Crate or family | Source of grammar and keyword tables | Notes |
|---|---|---|
| dctx-lex-c | hand-written, no third-party grammar found by grep | README: "Hand-written C and C++ structural extractor"; crate docs: never depends on tree-sitter. |
| dctx-lex-cfg | hand-written, no third-party grammar found by grep | JSON, TOML and YAML readers; README: hand-written. |
| dctx-lex-cs | hand-written, no third-party grammar found by grep | Crate docs: never depends on tree-sitter. |
| dctx-lex-go | hand-written, no third-party grammar found by grep | README: hand-written. |
| dctx-lex-java | hand-written, no third-party grammar found by grep | Cites JLS 3.3 (unicode escapes) as the behaviour it follows; no copied text. |
| dctx-lex-js | hand-written, no third-party grammar found by grep | JavaScript and TypeScript. Tests compare against the tree-sitter oracle (see below). |
| dctx-lex-kt | hand-written, no third-party grammar found by grep | README: hand-written. |
| dctx-lex-md | hand-written, no third-party grammar found by grep | Tests use CommonMark 0.31 spec examples 62–100 as inputs (test-only, not shipped; the CommonMark spec is CC-BY-SA-4.0). Tests compare against the tree-sitter oracle. |
| dctx-lex-php | hand-written, no third-party grammar found by grep | README: hand-written. |
| dctx-lex-py | hand-written, no third-party grammar found by grep | Cites PEP 701 (f-strings) as the behaviour it follows; no copied text. |
| dctx-lex-rb | hand-written, no third-party grammar found by grep | README: hand-written. |
| dctx-lex-rust | the `syn` parser crate (dependency) | Parses with `syn`, `proc-macro2` and `quote` (dependencies, listed in THIRD_PARTY.md); no grammar copied into the crate. Tests compare against the tree-sitter oracle. |
| dctx-lex-swift | hand-written, no third-party grammar found by grep | README: "First-party Swift structural extractor". |
| dctx-map-parse adapters | no grammar of their own | Convert dctx-lex-* facts. The optional `syntax-tree-sitter` feature compiles a tree-sitter walker (below). |
| Generic outline families (`crates/dctx/src/index/generic/`: brace, block — Elixir, Erlang, Haskell, Julia, Lua, OCaml, R —, lisp incl. Nix, infra, web) and `out_calls.rs` | hand-written, no third-party grammar found by grep | Heuristic symbol extraction; every symbol is labelled `guess`. |

## tree-sitter

tree-sitter is used only as a differential test oracle. `crates/dctx-map-parse` declares `tree-sitter`, `tree-sitter-rust`, `tree-sitter-md`, `tree-sitter-javascript` and `tree-sitter-typescript` as optional dependencies behind the `syntax-tree-sitter` feature, which the default build does not enable ("No C in the default build", `crates/dctx-map-parse/src/lib.rs`). No workspace crate enables that feature; without it `TreeSitterParser` is an alias of `PureParser`. The differential tests of dctx-lex-js, dctx-lex-md and dctx-lex-rust use the real oracle only when the feature is turned on explicitly (`cargo test -p dctx-map-parse --features syntax-tree-sitter`). No tree-sitter grammar, query file or node table is copied into a dctx lexer; the Rust, Markdown and JavaScript/TypeScript adapters walk tree-sitter node kinds only inside that feature.

Because Cargo.lock keeps optional dependencies, the best-effort THIRD_PARTY.md lists the tree-sitter crates; the runner regeneration from `cargo metadata` records their licences.
