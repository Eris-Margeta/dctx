# dctx test-gap audit — DX-GAPS

2026-10-18; independent, static inspection. No builds, tests, application code, or reproducers were run. Only this report was edited. Findings below distinguish code-backed failures from proposed tests; passing counts are not evidence of coverage.

## Inventory and method

Inventory: all 40 non-fixture packages in `docs/CRATE-MAP.md`, including the two standalone tool workspaces (38 packages belong to the root workspace). For each package, enumerate its `.rs` files, excluding directories named `target`, `fixtures`, `fixture`, or `node_modules`; sum each file's `wc -l` and `grep -c '#\[test\]'`. Counts include source, integration tests, helpers, build scripts and Rust corpus files outside those exclusions. They exclude doctests and non-Rust tests; macro-generated cases are not expanded. This is tests per **total Rust lines**, not tests per production line or measured coverage. Python's proptest function attributes count, but its generated cases do not.

Files were inspected in disjoint ranges of at most 120 lines. All packages received counts and targeted standards searches; the bottom ten received public extractor/store API and selected implementation/test inspection. This is not a correctness certification of the other 30 packages. Standards: `docs/standards/{RUST-STANDARDS,CODE-GUIDELINES}.md`; decision 078 precedence and the later plugin exception: `docs/design/ARCHITECTURE-DECISIONS.md:24,58,90`.

| Density rank | Package (directory differs where shown) | Literal tests | Rust lines | Tests / 1,000 lines |
|---:|---|---:|---:|---:|
| 1 | dctx-lex-cs | 12 | 3458 | 3.470 |
| 2 | dctx-lex-kt | 17 | 4087 | 4.160 |
| 3 | dctx-lex-java | 18 | 4320 | 4.167 |
| 4 | dctx-lex-rb | 16 | 3586 | 4.462 |
| 5 | dctx-lex-py | 19 | 4058 | 4.682 |
| 6 | dctx-lex-c | 18 | 3841 | 4.686 |
| 7 | dctx-lex-php | 11 | 2234 | 4.924 |
| 8 | dctx-lex-go | 14 | 2793 | 5.013 |
| 9 | dctx-adapter-store | 24 | 4606 | 5.211 |
| 10 | dctx-lex-swift | 11 | 2056 | 5.350 |
| 11 | dctx-lex-cfg | 15 | 2627 | 5.710 |
| 12 | dctx-map-segment | 34 | 5631 | 6.038 |
| 13 | architecture-bench (`tools/architecture-bench`) | 12 | 1796 | 6.682 |
| 14 | dctx-desktop (`apps/dctx-desktop/src-tauri`) | 5 | 706 | 7.082 |
| 15 | dctx-adapter-cache | 53 | 7411 | 7.152 |
| 16 | dctx-lex-js | 41 | 4771 | 8.594 |
| 17 | dctx-map-model | 19 | 2169 | 8.760 |
| 18 | dctx-map-resolve | 30 | 3397 | 8.831 |
| 19 | dctx-map-query | 52 | 5867 | 8.863 |
| 20 | dctx-map-store | 22 | 2268 | 9.700 |
| 21 | dctx-template | 84 | 8378 | 10.026 |
| 22 | plugin-index (`tools/plugin-index`) | 16 | 1591 | 10.057 |
| 23 | dctx | 102 | 9940 | 10.262 |
| 24 | dctx-adapter-fs | 27 | 2525 | 10.693 |
| 25 | dctx-plugin-platform | 17 | 1493 | 11.386 |
| 26 | dctx-plugin-git (`plugins/dctx-plugin-git`) | 42 | 3673 | 11.435 |
| 27 | dctx-plugin-protocol | 77 | 6302 | 12.218 |
| 28 | dctx-cli | 90 | 7194 | 12.510 |
| 29 | dctx-map-parse | 76 | 6048 | 12.566 |
| 30 | dctx-desktop-api | 30 | 2154 | 13.928 |
| 31 | dctx-plugin-distribution | 72 | 4954 | 14.534 |
| 32 | dctx-lex-rust | 47 | 2876 | 16.342 |
| 33 | dctx-ed25519 | 16 | 975 | 16.410 |
| 34 | dctx-plugin-host | 92 | 5452 | 16.875 |
| 35 | dctx-ports | 13 | 738 | 17.615 |
| 36 | dctx-hash | 10 | 535 | 18.692 |
| 37 | dctx-lex-md | 48 | 2446 | 19.624 |
| 38 | dctx-pack | 36 | 1750 | 20.571 |
| 39 | dctx-map-scan | 120 | 4131 | 29.049 |
| 40 | dctx-core | 202 | 5383 | 37.526 |

