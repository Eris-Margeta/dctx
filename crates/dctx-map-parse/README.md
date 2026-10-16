# dctx-map-parse

`Parser::parse(path, bytes)` turns one authorized UTF-8 file into deterministic
`ParsedFile` facts. `TreeSitterParser` supports Rust (`.rs`) and Markdown (`.md`,
`.markdown`), case-insensitively. Other extensions produce a module with a
first-comment summary. The caller performs scanning and file IO.

Nodes and exact `contains` edges use the public `dctx-map-model` types.
`RawReference` retains the originating node, relation, lexical scope, alias,
one-based inclusive location, and owner path. A resolver supplies destinations
and confidence; this crate never guesses them. Facts follow source traversal
order, and repeated reference sites remain separate.

Rust includes functions, methods, structs/unions, enums, traits, impl blocks,
constants/statics, aliases/associated types, declarative macros, inline modules,
and test functions. Inline modules are symbols with no `symbol_kind`: the
model's module IDs represent source files and cannot carry qualified names.
Methods are named `Type::method`; impls are `Type::impl` or `Type::impl<Trait>`.
Repeated names receive `~1`, `~2`, etc. after extraction, including references
and containment endpoints. Macro expansion and semantic type inference are
outside this syntax layer. Invalid Rust produces a typed error, not partial facts.

Imports include nested lists, `self`, aliases, and globs. Public imports emit
both `imports` and `reexports`. Calls preserve qualified paths or receiver names;
test calls additionally emit `tests`. Signatures and fields contribute
`uses-type` references. `#[test]` and qualified `::test` attributes mark tests.
`cfg` conditions are not evaluated: all syntactically present items are indexed.

Markdown supports ATX and Setext headings, level-based containment, duplicate
slugs, inline code names, and inline link destinations. Fenced/indented code
and HTML blocks do not create headings or references. A section ends before
the next heading at its level or above. Text before the first heading belongs
only to the file module. Headings use Unicode lowercase slugs and model aliases.

Fingerprints are SHA-256 of bodies with whitespace runs collapsed. Function
bodies exclude their declarations so a rename retains its fingerprint. This
is textual normalization, not token or semantic equivalence.

Integration boundary: the scanner's public `ScannedFile` remains independent
of graph types. The indexer must honor its content permission before calling
the parser. The store's current `Unresolved` record has only name and relation;
it cannot persist the parser's source, scope, alias, or location. Resolution
should consume `RawReference` before adapting results into store `FileFacts`.

Validation uses the packet's remote `cargo test -p dctx-map-parse` and
`cargo clippy -p dctx-map-parse -- -D warnings` commands. Fixtures specify exact
node/reference sets and a heading tree; property tests cover identity stability
and collision-free duplicate endpoints.
