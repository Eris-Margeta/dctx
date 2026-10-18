# dctx-lex-cfg

Hand-written configuration structural extractor for dctx map mode (decision 077).
Pure Rust: `dctx-map-model` and `dctx-hash` only, no `unsafe`, no tree-sitter,
no git, no mmap.

`CfgLexer::extract` returns a `LexedFile` in the same shape as `dctx-lex-go`:
a module node, one symbol per table or key (dotted path, for example
`package.version`), containment edges, unresolved references, a precision
label on every fact, `PARSER_ID`, `PARSER_REVISION`, and `body_fingerprint`
(whitespace-normalized SHA-256). The first line of a leading or trailing
comment is the summary.

Extracted:

- JSON objects and arrays. `.jsonc` accepts `//` and `/* */` comments.
  Trailing commas are accepted in JSON and JSONC. A dot inside a key stays in
  that key (`rust-analyzer.linkedProjects`).
- TOML tables, arrays of tables, dotted keys and inline tables.
- YAML block mappings and sequences, flow collections, multi-document files.
  Anchors and aliases are recorded on the signature and are not expanded.

References (precision `convention`) are emitted only when the string is obvious:
a dependency key under a dependencies table, a workspace member, or a relative
path (`./`, `../`, or a `path` value containing `/`). Anything else is abstained.
No edge is invented; the only edges are `contains`.

Keys containing `#`, `~` or a control character are abstained. Malformed
input yields partial facts and a `Diagnostic`. Oversized input, nesting past
`Limits`, and too many facts are typed `LexError`s with no facts.