## Findings, ordered by severity

**High — F1: store confinement does not apply to reads or existing write targets.** `crates/dctx-adapter-store/src/adapters/stdfs.rs:51,147,195`: `abs` only validates the relative spelling; `read` follows symlinks and `write` opens/truncates an existing target without containment validation. Minimal reproducer: open a temporary cache with `StdFs::open`, create `cache/segments` as a symlink to a second temporary directory containing a sentinel, then invoke the public `StoreFs::read("segments/sentinel")`; it returns outside bytes. A leaf symlink similarly permits `write` to truncate an outside sentinel. Fix: refuse symlink components and validate existing targets before access; define and test the concurrent replacement threat model rather than claiming that lexical checks provide confinement. Close with S1/P01 below.

**High — F2: Kotlin nested templates bypass the depth ceiling before extraction.** `crates/dctx-lex-kt/src/lexer/scan.rs:167,218,258`: `scan_string` calls `scan_interp`, whose depth restarts at 1; its call to `step` can recurse into another string with the unchanged limit. Exact missing regression: start with owned string `s = "0"`, repeat `s = format!("\"${{{s}}}\"")` three times, then extract `format!("val x = {s}")` with `max_depth: 2`; require `TooDeep`, with one nesting accepted. Interpolation delimiters are consumed rather than emitted, so later bracket pairing cannot enforce this recursion bound. Large repetitions below 8 MiB can exhaust the process stack. Fix: carry a cumulative scanner recursion depth or use an explicit bounded frame stack. Close with KT1/P02.

**High — F3: a pin is invisible to another store instance's collector.** `crates/dctx-adapter-store/src/adapters/store.rs:84,177` and `src/adapters/gc.rs:194`: every `open` allocates a new coordinator; GC marks only its own coordinator's leases. Deterministic reproducer using existing helpers (unwrap successful setup calls): `fs = MemoryFs::posix(); a = open(fs.clone()); b = open(fs); a.publish(req("main", 1, segment("held"))); p = a.pin("main"); b.retire_root("main"); b.gc(GcBudget { objects: 64 }); a.read_segment(&p, p.segments()[0].digest())` loses the pinned bytes. Existing P3 interleavings share one `Arc<SegmentStore>`, not two independent coordinators. Fix: share/persist lease authority across opens, or enforce exclusive store ownership and refuse the second instance. Close with S3/P03.

**High — F4: corrupt disk objects allocate before validation.** `crates/dctx-adapter-store/src/adapters/stdfs.rs:195` and `src/adapters/store.rs:348,361,399`: pointer, manifest and segment contents are fully read before envelope/length validation. Minimal hostile input: a sparse 1 GiB `roots/main/current` under a store configured with a small cap; `pin("main")` attempts the full allocation before rejecting the pointer. Fix: bounded reads with fixed pointer size and checked manifest/segment ceilings, including growth between metadata and read; assert the read bound using an instrumented port rather than allocating 1 GiB in the normal test suite. Close with S2/P01.

