# Java symbol extraction accuracy

**dctx is NOT STABLE until 2.0.** These are measurements of an authored
regression corpus, not estimates of accuracy across all Java projects.

## Corpus and scoring

`crates/dctx-lex-java/tests/accuracy/` contains **40 original Java source
files, 193 hand-labelled source symbols**, and one sibling `.expected.json`
per source. The examples were written for this corpus; none are copied from
third-party projects. They model real-world Java constructs in small, focused
snippets. They are not an independently sampled production-code benchmark.
Some refer to external libraries or annotations; the harness does not compile
or type-check them.

Each label is `{name, kind, line, parent}`. Names are lexically qualified with
`::`; `parent` is the containing symbol's qualified name, or `null` for a
file-level declaration. The parent is checked against the actual containment
edge, not inferred from the name. `line` is the one-based start of the
source declaration, including its annotations. Overloads are separate labels;
the scoring uses multiset matching and never deduplicates observations.

The model's existing Java vocabulary maps classes (including anonymous classes)
to `class`, records to `struct`, annotation types to `interface`, constructors
to `method`, and fields, enum constants and record components to `const`.
`@Test` nodes retain symbol kind `method` and participate in that row.
Local variables, parameters, type parameters, lambdas, method references and
initializer blocks do not introduce symbols in this model; declarations nested
inside their bodies still count. Anonymous classes use `new Type` names and
belong to the containing method or field. Implicitly generated record accessors,
constructors and other compiler-generated members are outside this source-symbol
metric. File module nodes and references are also outside it. `module-info.java`
and annotated `package-info.java` are negative controls with empty symbol lists;
this does not measure the accuracy of their module metadata.

A true positive must match **all four fields**. A wrong kind, line or parent
counts as both a missing expected symbol and an extra observed symbol.
Precision is TP / (TP + FP), recall is TP / (TP + FN). Overall numbers are
micro-averages over symbols, not averages over files or kinds. An empty
denominator is reported as 100%; no reported kind has an empty denominator.

The harness also independently blanks strings, character literals, text blocks
and comments while retaining byte positions and line endings, then requires the
extracted symbol tuples to remain identical. Every `Phantom` declaration used
as literal/comment bait is forbidden independently of the aggregate thresholds.
Thus even one leaked bait symbol fails, regardless of the precision percentage.
The masking oracle covers the raw spellings used here; it does not implement
Java's Unicode-escape preprocessing. Existing Unicode-escape tests remain in
the crate's separate suites.

## Measured before and after

Baseline: source at `54d0deff4fa6d65b051617d0a83325aa086989b7`, Java parser
revision 1. The `.before.json` siblings are actual observations captured before
editing the extractor, **not hand-labelled expectations**. `baseline.sha256`
freezes the source bytes, expectations and historical observations together.
The test checks that manifest and recomputes the before scores from those
observations on every run. It extracts the after observations using the current
code (parser revision 2). It does not rerun the old implementation during normal
tests and cannot rewrite the baseline. When extending this comparison, measure
the added examples with the original implementation as well.

| Symbol kind | Labels | Before TP / FP / FN | Before precision | Before recall | After TP / FP / FN | After precision | After recall |
| --- | ---: | --- | ---: | ---: | --- | ---: | ---: |
| class | 54 | 50 / 0 / 4 | 100.0000% | 92.5926% | 54 / 0 / 0 | 100.0000% | 100.0000% |
| const | 46 | 44 / 0 / 2 | 100.0000% | 95.6522% | 46 / 0 / 0 | 100.0000% | 100.0000% |
| enum | 3 | 3 / 0 / 0 | 100.0000% | 100.0000% | 3 / 0 / 0 | 100.0000% | 100.0000% |
| interface | 7 | 7 / 0 / 0 | 100.0000% | 100.0000% | 7 / 0 / 0 | 100.0000% | 100.0000% |
| method | 78 | 72 / 0 / 6 | 100.0000% | 92.3077% | 78 / 0 / 0 | 100.0000% | 100.0000% |
| struct | 5 | 5 / 0 / 0 | 100.0000% | 100.0000% | 5 / 0 / 0 | 100.0000% | 100.0000% |
| **Overall** | **193** | **181 / 0 / 12** | **100.0000%** | **93.7824%** | **193 / 0 / 0** | **100.0000%** | **100.0000%** |

The fixes address two causes of the twelve baseline misses:

- Post-parameter array dimensions, including annotated dimensions and dimensions
  before `throws`, now advance to the actual method body. Previously recovery
  skipped local types and later members. Ten misses came from `ArrayReturn.java`,
  `ArrayThrows.java` and `AnnotatedReturn.java`.
- Type-use annotations immediately after `new` now permit recognition of an
  anonymous class and its members. Two misses came from `AnnotatedNew.java`.
  The shared dimension walker also retains annotated field dimensions in the
  field signature.

## Coverage

| Construct | Source files |
| --- | --- |
| Compact constructors and generic records | `CompactRecord`, `GenericRecord` |
| Sealed interfaces, permits, non-sealed classes | `SealedTypes`, `NonSealed` |
| Enum constant bodies, abstract methods, constructors, annotations | `EnumBodies`, `EnumConstructor` |
| Annotation types, defaults, declaration annotations | `AnnotationType`, `AnnotatedMembers` |
| Bounded methods, intersection bounds, wildcards, generic constructors | `BoundedMethods`, `WildcardFields`, `GenericConstructor` |
| Nested, inner, local classes and records | `NestedTypes`, `InnerClass`, `LocalClass`, `LocalRecord` |
| Anonymous classes in methods/fields and qualified/generic construction | `AnonymousMethod`, `AnonymousField`, `AnnotatedNew`, `QualifiedNew`, `GenericAnonymous` |
| Lambdas and method/constructor/array references | `LambdaBodies`, `MethodReferences` |
| Default/static/private interface methods | `InterfaceMethods` |
| Static and instance initializers with local declarations | `Initializers` |
| Text blocks, escaped quotes/chars, declaration-like comments | `TextBlocks`, `Strings`, `Comments` |
| Module and package descriptors | `module-info.java`, `package-info.java` |
| CRLF and UTF-8 BOM, literal Unicode identifiers | `CrLf`, `Bom` |
| Fields and return arrays with annotated dimensions, throws | `ArrayFields`, `ArrayReturn`, `AnnotatedDimensions`, `ArrayThrows`, `AnnotatedReturn` |
| Overloads, test methods, try-with-resources, switch expressions | `Overloads`, `TestMethods`, `TryResources`, `SwitchBody` |

Names without an extension in this table refer to `.java` files. The CRLF and
BOM cases are stored as actual bytes, not escape sequences. The corpus-local
`.gitattributes` disables text conversion for Java fixtures so Git preserves
those bytes and the baseline manifest remains valid after checkout.

## Remaining misses and limits

There are **no remaining misses in the scored 193-symbol corpus**. That is a
finite regression result. In particular, the corpus was used to guide these
fixes, so its after score is not held-out validation.

Two additional manual probes of the public API confirmed limitations outside
this frozen corpus; they are **not included in either table**:

- An annotation within a qualified constructor type, for example
  `new java.util.@Mark ArrayList<String>() { void hit() {} }`, still loses the
  anonymous class and `hit` method. Only annotations immediately after `new`
  are fixed here.
- Escaped declaration names, for example `class \u0041 { void \u0072un() {} }`,
  are emitted using their raw escaped spellings instead of the Java names
  `A` and `A::run`. Under this corpus's exact-name scoring rule those would
  be misses and false positives. Escaped keywords being recognized does not
  establish that escaped names are normalized correctly.

Both probes can return `Completion::Complete`, so that flag is not a general
correctness guarantee. The samples do not establish completeness for every
Java release, malformed source, Unicode translation, or semantic resolution.
Method references are exercised as non-declarations; their target resolution
is not measured. The small enum and record rows especially should not be read
as broad claims of perfect extraction.

## Reproduction and acceptance

With a toolchain satisfying the workspace Rust requirement, run from the
repository root with a writable, worktree-local `CARGO_TARGET_DIR` and caches:

```sh
umask 022
cargo test -p dctx-lex-java --no-fail-fast
cargo clippy -p dctx-lex-java --all-targets -- -D warnings
cargo fmt --all -- --check
cargo test -p dctx-cli --no-fail-fast
```

The `d1113b_accuracy` integration test prints both tables even with default
Cargo test output capture and fails below overall recall 0.97 or precision
0.99. It includes separate controls for duplicate/incorrect observations and
for literal/comment masking. Existing Java tests remain part of the same gate.

Validation for this change: all four commands exited 0. The Java suite passed
42 tests and the CLI suite passed 366 tests, with zero failures or ignored
tests. Clippy and workspace formatting passed; no CLI golden drift occurred.
