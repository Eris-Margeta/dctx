# DX-RV3 review: hand-written lexers (py, go, java, c, cfg)

Read-only review of the scanner and the structural walk. No `cargo` run. Line numbers refer to this worktree. A verdict is FAIL when a concrete input is mis-parsed or wrongly refused, or when a cited standard is broken.

## Verdict

| Unit | Verdict |
|---|---|
| `crates/dctx-lex-py` | FAIL |
| `crates/dctx-lex-go` | FAIL |
| `crates/dctx-lex-java` | FAIL |
| `crates/dctx-lex-c` | FAIL |
| `crates/dctx-lex-cfg` | FAIL |
| decision 077 / decision 078 (all five) | PASS |
| Privacy in errors and logs | PASS |
| Production panics / unbounded input | PASS |
| Vacuous tests (`#[ignore]`, assertion with no positive control) | PASS |

Limits (bytes, depth, fact cap, UTF-8, dialect, invalid id) are typed `LexError`s and return no facts. Syntax damage is meant to stay `Completion::Partial` with diagnostics. The failures below break that split: some valid programs are marked complete while a declaration is dropped, and one valid C file is refused entirely.

## Findings

### 1. Go: `1.` eats the next declaration — high

`crates/dctx-lex-go/src/lexer/scan.rs:165` keeps a trailing dot only when the next byte is a digit. Go treats `1.` as one float, and a newline after a float inserts a semicolon (`scan.rs:274`). The dot is a separate punct token, `asi` is false for `.`, and `skip_expr` (`crates/dctx-lex-go/src/extract/references.rs:183`) stops only at a semicolon or a bracket that opened before the expression. No diagnostic is recorded, so `Completion` stays `Complete`.

Reproducer (`p.go`):

```go
package p

const (
    A = 1.
    B = 2
)

func F() {}
```

`B` and `F` are consumed as A's expression. Expected nodes include `B` and `F`. Missing test: float literals `1.` and `1.e2` at the end of a const/var spec, grouped and ungrouped, with a following declaration as the positive control (`const A = 1.0` must still see `F`).

Fix: treat `digits '.'` with no following digit, and `digits '.' 'e'/'E'`, as one number token so ASI runs before the next spec.

### 2. Python: same-quote f-string ends the literal — high

`fexpr_closed` (`crates/dctx-lex-py/src/lexer/strings.rs:199`) treats the f-string's own quote as the end of the expression before `scan_one_token` can take a string. Python 3.12 accepts that quote inside the replacement field (PEP 701). The `(` already pushed (`scan.rs:165`) is never closed, because the `)` sits in the bogus string, so later newlines do not become `Newline` tokens (`scan.rs:48`). `simple` then runs to EOF and the later `def` is not a declaration. A diagnostic makes the file `Partial`.

Reproducer (`m.py`), valid 3.12:

```python
x = f"{foo("bar")}"
def real():
    pass
```

Expected: function `real`, completion complete. Actual: `real` absent. The same hole is `f"{"}"}"; FLAG = 1` (the constant is inside the leftover string). Tests cover `"def fake"` and `b"def fake"` (`src/lib.rs:135`) and do not cover same-quote expressions.

Fix: while an f-expression is open, lex string literals with `scan_one_token` before testing the outer closer, and do not push those brackets onto the statement paren stack.

### 3. C/C++: every `<` counts as nesting — high

`pair` (`crates/dctx-lex-c/src/lexer/mod.rs:226`) increments `angle` on every `<` punct and returns `LexError::TooDeep` once `angle` exceeds `max_depth` (ceiling 256). The error is returned from `lex`, so the file yields no facts. Shift `>>` subtracts two (`mod.rs:236`); comparisons do not have a matching `>`.

Reproducer (`t.c`): `int a;`<newline> repeated, each line `if (a < 1) a++;`, 257 times. Expected: partial or complete facts for a valid translation unit. Actual: `TooDeep`. A one-comparison file is the positive control.

Fix: count `<` `>` only while a template argument list is open (after a type name), or stop refusing on angle depth and record a diagnostic.

### 4. YAML: a line that trims to `---` splits the document — high

`documents` (`crates/dctx-lex-cfg/src/yaml.rs:473`) treats any line whose trim is `---` or `...` as a document boundary. It does not track quotes or block scalars. Indented `---` inside `|` is still a separator (`separator` at `yaml.rs:491`).

Reproducer (`a.yaml`):