**Medium — F5: decision 078 policy is not enforced by digest equality tests.** `crates/dctx-map-scan/src/domain/scanned_file.rs:6,13` and `crates/dctx-map-parse/src/domain/identity.rs:4,9` use production `sha2`; their manifests place it in normal dependencies (`Cargo.toml:12` and `:33`). Known SHA-256 vectors pass equally with the forbidden provider. Exact missing test: a dependency/source gate rejects production `sha2` in these crates while accepting dev-only oracle use in `dctx-hash`. Fix: use `dctx-hash` and preserve digest parity; audit the standalone benchmark's `tools/architecture-bench/src/pin.rs:7` separately from the runtime closure. P04 covers the two runtime crates.

**Medium — F6: first-party test code contains unsafe.** `crates/dctx-map-query/tests/no_git.rs:220,221,239` mutates process environment in unsafe blocks; its private mutex cannot serialize unrelated test threads reading that environment. Exact missing gate: include integration-test source in the no-unsafe check. Fix: launch a child with `Command::env` for the git trap, preserving its existing positive control, and forbid unsafe in that test crate. P04.

**Medium — F7: concurrency assertions can pass without exercising concurrent reads.** `crates/dctx-adapter-store/tests/concurrent.rs:17` can finish the writer and set `stop` before either reader enters its loop. In `tests/p2_publish_crash_matrix.rs:35,49`, readiness means only that a thread started; an empty error list does not prove `observe` succeeded once. Missing test condition: deterministic acknowledgements of successful reads before and during the controlled publish/GC schedule. Fix: use barriers/channels and assert nonzero successful observations; retain the existing final-generation and crash-cut assertions. P03.

**Low — F8: size/header standards remain unenforced.** All ten lowest-density crates contain files over 300 lines: CS 7, Kotlin 7, Java 6, Ruby 7, Python 4, C 7, PHP 2, Go 3, store 5, Swift 1 (tests included). Examples: `crates/dctx-lex-cs/src/lexer/scan.rs:301` (464 lines), `crates/dctx-adapter-store/src/adapters/publish.rs:301` (510), `crates/dctx-lex-swift/tests/dctx_lex_swift.rs:301` (409). Nonempty files without any `//!` include `crates/dctx-cli/src/platform.rs:1`, `apps/dctx-desktop/src-tauri/build.rs:1`, and its `src/lib.rs:1`; three zero-line Ed25519 placeholder files also lack headers but are not executable code. Missing gate: count source and tests and require contextual module headers. Fix: split oversized concepts and add the required purpose/spec/dependency/consumer headers in separate, explicitly owned maintenance packets; do not inflate tests to improve this ratio.

Other requested checks: **no finding** for licence declarations (all 40 manifest declarations are Apache-2.0), ignored tests (no `#[ignore]` matches), or git/IO calls in the inspected nine pure extractor APIs. No excluded/omitted-source or content-in-error/log leak was established in those APIs: bytes are supplied by the caller and their typed errors carry categories/counts, not source text. This does not establish the upstream selection-before-read invariant; F1 is a concrete adapter boundary failure. **No finding** of a newly introduced external dependency can be established without a change baseline; existing approved exceptions are not automatically violations. Header searches establish presence only, not that every header meets the full 3–6-line contract. No additional concrete correctness finding is claimed for the remaining packages.

## Three next goals for each of the ten weakest crates

Each numbered item below is an exact missing test/behaviour goal, not a claim that all nearby behavior is untested. `P` = property/invariant tests needed; `H` = hostile-input tests needed; `C` = crash/concurrency tests needed. Lexer concurrency tests are not a priority for these stateless APIs. Python already has proptest; its gaps concern semantic properties and scale. Existing fixture-prefix loops and small random-byte loops do not replace generated valid-language tests with nonempty positive controls.

