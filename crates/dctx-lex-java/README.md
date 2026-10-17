# dctx-lex-java

`JavaLexer::extract(path, bytes)` turns one authorized `.java` file into
deterministic `LexedFile` facts. The extension match is case-insensitive.
Other extensions are `UnsupportedDialect`. The caller performs scanning and
file IO; this crate does not read the disk, the environment, or git.

`PARSER_ID` is `dctx-lex-java` and `PARSER_REVISION` is `1`. Every node
carries a `body_fingerprint`: SHA-256 of the body with whitespace runs
collapsed, from `dctx-hash`. A declaration with braces fingerprints that
brace span. A field with `=` fingerprints the initializer. Anything else
fingerprints its signature. The module fingerprints the whole file.

Nodes and exact `contains` edges use the public `dctx-map-model` types.
`RawReference` keeps the originating node, relation, lexical scope, an empty
alias, one-based inclusive lines, and the owner path. Repeated sites stay
separate records. Nothing in this crate resolves a destination.

Extracted facts:

- The package name is the module signature (`com.example.app`), not a
  separate node. A leading `/** */` supplies the module summary (`doc`);
  otherwise the summary source is `sig`.
- Imports, including `static` and `*` on-demand imports. The spelling keeps
  a `static ` prefix and a `.*` suffix. Aliases are absent.
- Classes, interfaces, enums, records, and `@interface` types, with modifiers.
  Records are `struct`. Enum constants and fields are `const`. Methods and
  constructors are `method`. Record components are `const` and not public;
  accessors are not invented.
- Constructors use the type's simple name (`Box::Box`). Nested names use `::`.
  Duplicate names get `~1`, `~2`, only on the stable id.
- Signatures run from the first declaration token through the name (fields,
  including `[]`) or through `)` and `throws` (methods). Javadoc is the
  summary and the start of the span, not part of the signature. The summary
  is the first non-tag sentence, up to `". "`.
- Annotations, `extends` on classes, bounds, field and method types, record
  components, `throws`, and `Foo.class` emit `uses-type`. `implements`, and
  `extends` on interfaces, emit `implements`. `permits` is `uses-type`.
  Type arguments of a superinterface are `uses-type`; the superinterface
  name is not repeated there.
- Calls are bare names and identifier `.` chains (`service.run`). `this(` and
  `super(` are calls. `new Type(...)` is `new Type`. `new Type(...) { }` is
  also a class named `new Type`. A method annotated `@Test` (or a name ending
  in `.Test`) is a `test` node at `Convention` precision, and calls inside it
  also emit `tests` at `Convention`. The calls themselves stay `Structural`.

Every fact has a precision label: `Structural`, `Convention`, or `Partial`.
A fact whose end byte is past the first diagnostic is `Partial`. The module
stays at its own precision. Incomplete or malformed files return `Ok` with
`Completion::Partial` and a diagnostic (`UnterminatedComment`,
`UnterminatedString`, `UnterminatedTextBlock`, `UnterminatedChar`,
`UnbalancedBracket`, `UnsupportedName`, `Malformed`). They do not panic.
At most 64 diagnostics are retained; overflow still marks the file partial.

Refusals return no facts: `UnsupportedDialect`, `InvalidId`, `InvalidUtf8`,
`TooLarge`, `TooDeep`, `TooManyFacts`. Caller limits only narrow the ceilings
(8 MiB, depth 256, 250_000 facts). `>` is always one token, so `>>` closes
two type arguments. `module` and `open module` are skipped.

Abstentions, rather than invented edges: bare field reads (`point.x`),
method references (`String::trim`), local-variable types, unicode-escape
preprocessing, overload resolution, type inference, preview string templates,
and lambda expressions as symbols. Lambda bodies are still scanned for calls.

The crate depends only on `dctx-map-model` and `dctx-hash`. It is pure Rust,
Apache-2.0, with no `unsafe` and no new external dependency.

Validation uses the packet's remote commands:

`cargo test -p dctx-lex-java`

`cargo clippy -p dctx-lex-java -- -D warnings`
