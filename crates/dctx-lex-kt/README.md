# dctx-lex-kt

Hand-written Kotlin structural extractor for dctx map mode (decision 077). Pure Rust:
`dctx-map-model` and `dctx-hash` only, no `unsafe`, no tree-sitter, no git, no mmap.

`KotlinLexer::extract` returns a `LexedFile` for `.kt` and `.kts` (case-insensitive).
Every node, containment edge and reference carries a precision label. `PARSER_ID` is
`dctx-lex-kt` and `PARSER_REVISION` is `1`. `body_fingerprint` is the SHA-256 hex of
the body with whitespace runs collapsed and the edges trimmed.

## Extracted

- Package name, stored as the module signature (not a separate package node). A leading
  KDoc comment is the module summary; otherwise the summary is the package name.
- Imports, including `*` wildcards and `as` aliases.
- Classes, interfaces, objects, enum classes, data classes and sealed types, with modifiers.
  Data classes are structs; interfaces and annotation classes are interfaces; enum classes
  are enums; other types and objects are classes.
- Primary constructors when a parameter list is present, named after the type. Properties
  declared with `val` or `var` in that list belong to the type. Secondary `constructor`
  declarations use the same local name.
- Properties (`val` and `var`, including enum constants) and functions. File-level functions
  are functions; members are methods. Signatures stop before the body.
- Type aliases, KDoc summaries (first description sentence, tag lines skipped), call sites
  and member calls (`receiver.name`, including `?.`), plus named types in type position.

## Abstains

Rather than invent an edge: bare member reads (`item.length` with no call), `::` callable
references, local variables, lambda and anonymous-object symbols, overload resolution, and
`implements` versus `extends`. A supertype is a uses-type, and a call only when a
constructor argument list follows. Script statements that are not declarations contribute
calls and no symbol.

## Diagnostics and refusals

Malformed or incomplete input returns the facts understood so far plus a `Diagnostic`
(unterminated comment, string, raw string or character; unbalanced bracket; a name that
cannot form a stable id; a declaration that lost its shape). Facts whose end byte is past
the first diagnostic are labeled partial. The module span stays structural.

A non-Kotlin path, invalid UTF-8, a path that cannot be a stable id, or input past the
byte, depth or fact ceiling is a typed `LexError` with no facts. Limits only narrow
`Limits::CEILING` (8 MiB, depth 256, 250_000 nodes plus references).

## Tests

```
cargo test -p dctx-lex-kt
cargo clippy -p dctx-lex-kt -- -D warnings
```