Packet ownership and runnable `ok` predicates are defined in the final table. For **each** item, add the named test as a top-level function in that crate's `tests/gaps.rs`; its item-specific predicate is `test -s crates/<crate>/tests/gaps.rs && cargo test -p <crate> --test gaps <function_name> -- --exact`, substituting the exact crate and function named below. Grouped acceptance must also verify all required named tests are selected and pass (zero selected tests is failure); no ignored tests, unconditional early returns, or acceptance of arbitrary errors. Use first-party deterministic generators where proptest is not already a dev dependency. Each proposed packet runs its targeted gate and the full workspace suite once at the end; this audit runs neither.

### dctx-lex-cs — P, H → P05

- CS1 `raw_delimiter_matrix`: `src/lexer/scan.rs:196,265`. Generate raw strings with 1–4 `$` prefixes, varying quote counts and brace runs, nesting a real `live()` interpolation beside literal `ghost()`/`class Ghost {}`. Assert exact call names and absence of ghost facts, including truncated closers; the current basic/prefix corpus does not cross-product these delimiters. Fix any mismatch in the scanner without weakening the positive control.
- CS2 `reopened_namespace_ownership`: `src/extract/declarations.rs:46`. Use `namespace N { partial class A { void F(){one();} } } namespace N { partial class A { void F(){two();} } }`; require unique ids, correct containment and reference source ids for both overload occurrences, then repeat with UTF-8 names and line shifts. Existing golden graph checks use a single namespace occurrence; add this generated identity property and repair remapping if needed.
- CS3 `hostile_limit_boundaries`: `src/lib.rs:41`, `src/domain/limits.rs:19`. Exercise dense valid token streams up to the 8 MiB ceiling, fact limit N/N+1, zero limits and more than 64 malformed spans; pin exact refusal variants and diagnostic bounds with valid nonempty controls. Current 40 random buffers of at most 300 bytes do not reach these boundaries. Keep resource checks deterministic, without wall-clock assertions.

### dctx-lex-kt — P, H; subprocess crash regression → P02

- KT1 `nested_template_depth`: F2's cumulative interpolation generator; test depth 0, 1, 2 and the ceiling, with accepted shallower controls. Run a larger adversarial recursion case in a child so stack abort is reported as a failed test, not lost evidence; fix cumulative accounting.
- KT2 `newline_continuation_equivalence`: `src/lexer/scan.rs:13,166`. Generate equivalent multiline properties using `?:`, safe calls and custom getters beside ordinary newline-separated declarations. Require the same names/calls/owners across formatting variants and require the following declaration to survive; existing basic golden output is not an equivalence oracle. Fix semicolon insertion or property stopping where this fails.
- KT3 `unicode_template_truncations`: `src/lexer/scan.rs:181,287`. Build valid strings with escaped multibyte characters, raw templates and backtick names; truncate at every byte and require UTF-8 refusal only at non-boundaries, otherwise valid spans/diagnostics and deterministic output. The current fixture-prefix test unconditionally unwraps each prefix and slices `source[..len]`; it supplies no UTF-8 boundary matrix. Fix boundary handling or the test harness as needed.

### dctx-lex-java — P, H → P06

- J1 `unicode_escape_precision`: `src/lexer/scan.rs:36`. Inputs include literal bytes `cl\u0061ss A {}` and Unicode-escaped comment delimiters/newlines, with ordinary `class A {}` as control. Require correct lexical translation or an explicit Partial/unsupported result rather than silent Complete omission; the golden corpus does not pin this boundary. Add the support or honest abstention required by the parser contract.
- J2 `generated_graph_ownership`: `src/extract/mod_cursor.rs:164`, `tests/dctx_lex_java.rs:423`. Generate overloaded methods, nested anonymous classes and `@Test` methods with distinct calls; assert all unique ids, owner paths, edge endpoints and reference sources belong to the output graph. The common invariant helper currently checks lengths/kinds but not these graph identities; broaden the oracle, preserving existing exact golden assertions.
- J3 `hostile_token_and_diagnostic_bounds`: `src/lib.rs:41`, `src/lexer/scan.rs:22`. Cross dense near-ceiling valid Java with small fact/depth limits and >64 damaged spans; require bounded retained diagnostics, exact errors and nonempty accepted controls. Existing deep-bracket tests and 40 small random buffers do not test wide token/diagnostic allocation. Repair pre-limit allocation where exposed.

