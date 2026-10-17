# D3 compatibility obligations

DX-D3-01 freezes today's legacy behaviour as fixed bytes and facts (D3-LEGACY-REMOVAL.md §3 and §5 packet D3-01; ARCHITECTURE-PROPOSAL.md row DX-D3). It does not choose a breaking change and does not delete modules. Deletion waits on Eris decision 1 (the compatibility announcement gate).

Pack facts use injected timestamps (`1700000000000` plus the offsets in the fixtures). Tests read fixtures with `include_str!`. They do not call legacy IO and they do not write oracles.

Legacy `CompiledTemplate::serialize` walks `HashMap` order (`crates/dctx-core/src/legacy/template.rs:257`). `dctx_template` sorts keys (`crates/dctx-template/src/v1/compiled.rs:53`). `legacy-template.hex` pins the sorted encoding (`SORTED`) and one unsorted order (`UNSORTED`, zeta then alpha). Decode accepts both. Encode emits `SORTED`.

## §3 family map

| §3 family | Obligation |
|---|---|
| purity.rs forbidden-IO scan, legacy control, manifest deps | needs Eris decision 6 |
| templates.rs asset conflicts, syntax, counts, critical rules | `crates/dctx-core/tests/templates.rs` (asset tests stay until installer ownership moves) |
| legacy_parity.rs normal and debug trees: path, kind, decision, ignored, size, time, content, binary class, child order, on the ignored/omitted/binary/SVG/nested corpus | `crates/dctx-pack/tests/compatibility.rs:normal_scan_matches_pinned_legacy_facts`; `debug_scan_matches_pinned_legacy_facts`; bytes in `fixtures/legacy-binary.hex` |
| content_reader.rs ignored, omitted and subtree read denial, plus classification | same two tests (`NOT_READ`, `UNTOUCHED`, PNG row). A read spy that fails when a denied byte is opened is the positive control beside the README.md read |
| walker.rs excluded recursion, included and debug metadata, omit | same two tests. `filter_tree` pruning has no pack assertion: needs Eris decision 3 |
| ignore.rs patterns, negation, depth, layers, errors, diagnostics | `crates/dctx-core/src/selection/policy.rs` and `selection/rule.rs` unit tests, exercised by the corpus (`.env`, `private/`, `!private/keep/`, `node_modules/`). Cache stats and the pattern-matching bench: needs Eris decision 7 |
| omit_content.rs lockfiles, SVG, env, depth | corpus rows for `Cargo.lock` (OmitContent) and `assets/icon.svg` (`*.svg`, content absent). Unreadable omit file skip versus refuse: needs Eris decision 4 |
| template.rs substitution, parse, validation, compile, magic, roundtrip | `crates/dctx-template/tests/v1_parity.rs`; `legacy_envelopes.rs:compiled_bytes_match_sorted_fixture_and_accept_legacy_order` |
| llm_formatter.rs headers, version, diff, tree-only, ids, indentation, project variables, changed-content-only, ignored markers, variable safety | `legacy_envelopes.rs:snapshot_body_matches_legacy_layout`; `tree_only_body_matches_legacy_layout`; `diff_fixture_pins_the_legacy_diff_envelope`. Whether those bytes stay the product promise: needs Eris decision 3 |
| reader.rs and writer.rs magic, truncation, nesting, version, timestamp, offsets, node encoding | needs Eris decision 2 |
| state.rs fresh, valid, corrupt, recreation timestamps, export deletion | needs Eris decision 4 |
| registry.rs FNV stability, Unicode, case, hex, separator; config.rs aliases and implicit paths | registry and historic metadata: needs Eris decision 2. Implicit global config: needs Eris decision 4 |
| git_safety.rs missing ignore stays missing; ignore_debug.rs has no linked tests | needs Eris decision 5 (installer and benchmark Git). ignore_debug stays unlinked until Eris decision 1 |
| benches/pattern_matching.rs glob, cold/warm cache, 1000-path load | needs Eris decision 7 |
| tools/run-tests.sh, test-cross-platform.sh, test-integrity.sh, performance-test.sh, benchmark-ignore-performance.sh | output and alias promises: needs Eris decision 3. Git and ignore edits in those tools: needs Eris decision 5 |

## Closed renderer gap tests

