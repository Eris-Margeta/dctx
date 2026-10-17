# dctx-map-parse

`Parser::parse(path, bytes)` turns one authorized UTF-8 file into deterministic
`ParsedFile` facts. `PureParser` is the default. It sends Rust (`.rs`) to
`dctx-lex-rust`, JavaScript (`.js`, `.mjs`, `.cjs`, `.jsx`) and TypeScript
(`.ts`, `.mts`, `.cts`, `.tsx`) to `dctx-lex-js`, Markdown (`.md`,
`.markdown`) to `dctx-lex-md`, Python (`.py`, `.pyi`) to `dctx-lex-py`, Go
(`.go`) to `dctx-lex-go`, and Java (`.java`) to `dctx-lex-java`,
case-insensitively. Other extensions produce a module with a first-comment
summary and an opaque precision label. The caller performs scanning and file IO.

Each `ParsedFile` carries a precision label: coverage, completion and parser
identity. That label is not edge confidence. Complete Rust is `syntax`.
JavaScript, TypeScript, Markdown, Python, Go and Java are `structural`.
Unknown files are `opaque`. A partial or diagnostic extraction is
`ParseError::Syntax`, so an indexer can mask and retry instead of storing
line-scan facts as a complete parse.

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

The default build depends on `dctx-map-model`, `dctx-lex-rust` (and its `syn`
closure), `dctx-lex-js`, `dctx-lex-md`, `dctx-lex-py`, `dctx-lex-go`,
`dctx-lex-java`, `sha2` and `thiserror`. Those lexer crates are path
dependencies. The crate adds no external dependency for Python, Go or Java.
It does not depend on tree-sitter or any C code. `syntax-tree-sitter` (default
off) opts into `tree-sitter`, `tree-sitter-rust`, `tree-sitter-md`,
`tree-sitter-javascript` and `tree-sitter-typescript` and compiles
`TreeSitterParser` as the differential oracle. That feature has no grammar for
Python, Go or Java, so those languages fall back to the pure adapters and keep
their structural precision. Without that feature the name `TreeSitterParser`
is `PureParser`, so existing callers keep compiling.

Validation uses the packet's remote `cargo test -p dctx-map-parse` and
`cargo clippy -p dctx-map-parse -- -D warnings` commands. Fixtures specify exact
node/reference sets and a heading tree; property tests cover identity stability
and collision-free duplicate endpoints.

JavaScript and TypeScript use the first-party `dctx-lex-js` extractor.
TSX is its own dialect. Declarations, classes, methods, named
arrows and function expressions receive qualified identities. JSDoc contributes
the first nonempty description line's first sentence. Invalid syntax refuses the
whole file. Rust and Markdown extraction is unchanged.

ES module targets use `module::name`, `module::default`, or `module::*`;
namespace bindings, side-effect imports and whole-module CommonJS bindings
retain the module string alone. Namespace aliases bind the module, never a glob.
Named bindings retain explicit aliases. Export clauses emit `reexports`;
exported declarations have `public` set. Literal `require` calls emit imports,
including destructured aliases. Dynamic calls remain ordinary calls.

Syntactic `test`, `describe` and `it` calls with string titles become test nodes,
including `only`, `skip`, `todo` and `concurrent` modifiers. Nested titles are
qualified by their enclosing test. Calls in test callbacks emit both `calls`
and `tests`; registration calls themselves do not. Framework alias resolution,
escaped string decoding, computed test titles, module execution, and semantic
type inference remain outside this syntax adapter.

Python, Go and Java use the first-party `dctx-lex-py`, `dctx-lex-go` and
`dctx-lex-java` extractors. `.py` and `.pyi` are both `Language::Python`.
Declarations, imports, calls and type uses are copied from the lexer, including
containment edges. A Go `package` clause is stored as a file-owned symbol: the
indexer treats `NodeKind::Package` as a structural node and would insert it a
second time. A partial completion or any diagnostic refuses the file with
`ParseError::Syntax`. The tree-sitter oracle does not parse these languages
itself; with `syntax-tree-sitter` enabled it calls the same pure adapters.