### dctx-lex-rb — P, H → P07

- RB1 `unicode_heredoc_terminators`: `src/lexer/heredoc.rs:58`. Use a quoted delimiter `'É'`, a literal body containing declaration-shaped text, the matching UTF-8 closing line and a following real method; repeat with several queued heredocs and truncations. `quoted_term` converts individual bytes with `char::from`, so delimiter identity needs a regression beyond the existing ASCII heredoc golden. Decode delimiter characters correctly or diagnose unsupported names explicitly.
- RB2 `visibility_scope_restoration`: `src/extract/members.rs:22`, `src/extract/mod.rs:89`. Generate nested modules/classes around `private`, `module_function`, inline visibility and `attr_accessor`; require expected public flags and restoration when the nested scope ends. The basic fixture checks individual flags, not nested transition sequences; add a state/invariant oracle and repair restoration where needed.
- RB3 `hostile_literal_depth`: `src/lib.rs:41`, `src/lexer/heredoc.rs:108`. Compose deeply nested percent literals/interpolations and many pending heredocs under narrowed limits; assert depth refusal, diagnostic cap and a retained real declaration in accepted siblings. Existing random tests mostly hit UTF-8 refusal and do not establish cumulative nesting bounds. Carry explicit bounded state if recursion or pending buffers escape the limits.

### dctx-lex-py — P (extend existing), H → P08

- PY1 `formatted_string_semantic_properties`: `src/lexer/strings.rs:66`. Generate valid f-strings combining nested replacement fields, format specifications, escaped braces and real `live()` calls beside literal `ghost()` text. Require exact call ownership, not just repeatability; `tests/robustness.rs:184` generates short token noise whose repeated refusals also satisfy the property. Preserve the existing proptests and add a semantic oracle with guaranteed successful controls.
- PY2 `indentation_composition`: `src/lexer/indent.rs:10,62`. Generate nested suites mixed with parenthesized continuations, blank/comment lines and CRLF; assert equivalent valid space/tab layouts retain identical names/owners, and intentionally inconsistent dedents become Partial with the right location. Existing separate fixtures do not test this composition; repair logical-indent transitions if the property fails.
- PY3 `wide_input_budget_and_error_redaction`: `src/lib.rs:40`, `src/domain/limits.rs:18`. Test near-8-MiB valid statements, fact-limit boundaries, and malformed source containing a sentinel with invalid paths/UTF-8; require exact bounded refusals and no sentinel in Display/Debug errors. Existing generated buffers are under 512 bytes. Add deterministic resource-bound evidence rather than timing thresholds.

### dctx-lex-c — P, H → P09

- C1 `splice_coordinate_properties`: `src/lexer/mod.rs:150`. Generate LF/CRLF backslash splices next to UTF-8 identifiers, comments, directives and malformed closers; compare fact names to the logical unspliced control while asserting locations refer to the original physical file. Existing splice examples do not cover this coordinate cross-product. Fix SourceMap conversion where it loses physical offsets.
- C2 `comparison_angle_depth`: `src/lexer/mod.rs:216`. Precede nested templates by many `>` comparisons and repeat with `<` comparisons under a small limit; require true generic nesting to refuse and valid shallow expressions to remain accepted or honestly Partial. The shared signed angle counter increments/decrements on punctuation regardless of expression context; no existing test isolates counter cancellation. Fix context-sensitive accounting rather than relaxing the depth oracle.
- C3 `macro_boundary_properties`: `src/extract/mod.rs:36`. Generate interleaved `#define/#undef`, conditional branches, include aliases and ordinary direct calls; require macro-hidden facts to abstain with diagnostics and unchanged direct calls to survive. Existing basic/header/macro fixtures pin individual examples rather than directive-state transitions. Add a small independent expected-state model and preserve positive controls.

