# dctx-lex-cs

`CsLexer::extract(path, bytes)` turns one authorized `.cs` file into
deterministic `LexedFile` facts. The extension match is case-insensitive.
Other extensions are `UnsupportedDialect`. The caller performs scanning and
file IO; this crate does not read the disk, the environment, or git.

`PARSER_ID` is `dctx-lex-cs` and `PARSER_REVISION` is `3`. Every node
carries a `body_fingerprint`: SHA-256 of the body with whitespace runs
collapsed, from `dctx-hash`. A declaration with braces fingerprints that
brace span. A field with `=` fingerprints the initializer. Anything else
fingerprints its signature. The module fingerprints the whole file.

Nodes and exact `contains` edges use the public `dctx-map-model` types.
`RawReference` keeps the originating node, relation, lexical scope, an
optional alias, one-based inclusive lines, and the owner path. Repeated
sites stay separate records. Nothing in this crate resolves a destination.

Extracted facts:

- The first namespace name is the module signature (`App.Sample`), not a
  package node. A leading `///` or `/** */` summary supplies the module
  summary (`doc`); otherwise the summary source is `sig`.
- `using` directives, including `global using`, `using static`, and
  `using Alias = Target`. `using static` stores alias `static`. An alias
  directive stores the alias identifier. `using var` and `using (...)` are
  statements, not imports.
- Namespaces (block and file-scoped) are symbols with no symbol kind.
  Classes, structs, interfaces, enums, and records carry their modifiers.
  Records, including `record struct`, are `struct`. The signature keeps the
  keyword. Fields, properties, and enum constants are `const`. Methods and
  constructors are `method`. Record positional components are `const` and
  not public; accessors are not invented. Class primary-constructor
  parameters are not promoted to fields.
- Constructors use the type's simple name (`Box::Box`). Nested names use
  `::` and keep dots inside namespace names (`App.Sample::Box::Run`).
  Duplicate names get `~1`, `~2`, only on the stable id.
- Signatures run from the first declaration token (attributes included)
  through the name (fields, properties, enum constants) or through `)` ,
  a constructor initializer, and `where` clauses (methods). XML doc text
  is the summary and is not part of the signature. The summary is the
  `<summary>` sentence, up to `". "`, capped at 100 characters.
- Attributes, base types, constraints, field and method types, parameters,
  and record components emit `uses-type`. The first base of a class or
  record class is `uses-type`; further bases, and every base of an
  interface, struct, or record struct, are `implements`. An enum's integral
  primitive is `uses-type`; any other enum base is `implements`. Type
  arguments are `uses-type`. Declared type-parameter names are not.
- Calls are bare names and identifier `.` or `?.` chains (`service.Run`).
  `this(` and `base(` are calls. `new Type(...)` is `new Type`. `new(` is
  `new`. Calls inside interpolation holes are included. Preprocessor
  directives (`#if`, `#region`, and the rest) are tokens and are never
  evaluated, so both branches stay in the token stream.

Every fact has a precision label: `Structural`, `Convention`, or `Partial`.
A fact whose end byte is past the first diagnostic is `Partial`. The module
stays at its own precision. Incomplete or malformed files return `Ok` with
`Completion::Partial` and a diagnostic (`UnterminatedComment`,
`UnterminatedString`, `UnterminatedChar`, `UnbalancedBracket`,
`UnsupportedName`, `Malformed`). They do not panic. At most 64 diagnostics
are retained; overflow still marks the file partial.

Refusals return no facts: `UnsupportedDialect`, `InvalidId`, `InvalidUtf8`,
`TooLarge`, `TooDeep`, `TooManyFacts`. Caller limits only narrow the ceilings
(8 MiB, depth 256, 250_000 facts). `>` is always one token, so `>>` closes
two type arguments.

Abstentions, rather than invented edges: bare member reads (`point.X`),
destructors, operators, indexers, local functions, calls inside attribute
argument lists, and lambda expressions as symbols. Lambda bodies are still
scanned for calls. Multi-dollar interpolation holes are lexed; the common
single-`$` hole is tokenized so calls inside it are extracted.

The crate depends only on `dctx-map-model` and `dctx-hash`. It is pure Rust,
Apache-2.0, with no `unsafe` and no new external dependency.

Validation uses the packet's remote commands:

`cargo test -p dctx-lex-cs`

`cargo clippy -p dctx-lex-cs -- -D warnings`
