# dctx-lex-php

Hand-written PHP structural extraction for map mode (decision 077, PARSERS-v1).
Apache-2.0, pure safe Rust, with only the path dependencies `dctx-map-model`
and `dctx-hash`. It performs no IO and invokes no external programs.

```rust
use dctx_lex_php::PhpLexer;
let facts = PhpLexer::default().extract("src/service.php", b"<?php function run() {}")?;
# Ok::<(), dctx_lex_php::LexError>(())
```

`PhpLexer` accepts `.php`, `.phtml` and `.inc` case-insensitively. Only code
inside `<?php` or `<?=` regions is scanned; HTML outside them is opaque.
The closing `?>` also terminates line comments and acts as a statement boundary.
Strings, interpolation, backticks, heredocs, nowdocs and comments are opaque.
Attributes retain their declaration signatures and supply type references.

The output has the same `LexedFile`, `Labeled`, `Completion`, `Precision`,
`Limits` and typed `LexError` shape as the Go/Java/JavaScript extractors.
Namespaces are unclassified symbols. Classes, interfaces, traits, enums,
constants, enum cases, functions and methods use the existing model kinds;
properties (including constructor promotion) are unclassified symbols.
Names use PHP namespace backslashes and lexical `::` qualification.
PHPDoc supplies the first summary sentence; otherwise the signature is used.
Visibility is retained in the signature and mapped to the model's `public` flag.

Only containment produces resolved edges. `use` (including groups, aliases,
function and const imports), literal require/include, inheritance, trait use,
type names, direct calls and static members produce unresolved references.
`extends` uses `UsesType` (`Implements` for an interface extending interfaces).
`$this->fixed` sites are convention references without any inferred receiver
type. Variable receivers, variable callees, variable variables, dynamic members,
`eval` and computed include paths never produce guessed destinations.
First-class callable syntax is omitted from call references.
Import function/const categories remain in source; the shared reference model
has no separate category field. Escaped include paths are conservatively omitted.

`Structural` means supported token structure, not full PHP validation or name
resolution. Unsupported or malformed declarations and unterminated lexical
constructs yield diagnostics and partial facts. Anonymous classes are marked
unsupported; their named identities are not invented. Heredoc contents are
opaque even when interpolated. Trait adaptations are skipped after recording
the trait names. This extractor does not execute or validate PHP programs.

Limits can only narrow the ceilings: 8 MiB input, 256 delimiter/interpolation
levels and 250,000 nodes plus references. Exceeding a limit, invalid UTF-8,
unsupported extensions and invalid file identities are typed refusals.
At most 64 diagnostics are retained. Every fact has a precision label.
All node IDs and paths belong to the input file; duplicate names receive
deterministic source-order ordinals. IDs do not contain source line numbers.
`body_fingerprint` uses whitespace-normalized SHA-256 from `dctx-hash`.
`PARSER_ID` and `PARSER_REVISION` provide parser provenance.

The integration suite pins golden facts and checks store ownership, unique and
stable IDs, containment and file-bounded spans for every fixture prefix. It
also exercises arbitrary bytes, PHP token noise and typed refusals without
external fuzz or test dependencies. Workspace parser wiring is a later packet.