### dctx-lex-php — P, H → P10

- PHP1 `template_region_properties`: `src/lexer/strings.rs:15`, `tests/dctx_lex_php.rs:205`. Generate alternating HTML/PHP regions with `?>` inside quoted literals/comments and declaration-shaped HTML. Require only authorized PHP-region facts and a following real call; assert exact output, not merely successful parsing. Existing single template fixture does not cross-product region closers and literal boundaries; repair masking if exposed.
- PHP2 `heredoc_and_interpolation_depth`: `src/lexer/strings.rs:21,83`. Generate quoted/dynamic strings, nowdocs and indented heredoc terminators, including terminator-prefix identifiers and nested interpolation at N/N+1. Require no literal decoy facts, the correct following declaration, and typed depth/unterminated results. Existing shallow brace refusal does not exercise string frame depth; preserve opaque-literal semantics.
- PHP3 `grouped_alias_scope_properties`: `src/extract/references.rs:46`. Compose two bracketed namespaces with grouped function/const/type imports, alias reuse and trait adaptation blocks; assert exact names, aliases, kinds, owners and no alias leakage to the next namespace. The current blocks test checks presence of selected names, not complete scoped import records. Add exact scoped records and correct state leakage if found.

### dctx-lex-go — P, H → P11

- GO1 `compound_type_dependencies`: `src/extract/references.rs:39,75`. Use `package p; type Key int; type Value int; var M map[Key]Value; var G Box[Value]` and a direct `var V Value` control; require `uses-type` for key/value/type arguments or explicit partial precision where unsupported. Current `skip_type` skips map keys and `named_type` skips type arguments wholesale; golden tests require selected names but do not assert this dependency matrix. Implement recursive bounded type extraction or honest abstention.
- GO2 `generated_owner_identity`: `tests/dctx_lex_go.rs:231`, `src/extract/mod_helpers.rs:282`. Generate same-named methods on distinct receiver types, repeated functions and Unicode exported names; assert unique ids, correct path/containment/reference endpoints and stable ids under line shifts. The common invariant helper currently checks spans/fingerprint lengths, not graph ownership. Strengthen it without removing existing fixtures.
- GO3 `hostile_width_and_refusal_oracle`: `src/lib.rs:41`, `tests/dctx_lex_go.rs:253`. Exercise maximal valid import/declaration width with narrow limits, malformed bracket streams and a source sentinel; require exact errors, diagnostic cap and no source in errors. The current stability helper accepts any repeated error string, and random buffers are at most 300 bytes. Add valid positive controls and reject unexpected refusals.

### dctx-adapter-store — P, H, C → P01 and P03

- S1 `symlink_confinement`: F1; Unix fixtures must cover parent and leaf symlinks for `read`, `write`, `len` and publication, plus ordinary in-root success. Assert outside sentinels remain unread/unmodified through a spying boundary where needed; return a typed refusal. P01.
- S2 `bounded_corrupt_reads`: F4; instrument `StoreFs` to reject requests past pointer/manifest/segment maxima before materializing bytes, including metadata/read growth. Assert valid objects still pin/read; add corrupt-length recovery controls. P01.
- S3 `independent_store_pin_gc`: F3's deterministic two-open schedule, then controlled concurrent publish/pin/GC/recovery schedules with acknowledged successful observations (F7). Require retained bytes until release, or an explicit second-open refusal; include crash interruption and the existing OmitLeases falsifying control. P03.

### dctx-lex-swift — P, H → P12

