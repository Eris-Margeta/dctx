# DX-RV4 independent review

Read-only review of the hand-written lexers and of the facade and map-parse wiring. No `cargo` run. Line numbers are from this tree.

## Verdicts

| Unit | Verdict |
|---|---|
| `crates/dctx-lex-cs` | PASS-WITH-NOTES |
| `crates/dctx-lex-kt` | FAIL |
| `crates/dctx-lex-rb` | FAIL |
| `crates/dctx-lex-php` | FAIL |
| `crates/dctx-lex-swift` | PASS-WITH-NOTES |
| `crates/dctx-map-parse/src/adapters/{csharp,kotlin,ruby,php,swift}.rs` | PASS-WITH-NOTES |
| Facade `language_of` / `lex_dispatch` vs `Language::for_path` | FAIL |

## Findings

### 1. High — Ruby: `/` after a call is division, so a regexp keyword swallows the next method

`crates/dctx-lex-rb/src/lexer/scan.rs:179` and `:260`. After an ordinary identifier, `ident` sets `expr` from `expr_keyword`, which is false for `puts`. `slash` then emits `/` as an operator. `%` in the same position is a literal (`scan.rs:144`); `/` is the odd one out.

`do`, `def`, `class`, `module`, `begin`, `case`, and `for` are statement enders (`extract/references.rs:210`). `scan_statement` (`references.rs:45`) will not end the statement on a newline while an ender is open, and it never calls `declaration`. The following method is consumed as the phantom block. Completion stays `Complete` because nothing records a diagnostic.

Real Ruby parses `puts /do/` as a call with a regexp argument. The method on the next line is a real declaration.

```ruby
puts /do/
def real
end
```

Expected: method `real`, no diagnostic. Actual: no method `real`.

The positive control at `lexer/mod.rs:211` only checks `10 / 2` and `return /cd/`. `return` is an expression keyword, so that test stays green.

Fix: after a command-position identifier, scan `/.../` as a regexp the same way `%r` and `return /.../` already are, and add this source as a test that requires a `real` method and an empty diagnostic list.

### 2. High — PHP: a heredoc closes on a label prefix, and the body becomes code

`crates/dctx-lex-php/src/lexer/strings.rs:121`. A line closes the heredoc when it starts with the label (leading spaces and tabs allowed) and the next byte is not an identifier byte. A space, semicolon, dot, or parenthesis all close it. The token ends at the label (`end` at line 128), so the rest of that line is scanned as PHP. There is no diagnostic, so `parse_pure` publishes the file as complete.

Real PHP closes only when the rest of the line is an optional `;` and then a newline. `END of the text` is body text.

```php
<?php
$s = <<<END
END of the text
function ghost() {}
END;
function real() {}
```

Expected: function `real` only. Actual: `function` at `declarations.rs:44` also emits `ghost`. The same function handles nowdoc. `END; function ghost() {}` on the closer line is the same bug: PHP requires a newline after the semicolon.

Missing test: this source, asserting the name set is exactly `real`.

Fix: after the label, allow only optional `;`, optional `\r`, and then `\n` or EOF; otherwise keep scanning.

### 3. High — PHP: `?>` inside `/* */` does not return to HTML

`crates/dctx-lex-php/src/lexer/scan.rs:45`. Line comments and `#` comments stop at `?>` and leave PHP mode (`scan.rs:38`). Block comments search only for `*/`. PHP ends a block comment at `?>` and switches to HTML until the next open tag.

```php
<?php
/* ?>
*/
function ghost() {}
```

Expected: no function (`ghost` is HTML). Actual: the comment ends at `*/` and `ghost` is emitted, completion `Complete`.

The inverse drops a real function and reports `UnterminatedComment`:

```php
<?php
/* ?>
<?php
function real() {}
```

Expected: function `real`. Actual: `real` stays inside the comment, and a diagnostic makes the adapter refuse the file.

Missing test: both sources.

Fix: on `?>` in a block comment, close the comment and set `php = false`, matching the line-comment path. A later `<?php` starts PHP again.

### 4. High — Kotlin: `\` plus a non-ASCII character skips the closer and the next declaration

`crates/dctx-lex-kt/src/lexer/scan.rs:177`. A regular-string escape does `pos += 1` twice, one byte each. U+00E9 is the bytes `C3 A9`, so `pos` lands on the continuation byte. `bump` (`lexer/mod.rs:385`) calls `char_at` (`mod.rs:381`), and `str::get` on a non-boundary returns `None`, so the width used is U+FFFD's 3. Those 3 bytes skip the rest of `é`, the closing quote, and the newline. `scan_string` then reads the next line as string text until a later newline and reports `UnterminatedString`. This is not a panic (`get` is checked). It is a desync.

```kotlin
fun earlier() = 1
val s = "\é"
fun real() = 1
```

`\é` is an illegal escape. The Kotlin lexer still ends the string at the quote, so `fun real` is a declaration. This scanner never tokenizes `real`. The spurious diagnostic makes `adapters/kotlin.rs:15` refuse the whole file, so `earlier` is dropped at the map-parse boundary as well.

Missing test: this file, asserting a `real` function node. The escape should advance one scalar, not one byte and then a replacement-character width.

Fix: after a backslash, call `bump` once from a character boundary. Keep a continuation byte at width 1.

### 5. Medium — Facade and map-parse disagree on case, and both drop Ruby basenames the lexer accepts

Tags agree: `lex_dispatch.rs:35` uses `csharp`, `kotlin`, `ruby`, `php`, `swift`, and `adapters/mod.rs:55` sends those `Language` variants to the same crates.

Extensions do not.

