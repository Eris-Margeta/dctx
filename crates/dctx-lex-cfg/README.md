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
  Object trailing commas are accepted in JSON and JSONC. Array trailing commas
  are JSONC-only. A dot inside a key stays in that key
  (`rust-analyzer.linkedProjects`).
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

Check `completion` and `diagnostics` after every successful extraction. A successful
call can contain partial facts; a refusal returns no facts. Diagnostics use byte
offsets into the supplied input, including a leading UTF-8 BOM. Duplicate keys
keep the first assignment. Correct the reported syntax, close an unfinished
string/comment/collection, or remove the duplicate before extracting again.

`CfgLexer` is stateless and can be shared between threads. The same path, bytes
and limits produce the same facts and ordering. Limits can only narrow the
8 MiB byte ceiling, depth 256 and 250,000 nodes plus references. The file node
uses one fact; containment edges do not use additional facts. To address a limit
refusal, reduce nesting or split the configuration into smaller files.

`body_fingerprint` is a standalone helper without the extraction byte limit. It
collapses all Unicode whitespace, including whitespace inside quoted values;
it does not parse configuration syntax.