```yaml
msg: |
  ---
b: 1
```

Expected: one document with keys `msg` and `b`. Actual: the line `---` starts a new document, so `b` is not a sibling of `msg`. Missing test: `---` and `...` indented inside `|`, `>`, and quotes, plus a real top-level `---` as the positive control.

Fix: recognize document markers only at column 0 outside strings and block scalars.

### 5. JSON: surrogate pairs drop the rest of the object — high

`string` (`crates/dctx-lex-cfg/src/json.rs:320`) decodes one `\uXXXX` with `char::from_u32`. A surrogate is `None`, so the function diagnoses and returns `None` with the cursor after the first unit. The object walk then leaves the value and stops. RFC 8259 requires UTF-16 surrogate pairs.

Reproducer (`a.json`): `{"emoji":"\uD83D\uDE00","keep":1}`. Expected: keys `emoji` (U+1F600) and `keep`, completion complete. Actual: `keep` absent, completion partial. Missing test: a pair, a lone surrogate (must be partial), and `"\u0041"` as the positive control.

Fix: when a high surrogate is followed by a low surrogate, emit one scalar; diagnose a lone surrogate without abandoning the rest of the value.

### 6. YAML: quoted scalars cannot contain a newline — high

`quoted` (`crates/dctx-lex-cfg/src/yaml.rs:423`) returns `UnterminatedString` on `\n` for both `"` and `'`. YAML 1.2 allows a line break inside a quoted scalar.

Reproducer: `a: "hello\nworld"\nb: 1\n` with a real newline between hello and world. Expected: keys `a` and `b`. Actual: the string is unterminated and `b` is not reliably a sibling key. Missing test: double- and single-quoted multiline scalars beside a following key.

Fix: accept newlines in quoted scalars (fold escaped breaks in double quotes) until the closing quote.

### 7. JSON arrays accept a trailing comma — medium

Object trailing commas are diagnosed when `jsonc` is false (`json.rs:149`). `array` (`json.rs:186`) does `continue` after `,` and then accepts `]`, with no diagnostic. `[1,]` is not JSON. Completion stays `Complete`.

Reproducer: `[1,]` on `a.json`. Expected: `InvalidSyntax` and `Partial`. `[1]` is the positive control. JSONC may keep the comma legal.

Fix: use the same peek-at-closer check the object path uses.

### 8. TOML: backslash newline only matches LF — medium

In a multiline basic string, only `\` + `\n` is a line-ending escape (`crates/dctx-lex-cfg/src/toml.rs:406`). TOML also discards `\` + CRLF. A CR hits the `_` arm and returns `InvalidSyntax`, so the key's value is dropped.

Reproducer (`a.toml`, CRLF): `a = """foo\<CRLF>bar"""`. Expected: key `a`, signature `foobar`, completion complete. LF form is the positive control.

Fix: if the byte after `\` is CR, consume an optional following LF and skip indentation, same as the LF arm.

### 9. Java: `\u` escapes are not translated in raw source — medium

`run` (`crates/dctx-lex-java/src/lexer/scan.rs:46`) tokenizes the file as written. `escape` (`scan.rs:215`) runs only inside strings, text blocks, and character literals. The JLS translates `\u` escapes before tokenization, including those that form a keyword or a line terminator.

Reproducer (`A.java`): `\u0063lass A {}`. Expected: class `A`. Actual: the word is not `class`, so no type is emitted. Second input: `// c\u000a class B {}` (the escape is a newline). Expected: class `B`. Actual: `B` stays inside the line comment.

Fix: one pre-pass that replaces `\u` escapes (and `\\u` runs) before the scan, including inside comments.

### 10. C dialect: `.C` is C, and several C++ extensions are refused — medium

`Dialect::for_path` (`crates/dctx-lex-c/src/domain/facts.rs:19`) compares `.c` case-insensitively, so `box.C` is `Dialect::C`. GCC treats `.C` as C++. The test at `facts.rs:147` locks the C label in. Extensions `.hxx`, `.c++`, `.ixx`, and `.cppm` are not in the list at `facts.rs:21`, so `Widget.hxx` is `UnsupportedDialect` and returns no facts.

Fix: if the extension is exactly `.C`, select C++; add the other C++ extensions. Keep `.c` and `.h` as C.

### 11. Containment edges stay `Confidence::Exact` after damage — medium

PARSERS-v1 allows `exact` for syntactically proven containment with complete spans. These sites always set `Confidence::Exact` while a separate precision label may be `Partial`:

