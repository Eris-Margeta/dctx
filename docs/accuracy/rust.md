# Rust declaration accuracy (D1113B-ACCRUST)

**dctx is NOT STABLE until 2.0.** These are measurements on a small, authored
regression corpus, not an estimate of accuracy across arbitrary Rust projects.

The corpus in `crates/dctx-lex-rust/tests/accuracy/` contains **40 source files**
and **123 labelled declarations**, covering all 13 public `DeclarationKind`
variants. Every example was written for this corpus; no third-party source was
copied. Each source has a sibling `.expected.json` with independently authored
`{name, kind, line, parent}` labels. `line` is the one-based declaration start,
excluding attributes and docs. `name` is lexically qualified; `parent` is the
qualified enclosing declaration name, or JSON `null`. Impl members follow the
extractor's documented scope convention (`Cache::lookup`, parent
`Cache<K,V>::impl`). Fields, enum variants, closure bindings, external-module
imports and macro expansions are outside the declaration taxonomy.

There are 32 syntactically valid files (103 labels) and eight recovery files
(20 labels). Recovery files end in an explicit `@` syntax error, leaving the
preceding declarations unambiguous while forcing the conservative scanner.
They are deliberately malformed editor-buffer examples, not compilable crates.
Valid files are syntax examples, too: name resolution, feature availability and
type checking are not required. In particular, cfg-disabled declarations are
included, and trait aliases/negative impls exercise parsed unstable syntax.

Coverage includes generic inherent/trait impls with where clauses and split
headers; associated types, GATs and constants; macro_rules definitions; async,
const, unsafe and extern functions; foreign declarations; nested modules and
local items; raw identifiers; raw, byte and C strings containing declaration
text; lifetimes versus char literals; attributes, docs and nested comments
containing code; closure bindings; enum struct variants; unions; statics; type
and trait aliases; cfg-gated duplicates; CRLF and a UTF-8 BOM.

## Method and results

The baseline was measured with the unmodified extractor at
`54d0deff4fa6d65b051617d0a83325aa086989b7`, before changing `src`.
`tests/accuracy/before.tsv` freezes its per-kind expected/emitted/correct counts.
It is historical data, not a second implementation or a fresh baseline run.
The same labels were used for both measurements. The harness verifies that the
baseline and current expected counts still agree for every kind; corpus changes
require remeasuring the baseline if the comparison is to remain meaningful.

A true positive matches **all four fields** within the same source file.
Matching consumes occurrences, so duplicate declarations count separately and
an extra duplicate is a false positive. A wrong kind or parent counts as both
a false positive and a false negative. Precision is correct/emitted; recall is
correct/expected. An empty denominator scores 1. Per-kind values below are
percentages rounded to two decimal places; the gate uses unrounded ratios.

| Kind | Labels | Before correct/emitted | Before recall | Before precision | After correct/emitted | After recall | After precision |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Module | 10 | 10/10 | 100.00% | 100.00% | 10/10 | 100.00% | 100.00% |
| Struct | 15 | 15/15 | 100.00% | 100.00% | 15/15 | 100.00% | 100.00% |
| Union | 1 | 1/1 | 100.00% | 100.00% | 1/1 | 100.00% | 100.00% |
| Enum | 1 | 1/1 | 100.00% | 100.00% | 1/1 | 100.00% | 100.00% |
| Trait | 4 | 4/6 | 100.00% | 66.67% | 4/4 | 100.00% | 100.00% |
| TraitAlias | 4 | 2/2 | 50.00% | 100.00% | 4/4 | 100.00% | 100.00% |
| Impl | 11 | 10/10 | 90.91% | 100.00% | 11/11 | 100.00% | 100.00% |
| Function | 35 | 32/34 | 91.43% | 94.12% | 35/35 | 100.00% | 100.00% |
| Method | 13 | 12/12 | 92.31% | 100.00% | 13/13 | 100.00% | 100.00% |
| Const | 15 | 15/15 | 100.00% | 100.00% | 15/15 | 100.00% | 100.00% |
| Static | 5 | 5/5 | 100.00% | 100.00% | 5/5 | 100.00% | 100.00% |
| TypeAlias | 7 | 7/7 | 100.00% | 100.00% | 7/7 | 100.00% | 100.00% |
| Macro | 2 | 2/2 | 100.00% | 100.00% | 2/2 | 100.00% | 100.00% |
| **Overall** | **123** | **116/120** | **94.31%** | **96.67%** | **123/123** | **100.00%** | **100.00%** |

The valid-file subset was already 103/103 before the change. Improvements are
entirely in recovery: 13/20 correct with 17 emitted before, 20/20 with 20 emitted
after. The baseline had seven missing exact labels and four extra labels:
three missed same-line functions, a missed impl and misclassified method, two
trait aliases labelled as traits, and one spurious function inside a multiline
attribute. The latter is attribute token text, not a string/comment false
positive; both runs had zero string/comment false symbols in this corpus.

The fixes retain attribute bracket depth across lines and exclude attribute
contents from scope tracking; recognize subsequent item heads after braces and
semicolons; buffer split impl headers until the body opens; and distinguish
trait aliases in the recovery head parser. Recovery still reports partial,
opaque coverage and `LineScan` precision. The syntax visitor is unchanged.
`PARSER_REVISION` is now 2 so cached facts can be invalidated.

## Reproduction and safeguards

Measured with `rustc 1.100.0` and default crate features. Run from the repository
root, using worktree-local Cargo and temporary directories:

```sh
umask 022
export CARGO_HOME="$PWD/target/d1113b/cargo-home"
export CARGO_TARGET_DIR="$PWD/target/d1113b/build"
export TMPDIR="$PWD/target/d1113b/tmp"
mkdir -p "$TMPDIR"
cargo test -p dctx-lex-rust --no-fail-fast
cargo clippy -p dctx-lex-rust --all-targets -- -D warnings
cargo fmt --all -- --check
cargo test -p dctx-cli --no-fail-fast
```

`d1113b_accuracy` prints the frozen BEFORE table and freshly measured AFTER table
even with default test-output capture. It fails below 97% overall recall or 99%
overall precision. Independently lexed identifier-token starts reject declarations
starting inside literal bodies or comments. `phantom` sentinels additionally
reject code text inside attributes and unexpanded macro bodies. All corpus
sources must remain lexable, with valid files reporting complete extraction and
recovery files reporting partial extraction. No timing assertions are used.

## Remaining limitations

There are no remaining mismatches in this corpus. The corpus is intentionally
small and exercises selected defects; some kinds have only one or two examples.
Its perfect final score is not evidence of universal accuracy, and no large
third-party repository or random sample was measured here.

The recovery scanner remains heuristic. Item heads split after a visibility or
modifier (for example, `pub` on one line and `fn f()` on the next) can get the
wrong start line. An impl header with a braced const argument can be mistaken
for a body opener. A recovery trait alias with its `=` on another line can
still be classified as a trait. These cases are outside the scored corpus.
Unterminated literals/comments and malformed delimiter trees also cannot
promise the same recovery coverage as these lexable examples.

The extractor does not expand macros: generated declarations, including items
inside `include!` or other item-producing invocations, are not extracted. Fields
and enum variants are intentionally not declaration kinds. `mod name;` is an
import, not an inline-module declaration. No compiler cfg evaluation, name
resolution or type checking is part of this accuracy claim. Public outputs and
these conventions may change before dctx 2.0.