DX-D3-03b closes all four gaps against the unchanged D3-01 pinned bytes. These tests now run without `#[ignore]`. `render_markers` remains the body-only API; `render_legacy_snapshot` and `render_legacy_diff` add the legacy envelopes over injected trees, templates, versions and changes.

| Test | Gap |
|---|---|
| `crates/dctx-template/tests/legacy_envelopes.rs:snapshot_envelope_matches_legacy_bytes` | ~~D3-03 gap: render_markers does not emit the snapshot version header~~ Closed by `render_legacy_snapshot`. |
| `tree_only_envelope_matches_legacy_bytes` | ~~D3-03 gap: render_markers does not emit the snapshot version header~~ Closed by the snapshot renderer's tree-only option. |
| `diff_envelope_matches_legacy_bytes` | ~~D3-03 gap: diff header, version line, summary, updated tree, changed-content-only, SVG diff sentence~~ Closed by `render_legacy_diff`. |
| `svg_snapshot_sentence_matches_legacy` | ~~D3-03 gap: snapshot SVG sentence is `[SVG file (omit_svg_content=true), content omitted]`~~ Closed by explicit legacy SVG handling. |

Today's diff writer (`llm_formatter.rs` `generate_diff_context_with_template`) trims the tree and then `write_all`s it, so the closing tag is glued to the last tree line. `legacy-diff.txt` pins that glue and the diff SVG sentence `[SVG file, content omitted (omit_svg_content=true)]`. The snapshot SVG sentence differs from the diff sentence. `render_markers` emits `[Binary file: SVG file, content omitted]` when `binary_label` is `SVG file`, and `[Content omitted by pattern rules]` when `omit_content` is set. The parity corpus takes the pattern path for `*.svg` because the file is OmitContent and its bytes are never classified.

Passing renderer checks already lock the layout body: project variable `[DIRECTORY:Corpus]`, ids (`ROOT`, `D001`, `F002`, …), two-space indentation, `[IGNORED]` markers, pattern and PNG omission sentences, and the absence of ignored bytes (`SECRET=1`, `sensitive`). Tree-only drops the `LINES=` file blocks and the included file body. The instructions text still names the marker. A file whose bytes end in a newline keeps that newline inside the block, so a blank line sits before the end marker.

## Facade legacy output

`PackRequest::legacy_output` is `None` by default: text and JSON summaries retain their existing bytes. To opt in, supply `dctx_template::LegacyOutput::new(template, layout)` with `LegacyLayout::{Snapshot, TreeOnly, Diff}`. The template already holds project variables. `omit_svg_content` defaults to true and `debug_ignored` to false; both are explicit caller settings. Selection rules remain injected through the session. Turning SVG omission off never restores bytes withheld by a pattern decision.

Diff summaries preserve the injected report's order, while file blocks follow the full tree and retain its ids. A first run or major reset emits a snapshot; `PackReport::legacy_layout` records the actual layout. An unchanged diff has the current version on both sides, an empty summary and no file blocks. Rendering reads no config, environment, filesystem or Git state.

`crates/dctx/tests/legacy_output.rs` checks facade snapshot/tree bytes against the pinned fixtures, a fixed diff document (including SVG, unchanged-file exclusion and full-tree ids), first-run/major-reset behavior, unchanged summaries, and explicit custom-template/SVG settings with a source read spy. It checks both request and session byte caps, including the header and UTF-8 bytes, with exact-cap success and a one-byte-short refusal. Refusal writes no partial page. As in the existing pack operation, the engine publishes before final output encoding; an output-budget refusal does not roll back the published snapshot.

## Counts

`compatibility.rs` asserts both modes, fixed timestamps, child order, binary classification, a read of `README.md`, and denial for `.env`, `Cargo.lock`, `icon.svg`, `node_modules` and `private`. `malformed_compiled_bytes_are_refused` refuses empty input, a bad magic, a short buffer, version 99 and a non-UTF-8 name, with the sorted fixture as the positive control.

DX-D3-03b remote validation: `cargo test -p dctx-template --test legacy_envelopes --test markers --test v1_parity --test purity` passed 47 tests (zero ignored); `cargo test -p dctx --test legacy_output --test golden` passed 12 tests; `cargo clippy -p dctx-template -p dctx -- -D warnings` passed. All Cargo commands used the packet's remote predicates. No local Cargo or full workspace suite was run. The facade read spy initially assumed output order was scan order; it now checks the exact read multiset, since the engine sorts the rendered tree after scanning.