- `crates/dctx-lex-py/src/extract/mod.rs:232`
- `crates/dctx-lex-go/src/extract/mod_helpers.rs:224`
- `crates/dctx-lex-java/src/extract/mod_helpers.rs:139`
- `crates/dctx-lex-c/src/extract/declarations_types.rs:342`
- `crates/dctx-lex-cfg/src/path.rs:424`

A node whose end lies past the first diagnostic still gets an exact contains-edge. Fix: when precision is `Partial`, set confidence to `Name` (or the model's non-exact variant) on that edge.

### 12. Files over the ~300-line standard — medium

`docs/standards/RUST-STANDARDS.md` and `docs/standards/CODE-GUIDELINES.md` cap a file at about 300 lines. These are past that (tests in the same file included):

| Lines | Path |
|---:|---|
| 497 | `crates/dctx-lex-cfg/src/yaml.rs` |
| 496 | `crates/dctx-lex-cfg/src/path.rs` |
| 493 | `crates/dctx-lex-java/tests/dctx_lex_java.rs` |
| 492 | `crates/dctx-lex-cfg/src/toml.rs` |
| 489 | `crates/dctx-lex-py/src/extract/mod.rs` |
| 488 | `crates/dctx-lex-py/src/extract/declarations.rs` |
| 485 | `crates/dctx-lex-py/src/lexer/strings.rs` |
| 480 | `crates/dctx-lex-c/src/lexer/mod.rs` |
| 478 | `crates/dctx-lex-c/src/extract/mod.rs` |
| 434 | `crates/dctx-lex-c/src/extract/declarations_types.rs` |
| 422 | `crates/dctx-lex-java/src/extract/references.rs` |
| 419 | `crates/dctx-lex-java/src/lexer/scan.rs` |
| 413 | `crates/dctx-lex-c/src/extract/functions.rs` |
| 412 | `crates/dctx-lex-c/src/extract/declarations.rs` |
| 393 | `crates/dctx-lex-cfg/src/json.rs` |
| 391 | `crates/dctx-lex-py/src/extract/references.rs` |
| 374 | `crates/dctx-lex-java/src/extract/declarations_members.rs` |
| 373 | `crates/dctx-lex-c/tests/dctx_lex_c.rs` |
| 361 | `crates/dctx-lex-java/src/extract/declarations_modifiers.rs` |
| 360 | `crates/dctx-lex-c/src/lexer/scan.rs` |
| 348 | `crates/dctx-lex-java/src/extract/declarations_types.rs` |
| 346 | `crates/dctx-lex-go/src/lexer/scan.rs` |
| 313 | `crates/dctx-lex-cfg/tests/dctx_lex_cfg.rs` |

`crates/dctx-lex-go/src/extract/declarations.rs` is 302 lines. Fix: split on the boundaries already described in the `//!` headers (scan vs test, declaration vs finish, string vs f-string).

## No finding

decision 077 / decision 078. Each `Cargo.toml` is Apache-2.0 and depends only on `dctx-hash` and `dctx-map-model`. `dctx-lex-py` dev-depends on `proptest`, which the Rust standard already allows. Every crate has `#![forbid(unsafe_code)]`. Body fingerprints call `dctx_hash::Sha256`. No crate spawns git, reads `.git`, or uses a git object id.

Privacy. The lexer never opens a path; the caller passes bytes. `LexError` text is a limit, a line number, or an id error, not a source snippet. Diagnostics are line, byte, and reason. No production `println` / `eprintln`. Docstrings and config scalars are copied into summaries and signatures; that is the fact contract, not an error or a log.

Panics and allocation. Depth and byte ceilings are checked before recursive walks. Production `unwrap` did not show up outside tests. Comment and problem buffers in the Python lexer are capped. No panic on a hostile input was demonstrated from the control flow that was read.

Tests. No `#[ignore]`. Refusal tests in each `lib.rs` pair the error with an input that succeeds. They do not cover the inputs in findings 1–10, so those bugs are untested, and the tests that do exist are not vacuous.

Line splice in C. `prepare` / `splice_len` (`crates/dctx-lex-c/src/lexer/mod.rs:150` and `:189`) delete `\` + LF, CR, and CRLF before scanning, including inside comments and strings. A continued `#define` or `//` line does not leak the next physical line as code.

Headers. Every reviewed `.rs` file starts with a `//!` header of three to six lines.
