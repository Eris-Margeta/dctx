# Kotlin symbol extraction accuracy

**dctx is NOT STABLE until 2.0.** These are measurements of an authored
regression corpus, not estimates of accuracy across all Kotlin projects.

## Corpus and scoring

`crates/dctx-lex-kt/tests/accuracy/` contains **42 original Kotlin source
files (39 `.kt`, 3 `.kts`) with 453 hand-labelled symbols**. Each source has a
sibling `<file>.expected.json` (the hand labels) and `<file>.before.json` (what
the extractor produced before this round of fixes, frozen). `baseline.sha256`
pins every source, label and baseline file. The examples were written for this
corpus; none are copied from third-party projects. They model real-world
constructs in small, focused files and are not compiled or type-checked.

Covered constructs: data, sealed, enum, value and inline classes; companion
objects (named and unnamed); object declarations and anonymous object
expressions; extension functions and properties; `suspend`, `inline`/`reified`,
`tailrec`, `external`, `expect`/`actual`; type aliases; annotation classes and
annotations with arguments, use-site targets and `@file:`; primary-constructor
`val`/`var` properties and annotated `constructor` keywords; secondary
constructors; nested, inner and local classes; interfaces with default methods
and `fun interface`; top-level, `const`, `lateinit` and delegated properties;
`operator` and `infix` functions; backtick names; raw strings, string templates
and character literals that contain declaration keywords; KDoc and nested block
comments containing code; `where` clauses and multi-line signatures; scripts
with a shebang and a Gradle-style build script; CRLF line endings; a UTF-8 BOM.

Each label is `{name, kind, line, parent}`. Names are lexically qualified with
`::` and backticks are dropped (`Calculator spec::adds two numbers`). `parent`
is the containing symbol's qualified name, or `null` for a file-level
declaration; the harness reads it from the actual containment edge. `line` is
the one-based first line of the node's span: the leading KDoc block when there
is one, otherwise the first annotation or modifier. Scoring is multiset
matching, so a duplicate or a wrong line or parent counts as both a false
positive and a miss.

Kind mapping: classes, objects and companion objects are `class` (an unnamed
companion is `Companion`); data classes are `struct`; interfaces and annotation
classes are `interface`; enum classes are `enum`; file-level and local functions
are `fn`; member functions and constructors (named after the type) are
`method`; properties, primary-constructor `val`/`var` parameters and enum
entries are `const`; type aliases are `type`. Local variables, parameters,
lambdas, anonymous functions, anonymous objects themselves, accessors and
script statements do not introduce symbols. Named functions inside an anonymous
object belong to the nearest enclosing named declaration
(`val listener = object : Runnable { override fun run() }` gives
`listener::run`).

The test (`cargo test -p dctx-lex-kt --test d1113c_accuracy`) prints the table
below, fails when overall recall is below 0.97 or precision below 0.99, fails
when a valid fixture produces a diagnostic, and fails when any symbol comes from
inside a string or comment: every bait name contains `phantom`, and every file
is re-extracted after an independent masking pass blanks comments, strings and
character literals (nested comments, raw strings and `${...}` templates
included). The symbols must be identical.

## Results

| kind      | before P | before R | after P | after R |
|-----------|---------:|---------:|--------:|--------:|
| class     |   95.00% |   97.44% | 100.00% | 100.00% |
| const     |  100.00% |   98.67% | 100.00% | 100.00% |
| enum      |  100.00% |  100.00% | 100.00% | 100.00% |
| fn        |   98.15% |  100.00% | 100.00% | 100.00% |
| interface |  100.00% |  100.00% | 100.00% | 100.00% |
| method    |   97.76% |   95.62% | 100.00% | 100.00% |
| struct    |  100.00% |  100.00% | 100.00% | 100.00% |
| type      |  100.00% |  100.00% | 100.00% | 100.00% |
| **overall** | **98.23%** | **97.79%** | **100.00%** | **100.00%** |

Before: 443 true positives, 8 false positives, 10 misses. After: 453, 0, 0.

## Defects fixed

- An anonymous object expression (`val x = object : T { ... }`) was read as a
  declaration of a class named `Companion`, at file level or nested inside the
  property's owner, and its members were attached to that phantom class.
- A class literal (`T::class`, `x::class`) ended the enclosing expression and
  produced a `Malformed` diagnostic on valid code.
- Local classes inside function bodies were not extracted; their member
  functions were reported as local functions of the enclosing function.
- A primary constructor written with annotations or modifiers before the
  `constructor` keyword on its own line started at the parameter list instead
  of at its first annotation.

## Held-out corpus

After the fixes above, a second author wrote
`crates/dctx-lex-kt/tests/accuracy/heldout/`: **10 more original files (9
`.kt`, 1 `.kts`) with 170 hand-labelled symbols**, labelled with the same rules
and written to mix constructs the way real files do: multi-line generic
parameter lists with trailing commas, annotations spread over several lines,
named and nested objects inside companions, enum entries with bodies and
constructor arguments, sealed interfaces with `data object`, generic extension
receivers, DSL builders, `where` clauses, `expect`/`actual`, `fun interface`,
`value class`, delegation (`by`), labels, nested string templates and
character literals containing braces, and context receivers. The test
`d1113g_heldout_accuracy` scores it separately with the same thresholds and
the same masking check.

| corpus   | before P | before R | after P | after R |
|----------|---------:|---------:|--------:|--------:|
| held-out |   99.41% |   99.41% | 100.00% | 100.00% |

Before: 169 true positives, 1 false positive, 1 miss. The one defect it found:

- A declaration with context receivers (`context(Logger)` on the line before
  `fun`) started at the `fun` keyword instead of at the `context(...)`
  modifier, so its line was off by one. `context(...)` is now read as part of
  the modifier list when a declaration follows it.

## Known limits

- 100% on this corpus does not mean 100% on real projects. The corpus was
  written by the same people who fixed the extractor, so it is biased towards
  the constructs they thought of.
- Not covered: multiplatform source sets beyond `expect`/`actual`, generated code, very large
  files, and deliberately malformed input (see the hostile-input tests instead).
- A call to a function named `context(...)` directly before a local
  declaration in a function body or script is read as a context-receiver
  list, which moves that declaration's start line up.
- Destructuring declarations and properties of anonymous objects declared inside
  function bodies are local and produce no symbols by design.
- Call and reference resolution, signatures, end lines, summaries and
  fingerprints are not part of this measurement.
