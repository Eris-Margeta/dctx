# dctx-lex-go

Hand-written Go structural extractor for dctx map mode (decision 077). Pure Rust:
`dctx-map-model` and `dctx-hash` only, no `unsafe`, no tree-sitter, no git, no mmap.

`GoLexer::extract` returns a `LexedFile` in the same shape as `dctx-lex-js`: a
module node, labeled symbols and containment edges, unresolved references, a
precision label on every fact, `PARSER_ID`, `PARSER_REVISION`, and
`body_fingerprint` (whitespace-normalized SHA-256).

Extracted: package clause, imports (grouped, aliased, dot, blank), functions
and methods (receivers, signatures, type parameters), struct, interface and
type declarations, const and var blocks, doc-comment summaries, call sites
whose callee is an identifier or selector chain, and named types in type
position (including selector types such as `pkg.Type`).

Abstains instead of inventing an edge: index expressions versus type-argument
calls (`F[T](x)`), bare field selectors (`obj.Field` with no call), composite
receivers, and inline struct or interface field lists that appear only as a
parameter type. The model has no var symbol kind; a var is a symbol with
`symbol_kind: None` and a `var` signature. A package clause is a per-file
`package` node (`k:<path>`), not a resolved module graph.

Malformed input yields partial facts and a `Diagnostic`. Oversized input,
bracket nesting past `Limits`, and too many facts are typed `LexError`s with
no facts. `{` must stay on the signature's line, matching Go's semicolon rule.