- `crates/dctx/src/index/extract.rs:37` matches the extension with `rsplit_once` and no case fold. `App.CS`, `main.KTS`, `app.RB`, `app.PHTML`, and `App.SWIFT` are `opaque`, so `symbols` returns nothing.
- `crates/dctx-map-parse/src/domain/facts.rs:45` lowercases the last dot-segment. The tests at `facts.rs:165` lock `App.CS`, `main.KTS`, and `App.SWIFT` to the real languages. Each lexer `Dialect::for_path` is also case-insensitive (`dctx-lex-cs/src/domain/facts.rs:17` and the siblings).

`dctx-lex-rb/src/domain/facts.rs:15` also accepts basenames `Rakefile` and `Gemfile`. Neither dispatcher does. `language_of("Rakefile")` is `opaque` (no dot). `Language::for_path("Rakefile")` lowercases the whole path to `rakefile`, which is not an arm, so the file is an opaque module. A `def real` in a `Gemfile` is extracted only by a direct lexer call.

`for_path` uses `rsplit('.').next()`, so a dot-free path whose entire text is an extension (`php`, `swift`, `inc`) is classified as that language. `language_of` yields `opaque` for the same path.

NUL: `extract.rs:58` returns no symbols when any byte is 0. `adapters/mod.rs:122` `prepare` accepts a NUL because it is valid UTF-8, then the lexer emits signatures from that file.

Fix: fold case in `language_of` the same way `Language::for_path` does; teach both dispatchers the `Rakefile` and `Gemfile` basenames; decide one NUL rule and use it in `prepare` and `extract_structural`.

### 6. Medium — Ruby: a non-ASCII heredoc terminator never matches, so the rest of the file is string text

`crates/dctx-lex-rb/src/lexer/heredoc.rs:66` and `:75` push `char::from(byte)` , which is Latin-1, not UTF-8. The line compare at `heredoc.rs:270` uses the real UTF-8 line. Unquoted labels stop at the first non-ASCII byte (`ident_end`, `heredoc.rs:277`), so `<<hé` stores the terminator `h`.

```ruby
def kept
end
s = <<"hé"
body
hé
def lost
end
```

`hé` is a legal Ruby identifier. The closing line does not match, the heredoc runs to EOF, `lost` is never tokenized, and `UnterminatedString` makes the adapter refuse the file (including `kept`).

Missing test: this file, expecting methods `kept` and `lost` and no unterminated-string diagnostic.

Fix: decode the quoted terminator as UTF-8, and let `ident_end` follow Ruby identifier scalar rules.

### 7. Low — Several files break the 300-line standard

`docs/standards/RUST-STANDARDS.md` and `docs/standards/CODE-GUIDELINES.md` cap a file at about 300 lines. These are past that:

| File | Lines |
|---|---|
| `dctx-lex-kt/src/lexer/mod.rs` | 496 |
| `dctx-lex-kt/src/extract/mod.rs` | 490 |
| `dctx-lex-kt/src/extract/declarations.rs` | 485 |
| `dctx-lex-kt/src/lexer/scan.rs` | 484 |
| `dctx-lex-rb/src/extract/mod.rs` | 476 |
| `dctx-lex-cs/src/lexer/scan.rs` | 464 |
| `dctx-lex-rb/src/lexer/scan.rs` | 453 |
| `dctx-lex-cs/src/extract/mod.rs` | 421 |
| `dctx-lex-rb/src/extract/references.rs` | 413 |
| `dctx-lex-swift/tests/dctx_lex_swift.rs` | 409 |
| `dctx-lex-cs/src/extract/declarations.rs` | 382 |

`//!` headers of 3–4 lines are present on the reviewed Rust files. Splitting the scanners (string forms vs statement walk) would satisfy the size rule. This note is why C# and Swift are PASS-WITH-NOTES rather than PASS.

## No finding

- **decision 077 / decision 078.** Each crate is `license = "Apache-2.0"`, `#![forbid(unsafe_code)]`, and depends only on `dctx-hash` and `dctx-map-model`. No `git` invocation, no `sha2` production dependency, no new external crate, no `unsafe`.
- **Privacy in errors and logs.** No `println!`, `eprintln!`, `dbg!`, or `log::` / `tracing::` in the five lexer crates. `LexError` and `ParseError` carry path, line, and byte offset, not source text. Signature fields do copy declaration text; that is the fact model, and these paths do not attach an omitted-file body to an error.
- **C# verbatim and interpolated strings.** `lexer/scan.rs:158` masks `@"..."`, `@"..." ` with `""` escapes, `$"..."`, `$@` / `@$`, and `"""` raw bodies, and it tokenizes interpolation holes. The fixture test hides `Decoy`, `Hidden`, `Fake`, and `Raw` and keeps `Flush`. No declaration-level failure turned up for those forms.
- **Swift raw strings and nested comments.** `lexer/strings.rs:8` closes on quote-plus-the-same-hash-count and scans `\#(`, `\##(`, … as holes. `lexer/scan.rs:151` nests `/* */`. No false or missing declaration turned up for those forms.
- **Store edges checked.** Containment edges in these crates use `EdgeKind::Contains` with `Confidence::Exact`, which `EdgeKind::permits` allows. Calls are unresolved `RawReference` values, not `exact` call edges.
- **Vacuous tests.** No `#[ignore]`, `assert!(true)`, `todo!`, or `unimplemented!` in the five crates. The Ruby regexp test is a real assertion with the wrong inputs (finding 1), not an empty assertion.
- **Adapter shape.** `parse_pure` for these five languages copies lexer nodes, edges, and references and refuses `Partial` or any diagnostic (`csharp.rs:15` and the copies). Pure and tree-sitter dispatch both take that path (`adapters/mod.rs:55` and `:86`). They agree with `lex_dispatch.rs:16`, which publishes symbols only for `Completion::Complete`.