- SW1 `raw_delimiter_properties`: `src/lexer/strings.rs:8`. Generate hash/quote delimiter counts, escaped interpolation and nested real calls beside literal decoys, including malformed/truncated closes and large delimiter prefixes. Require exact calls and cumulative depth refusal with accepted controls; existing string golden and shallow depth tests are not a delimiter matrix. Keep allocations bounded while scanning prefixes.
- SW2 `overload_remap_properties`: `src/extract/mod.rs:156`. Generate overloads inside repeated extensions and nested types, with distinct calls before/after line shifts; assert that every remapped edge/reference source names its intended declaration. The existing overload fixture checks uniqueness and stable node ids, not the complete source mapping across repeated scopes. Correct remapping if the property fails.
- SW3 `hostile_fact_and_diagnostic_boundaries`: `src/extract/mod.rs:133,145`, `tests/dctx_lex_swift.rs:326`. Generate valid wide programs at byte/fact boundaries and >64 malformed spans; require exact errors and diagnostic bounds with nonempty accepted controls. The current arbitrary-byte/ASCII loop validates only `Ok` results, so systematic erroneous refusal of those inputs is invisible. Add an acceptance oracle for generated valid cases rather than accepting every error.

## Ranked packets

These 12 proposals close the 30 numbered gaps above; P04 additionally closes F5/F6. They are proposed work, not dispatched plan entries. All paths are relative and explicit; `own` includes only the listed files. Test files use `//!` headers and stay under 300 lines; split a packet before dispatch if its oracle needs more space. No proposal authorizes new external dependencies. Commands below are future packet predicates, never commands executed by this audit. The full-suite closeout is `cargo test --workspace` once after targeted checks; standalone tool validation is separate when a packet actually changes a tool.

