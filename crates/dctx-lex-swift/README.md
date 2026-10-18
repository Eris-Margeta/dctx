# dctx-lex-swift

First-party Swift structural extractor (Apache-2.0). Pure Rust, no IO, no unsafe,
and only path dependencies on dctx-map-model and dctx-hash.

`SwiftLexer::default().extract("Sources/App.swift", bytes)` returns labeled nodes,
file-owned containment edges, unresolved import/call/member references, and bounded
diagnostics. Byte, nesting and total fact ceilings produce typed `LexError`s.
Malformed UTF-8 is refused; malformed Swift retains partial facts.

Recognizes imports, extensions, classes, structs, enums and cases, protocols,
actors, aliases, properties, functions, initializers, subscripts and operator
declarations. Signatures retain attributes, access modifiers, generic parameters
and where clauses. Actors map to class, protocols to interface, extensions to impl,
properties/cases to const or an unspecified symbol kind in the shared vocabulary.

Nested comments, regular/multiline/raw strings and nested interpolation are lexed.
Literal text never produces declarations or references. Compiler condition tokens
remain unevaluated: declarations in every branch are retained. Macros produce a
diagnostic and their argument/body tokens are skipped. No dispatch or macro edge
is resolved. Member reads are convention-labeled unresolved uses-type hints because
the shared model has no member-read relation. Closure bodies contribute lexical
references to their enclosing declaration; local bindings are not graph symbols.

Precision describes syntactic evidence, not semantic resolution. Partial spans
are labeled Partial; unsupported syntax receives diagnostics. This is a structural
extractor, not a Swift type checker or complete grammar validator. Duplicate names
receive deterministic source-order ordinals; unique IDs ignore line positions.
Body fingerprints use whitespace-normalized SHA-256 from dctx-hash.
