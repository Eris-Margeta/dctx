Historical design record; superseded by [the current documentation](../TESTING.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Code quality review: initial improvements

Measured on a Linux build host, 2026-10-22. The review completed an initial subset of improvements;
the workspace was not pedantic-clean. Default workspace features were checked, excluding
`dctx-desktop`; `dctx-desktop-api` and the Git plugin were included. No dependency, version, manifest or runtime behavior changes.

## Completed

- Documented all 54 missing public library items reported by the separate check:
  35 in `dctx-core`, 19 in `dctx-cli`. Both now deny `missing_docs`, matching the
  other library crates. This checks presence of public rustdoc, not completeness
  of every error contract or example.
- Repaired 14 rustdoc link diagnostics across core, template, cache adapter, map
  parse/scan, facade and CLI. Public docs describe private implementation details
  directly or link to an exported type. No warning suppression or private-item
  rendering was used to make documentation pass.
- Cleared all 88 baseline library pedantic diagnostics in `dctx-hash`: documented
  SHA-256 errors, marked pure return values `must_use`, added hexadecimal digit
  separators, and corrected Markdown. The sole new allow is function-local at
  `crates/dctx-hash/src/compress.rs:19`: FIPS 180-4's a–h variable names stay aligned
  with the specification. Its `reason` is in the source. No blanket pedantic or
  missing-docs allow was added; `clippy.toml` is unchanged.
- Compared all 18 edited Rust files with the parent source: executable tokens
  are unchanged after excluding documentation, lint attributes, hexadecimal
  separators and an optional trailing field comma. All 83 hexadecimal values in
  the two edited SHA-256 implementation files retain their original values.

## Reproduce the checks

Run these commands from the repository root. Results below are from the
original Linux measurement, not a claim about current CI.

| Command | Exit | Meaning |
| --- | ---: | --- |
| `cargo clippy --workspace --exclude dctx-desktop --lib --message-format=json -- -W clippy::pedantic` | 0 | Inventory before and after; warning mode is not a clean gate. |
| `env RUSTFLAGS=-Wmissing-docs cargo check --workspace --exclude dctx-desktop --lib --message-format=json` | 0 | Initial inventory: 54 missing-docs warnings. |
| `env RUSTFLAGS=-Dmissing-docs cargo check --workspace --exclude dctx-desktop --lib` | 0 | Separate strict public-item documentation check. |
| `env RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --exclude dctx-desktop --no-deps` | 0 | Warning-free public documentation, including link validation. |
| `cargo clippy -p dctx-hash --lib -- -D warnings -D clippy::pedantic` | 0 | Strict pedantic gate for the completed first crate. |
| `cargo test -p dctx-hash --lib` | 0 | All 10 existing hash unit/differential tests passed, including NIST vectors. |
| `cargo clippy --workspace --exclude dctx-desktop --all-targets -- -D warnings -A clippy::field_reassign_with_default -A clippy::assertions_on_constants` | 0 | Existing CI Clippy command; these two existing CI exceptions are unchanged. |

Early strict doc attempts exited 101 for the links described above. A diagnostic
run also used `--keep-going` to expose the final broken link. The exact required
command subsequently passed without that flag. No infrastructure exit 255/75
occurred, so retry/fallback was unnecessary. Local checks are `test -s
docs/CODE-QUALITY.md` and `git diff --check` (local workstation/zsh).

## Remaining work and explicit split

The inventory fell from **2,175 to 2,085 diagnostics**; **36 library packages**
still report pedantic warnings. An exit of zero from the inventory does not mean
those warnings were resolved. Do not enable workspace-wide pedantic denial until
the remaining work is reviewed. Keep follow-ups separate and preserve the
current behavior; any allow must be at the smallest applicable item with a
concrete reason.

1. **Public contracts:** 525 `missing_errors_doc`, 609 `must_use_candidate`,
   49 `return_self_not_must_use`, and 224 `doc_markdown` diagnostics. Start at
   `crates/dctx-core/src/budget.rs:93`, `crates/dctx-core/src/budget.rs:11`,
   `crates/dctx-core/src/filter.rs:57`, and `crates/dctx-core/src/datatypes.rs:9`.
   Read each function's actual error paths; assess downstream warning impact
   before adding `must_use`. Existing prose is not a complete `# Errors` contract.
2. **Conversion bounds:** review every truncating, wrapping, signed and precision
   conversion, starting at `crates/dctx-core/src/filter.rs:82` and
   `crates/dctx-core/src/file_classifier/sniff.rs:33`. Use checked conversions
   when equivalent; retain legacy float saturation behavior unless separately
   authorized. An allow needs the local bound or compatibility reason.
3. **Ownership and implementation style:** remaining option combinators, closures,
   formatting allocations, literals, redundant matches, ownership signatures,
   and function/struct size findings. Start at
   `crates/dctx-lex-md/src/lexer/outline.rs:50`,
   `crates/dctx-lex-md/src/lexer/blocks/leaves.rs:120`,
   `crates/dctx-lex-swift/src/extract/declarations.rs:10`, and
   `crates/dctx/src/index/extract.rs:468` (redundant underscore condition flagged
   as `overly_complex_bool_expr`). Do not infer a functional bug solely from a
   diagnostic's wording. Preserve call signatures and parsing semantics.

The following inventory identifies every remaining package and a current
`path:line` entry point. It is a split queue, not a list of resolved warnings.

| Package | Diagnostics | First unresolved location |
| --- | ---: | --- |
| `dctx` | 153 | `crates/dctx/src/index/extract.rs:347` |
| `dctx-adapter-cache` | 140 | `crates/dctx-adapter-cache/src/lib.rs:3` |
| `dctx-adapter-fs` | 26 | `crates/dctx-adapter-fs/src/adapters/source.rs:50` |
| `dctx-adapter-store` | 73 | `crates/dctx-adapter-store/src/adapters/recover.rs:101` |
| `dctx-cli` | 76 | `crates/dctx-cli/src/compat.rs:54` |
| `dctx-core` | 125 | `crates/dctx-core/src/budget.rs:11` |
| `dctx-desktop-api` | 46 | `crates/dctx-desktop-api/src/error.rs:116` |
| `dctx-ed25519` | 92 | `crates/dctx-ed25519/src/compress.rs:6` |
| `dctx-lex-c` | 50 | `crates/dctx-lex-c/src/domain/mod.rs:2` |
| `dctx-lex-cfg` | 30 | `crates/dctx-lex-cfg/src/json.rs:250` |
| `dctx-lex-cs` | 36 | `crates/dctx-lex-cs/src/domain/facts.rs:15` |
| `dctx-lex-go` | 37 | `crates/dctx-lex-go/src/extract/declarations_funcs.rs:44` |
| `dctx-lex-java` | 34 | `crates/dctx-lex-java/src/domain/facts.rs:15` |
| `dctx-lex-js` | 37 | `crates/dctx-lex-js/src/lexer/mod.rs:73` |
| `dctx-lex-kt` | 46 | `crates/dctx-lex-kt/src/domain/facts.rs:15` |
| `dctx-lex-md` | 16 | `crates/dctx-lex-md/src/domain/document.rs:16` |
| `dctx-lex-php` | 14 | `crates/dctx-lex-php/src/domain/mod.rs:2` |
| `dctx-lex-py` | 29 | `crates/dctx-lex-py/src/domain/mod.rs:2` |
| `dctx-lex-rb` | 13 | `crates/dctx-lex-rb/src/domain/mod.rs:2` |
| `dctx-lex-rust` | 10 | `crates/dctx-lex-rust/src/adapters/syntax.rs:3` |
| `dctx-lex-swift` | 29 | `crates/dctx-lex-swift/src/domain/mod.rs:2` |
| `dctx-map-model` | 65 | `crates/dctx-map-model/src/adapters/render.rs:4` |
| `dctx-map-parse` | 70 | `crates/dctx-map-parse/src/lib.rs:3` |
| `dctx-map-query` | 61 | `crates/dctx-map-query/src/adapters/cache.rs:35` |
| `dctx-map-resolve` | 46 | `crates/dctx-map-resolve/src/domain/collision.rs:59` |
| `dctx-map-scan` | 28 | `crates/dctx-map-scan/src/adapters/content.rs:17` |
| `dctx-map-segment` | 60 | `crates/dctx-map-segment/src/adapters/mod.rs:19` |
| `dctx-map-store` | 36 | `crates/dctx-map-store/src/lib.rs:3` |
| `dctx-pack` | 15 | `crates/dctx-pack/src/engine.rs:64` |
| `dctx-plugin-distribution` | 126 | `crates/dctx-plugin-distribution/src/domain/admission.rs:44` |
| `dctx-plugin-git` | 95 | `plugins/dctx-plugin-git/src/adapters/link.rs:4` |
| `dctx-plugin-host` | 137 | `crates/dctx-plugin-host/src/domain/authority.rs:51` |
| `dctx-plugin-platform` | 22 | `crates/dctx-plugin-platform/src/admission.rs:2` |
| `dctx-plugin-protocol` | 116 | `crates/dctx-plugin-protocol/src/lib.rs:5` |
| `dctx-ports` | 20 | `crates/dctx-ports/src/lib.rs:1` |
| `dctx-template` | 76 | `crates/dctx-template/src/binary/mod.rs:125` |

Full diagnostic counts by lint (without the `clippy::` prefix):

| Lint | Diagnostics |
| --- | ---: |
| `must_use_candidate` | 609 |
| `missing_errors_doc` | 525 |
| `doc_markdown` | 224 |
| `cast_possible_truncation` | 179 |
| `unreadable_literal` | 94 |
| `map_unwrap_or` | 93 |
| `needless_pass_by_value` | 50 |
| `return_self_not_must_use` | 49 |
| `format_push_string` | 38 |
| `redundant_closure_for_method_calls` | 25 |
| `match_same_arms` | 22 |
| `single_match_else` | 14 |
| `manual_let_else` | 13 |
| `unnecessary_wraps` | 11 |
| `too_many_lines` | 11 |
| `struct_excessive_bools` | 10 |
| `wildcard_imports` | 10 |
| `elidable_lifetime_names` | 10 |
| `case_sensitive_file_extension_comparisons` | 9 |
| `unused_self` | 8 |
| `cast_possible_wrap` | 8 |
| `assigning_clones` | 6 |
| `cast_precision_loss` | 5 |
| `needless_continue` | 5 |
| `naive_bytecount` | 5 |
| `cast_sign_loss` | 4 |
| `trivially_copy_pass_by_ref` | 4 |
| `similar_names` | 4 |
| `if_not_else` | 4 |
| `ref_option` | 4 |
| `ignored_unit_patterns` | 4 |
| `unnested_or_patterns` | 3 |
| `missing_fields_in_debug` | 3 |
| `struct_field_names` | 2 |
| `semicolon_if_nothing_returned` | 2 |
| `default_trait_access` | 2 |
| `bool_to_int_with_if` | 2 |
| `nonminimal_bool` | 2 |
| `format_collect` | 2 |
| `unnecessary_debug_formatting` | 2 |
| `many_single_char_names` | 1 |
| `uninlined_format_args` | 1 |
| `doc_link_with_quotes` | 1 |
| `verbose_bit_mask` | 1 |
| `no_effect_underscore_binding` | 1 |
| `option_option` | 1 |
| `implicit_hasher` | 1 |
| `overly_complex_bool_expr` | 1 |

The complete per-diagnostic queue is retained in the private working notes as
`pedantic-after-summary.json`
(SHA-256 `00e555f9919fd2bfbb0f576760bd0aefa4753d69bc3a975fd921d1ddd85cf896`). Each entry records lint, message, path and line. Raw
before/after JSON and strict-check logs are alongside it. Re-run the JSON
inventory after each follow-up because line numbers and toolchain lints drift.

Coverage limits: Linux default features only; desktop, optional-feature matrices,
other targets, doc-test execution and a semantic review of every existing public
comment were not covered by the documentation gates. Full workspace tests are
left to the exact-commit CI run, recorded in the check results. This work makes no
security audit, release, publication or production-readiness claim.