| Rank / id | Exact next packet goal | own files | ok predicate |
|---|---|---|---|
| 1 / P01 | Close S1/S2: confine store accesses and bound corrupt-object reads, preserving valid durable publication. | `crates/dctx-adapter-store/src/adapters/stdfs.rs`, `crates/dctx-adapter-store/src/adapters/store.rs`, `crates/dctx-adapter-store/src/ports/mod.rs`, `crates/dctx-adapter-store/src/adapters/memory.rs`, `crates/dctx-adapter-store/tests/gaps.rs` | `test -s crates/dctx-adapter-store/tests/gaps.rs && cargo test -p dctx-adapter-store --test gaps` |
| 2 / P02 | Close KT1–KT3: cumulative template depth, continuation equivalence, UTF-8 truncation oracle. | `crates/dctx-lex-kt/src/lexer/scan.rs`, `crates/dctx-lex-kt/src/lexer/mod.rs`, `crates/dctx-lex-kt/src/extract/members.rs`, `crates/dctx-lex-kt/tests/gaps.rs` | `test -s crates/dctx-lex-kt/tests/gaps.rs && cargo test -p dctx-lex-kt --test gaps` |
| 3 / P03 | Close S3/F7: preserve pins across independent opens or refuse them, and prove readers execute in crash/GC schedules. Depends on P01; shared store changes must be serialized. | `crates/dctx-adapter-store/src/adapters/store.rs`, `crates/dctx-adapter-store/src/adapters/gc.rs`, `crates/dctx-adapter-store/tests/concurrent.rs`, `crates/dctx-adapter-store/tests/p2_publish_crash_matrix.rs`, `crates/dctx-adapter-store/tests/gaps.rs` | `test -s crates/dctx-adapter-store/tests/gaps.rs && cargo test -p dctx-adapter-store` |
| 4 / P04 | Replace runtime sha2 in scan/parse with first-party SHA-256, preserve digest vectors, remove unsafe environment mutation and gate both policies. | `crates/dctx-map-scan/Cargo.toml`, `crates/dctx-map-scan/src/domain/scanned_file.rs`, `crates/dctx-map-parse/Cargo.toml`, `crates/dctx-map-parse/src/domain/identity.rs`, `crates/dctx-map-query/tests/no_git.rs`, `crates/dctx-map-query/tests/policy_gaps.rs` | `test -s crates/dctx-map-query/tests/policy_gaps.rs && cargo test -p dctx-map-scan -p dctx-map-parse -p dctx-map-query` |
| 5 / P05 | Close CS1–CS3 with semantic/property and hostile-boundary oracles; repair exposed scanner/namespace identity failures. | `crates/dctx-lex-cs/src/lexer/scan.rs`, `crates/dctx-lex-cs/src/extract/mod.rs`, `crates/dctx-lex-cs/src/extract/declarations.rs`, `crates/dctx-lex-cs/tests/gaps.rs` | `test -s crates/dctx-lex-cs/tests/gaps.rs && cargo test -p dctx-lex-cs --test gaps` |
| 6 / P06 | Close J1–J3: Unicode lexical precision, generated graph identity, wide hostile input. | `crates/dctx-lex-java/src/lexer/scan.rs`, `crates/dctx-lex-java/src/extract/mod.rs`, `crates/dctx-lex-java/src/extract/mod_cursor.rs`, `crates/dctx-lex-java/tests/gaps.rs` | `test -s crates/dctx-lex-java/tests/gaps.rs && cargo test -p dctx-lex-java --test gaps` |
| 7 / P07 | Close RB1–RB3: UTF-8 heredocs, visibility state transitions, cumulative literal limits. | `crates/dctx-lex-rb/src/lexer/heredoc.rs`, `crates/dctx-lex-rb/src/lexer/scan.rs`, `crates/dctx-lex-rb/src/extract/mod.rs`, `crates/dctx-lex-rb/src/extract/members.rs`, `crates/dctx-lex-rb/tests/gaps.rs` | `test -s crates/dctx-lex-rb/tests/gaps.rs && cargo test -p dctx-lex-rb --test gaps` |
| 8 / P08 | Close PY1–PY3 by extending semantic properties and wide-input/redaction controls. | `crates/dctx-lex-py/src/lexer/strings.rs`, `crates/dctx-lex-py/src/lexer/indent.rs`, `crates/dctx-lex-py/src/extract/mod.rs`, `crates/dctx-lex-py/tests/gaps.rs` | `test -s crates/dctx-lex-py/tests/gaps.rs && cargo test -p dctx-lex-py --test gaps` |
| 9 / P09 | Close C1–C3: physical source-map properties, angle-depth isolation, macro state oracle. | `crates/dctx-lex-c/src/lexer/mod.rs`, `crates/dctx-lex-c/src/extract/mod.rs`, `crates/dctx-lex-c/src/lexer/preprocessor.rs`, `crates/dctx-lex-c/tests/gaps.rs` | `test -s crates/dctx-lex-c/tests/gaps.rs && cargo test -p dctx-lex-c --test gaps` |
| 10 / P10 | Close PHP1–PHP3: template regions, literal frame bounds, exact scoped aliases. | `crates/dctx-lex-php/src/lexer/mod.rs`, `crates/dctx-lex-php/src/lexer/strings.rs`, `crates/dctx-lex-php/src/extract/references.rs`, `crates/dctx-lex-php/tests/gaps.rs` | `test -s crates/dctx-lex-php/tests/gaps.rs && cargo test -p dctx-lex-php --test gaps` |
| 11 / P11 | Close GO1–GO3: compound type dependencies, graph ownership, exact hostile refusals. | `crates/dctx-lex-go/src/extract/references.rs`, `crates/dctx-lex-go/src/extract/mod_helpers.rs`, `crates/dctx-lex-go/src/lexer/mod.rs`, `crates/dctx-lex-go/tests/gaps.rs` | `test -s crates/dctx-lex-go/tests/gaps.rs && cargo test -p dctx-lex-go --test gaps` |
| 12 / P12 | Close SW1–SW3: delimiter properties, overload source remapping, nonvacuous valid-input boundary checks. | `crates/dctx-lex-swift/src/lexer/strings.rs`, `crates/dctx-lex-swift/src/extract/mod.rs`, `crates/dctx-lex-swift/tests/gaps.rs` | `test -s crates/dctx-lex-swift/tests/gaps.rs && cargo test -p dctx-lex-swift --test gaps` |
