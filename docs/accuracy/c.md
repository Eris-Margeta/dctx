# C and C++ symbol extraction accuracy

**dctx is NOT STABLE until 2.0.** These are measurements of a small, authored
regression corpus, not an estimate of accuracy across arbitrary projects.

The corpus in `crates/dctx-lex-c/tests/accuracy/` contains 38 source files (20 C++,
18 C) and 125 manually labelled symbols. All examples were written for this
corpus; none contain copied third-party code. They exercise macros (object-like,
function-like and continued), function pointers and callback typedefs, static
inline functions, tagged and anonymous typedef aggregates, K&R definitions,
nested/inline/anonymous namespaces, templates and specializations, operators,
conversion operators, constructors/destructors, qualified definitions, lambdas,
raw and escaped strings, comments, inactive conditionals, C linkage, attributes,
CRLF and a UTF-8 BOM. Two files contain only negative lexer input; the standalone
raw literal in `30_string_only.cpp` intentionally is not a valid translation unit.

## Labelling and scoring

Each source has a sibling `.expected.json`, an array of objects with `name`,
`kind`, `line` and `parent`. Labels were authored from the declarations, using the
extractor's public representation:

- `line` is the first physical line of the declaration, including a preceding
  template or attribute. `parent: null` means the file; other parents name the
  lexical containing symbol. Out-of-class definitions belong to their enclosing
  namespace/file, consistent with the existing containment contract.
- Names are qualified with `::`. Destructors use `dtor_Name`; conversion types
  retain spaces between words and bind pointer punctuation (`operator const char*`).
  Specialization arguments are omitted, matching the current name model.
- `untyped` means `symbol_kind: None`: namespaces, namespace aliases, fields and
  globals. Unions and typedef aliases use `type`; enum members use `const`.
  An anonymous typedef's members belong to its alias. Anonymous namespaces have
  no namespace symbol and their children retain the surrounding lexical parent.
  Local bindings and unnamed lambdas are outside the symbol inventory.
- Literal `#if 0` branches have no expected symbols. Conditions the extractor
  cannot evaluate retain alternative declarations with `Convention` precision.

The test matches the complete four-field tuple, one occurrence at a time;
a wrong name, kind, line or parent contributes both a false positive and a false
negative. Duplicate declarations are not collapsed. Module nodes are excluded.
Precision is TP/(TP+FP), recall is TP/(TP+FN); an empty denominator scores 1.
Overall values use pooled counts, not averages of percentages.

Every declaration name planted in a comment or literal uses `ghost_`, `GHOST_`
or `fake`. Any extracted poison name fails independently of the aggregate
threshold, including poison sharing a real declaration's starting line. The
negative-only files reject every emitted symbol outright and also participate
in precision. The byte-exact sources and
labels are pinned by `corpus.sha256`, including BOM and CRLF bytes. The local
`.gitattributes` disables text normalization for the CRLF fixture.

## Measured before and after

The before run used commit `54d0deff4fa6d65b051617d0a83325aa086989b7`, parser
revision 2, against these same sources and labels before extraction changes.
It failed the accuracy gate. Its measured counts are retained in `before.tsv`.
The after run uses parser revision 3. The test prints the **recorded** before
table and a freshly computed after table; it does not rerun the old extractor.

| Kind | Before TP/FP/FN | Before precision | Before recall | After TP/FP/FN | After precision | After recall |
| --- | --- | --- | --- | --- | --- | --- |
| class | 2/0/0 | 100.000% | 100.000% | 2/0/0 | 100.000% | 100.000% |
| const | 6/0/0 | 100.000% | 100.000% | 6/0/0 | 100.000% | 100.000% |
| enum | 2/0/0 | 100.000% | 100.000% | 2/0/0 | 100.000% | 100.000% |
| function | 21/5/4 | 80.769% | 84.000% | 25/0/0 | 100.000% | 100.000% |
| macro | 5/1/1 | 83.333% | 83.333% | 6/0/0 | 100.000% | 100.000% |
| method | 22/0/5 | 100.000% | 81.481% | 27/0/0 | 100.000% | 100.000% |
| struct | 15/1/0 | 93.750% | 100.000% | 15/0/0 | 100.000% | 100.000% |
| type | 7/2/2 | 77.778% | 77.778% | 9/0/0 | 100.000% | 100.000% |
| untyped | 26/8/7 | 76.471% | 78.788% | 33/0/0 | 100.000% | 100.000% |
| **Overall** | **106/17/19** | **86.179%** | **84.800%** | **125/0/0** | **100.000%** | **100.000%** |

Fixes bind pointer declarator names before scanning their parameter lists;
recognize functions returning function pointers and K&R parameter declarations;
retain operator qualifiers and conversion type names; choose the body after
constructor initializers; retain anonymous typedef members; and skip an initial
BOM without shifting offsets. Literal false branches are removed before bracket
pairing and macro bookkeeping. Unknown conditions remain conservative. The
existing C fixture's stale expectation of `configured` inside `#if 0` was corrected
and its parser revision assertion updated in `tests/dctx_lex_c.rs`.

## Known remaining misses and limits

There are no remaining misses in these 125 labels. This corpus drove the fixes;
it is not an independent holdout set. The two class and two enum labels in
particular provide little statistical evidence for generalization. Kinds outside
the nine rows above have no C/C++ labels here.

The extractor remains lexical, not a complete C/C++ parser or preprocessor.
The concrete examples below were checked in separate probes and do not enter
the scored table:

- Only literal `0`/`1` conditions, with optional enclosing parentheses and comments,
  are evaluated. For example, `#if 0U` still retains its inactive declaration.
  Macro replacement and build configurations are not evaluated; macro-introduced
  declarations can still be missed. Unknown branch alternatives are inventoried.
- Function-type typedefs without a pointer, such as
  `typedef int Handler(int code);`, can bind `code` instead of `Handler`.
- Constants of an untagged, non-typedef enum, such as
  `enum { FLAG = 1, READY = 2 };`, are missed.
- Grouped function declarations such as `int first(void), second(void);` can lose
  the first function. These examples are outside the scored corpus.
- Specializations share the primary template's spelling and receive duplicate
  ordinals; overload signatures are not part of names. Anonymous namespaces are
  flattened, and namespace/field/global kinds remain coarse. Scores assess that
  representation, not full C++ semantic identity.

Run `cargo test -p dctx-lex-c --no-fail-fast` to print both tables and enforce
recall >= 0.97, precision >= 0.99, and zero poison symbols. The baseline fingerprint
must be deliberately remeasured if the corpus is changed. Other requested gates
are `cargo clippy -p dctx-lex-c --all-targets -- -D warnings`,
`cargo fmt --all -- --check`, and `cargo test -p dctx-cli --no-fail-fast`.
