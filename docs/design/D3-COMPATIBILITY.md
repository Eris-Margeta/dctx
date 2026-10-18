# D3 compatibility obligations

DX-D3-01 froze legacy behaviour as fixed bytes and facts (D3-LEGACY-REMOVAL.md §3 and §5 packet D3-01; ARCHITECTURE-PROPOSAL.md row DX-D3). DX-D3-08b deletes the core IO wrappers and their exports under the PA decision in decision 114, after D3-02 codec, D3-03b renderer, D3-04c CLI and D3-07a/b test migration. The fixed compatibility fixtures remain the oracle.

Pack facts use injected timestamps (`1700000000000` plus the offsets in the fixtures). Tests read fixtures with `include_str!`. They do not call legacy IO and they do not write oracles.

The deleted legacy `CompiledTemplate::serialize` walked `HashMap` order. `dctx_template` sorts keys (`crates/dctx-template/src/v1/compiled.rs:53`). `legacy-template.hex` pins the sorted encoding (`SORTED`) and one unsorted order (`UNSORTED`, zeta then alpha). Decode accepts both. Encode emits `SORTED`.

## §3 family map

| §3 family | Obligation |
|---|---|
| purity.rs forbidden-IO scan, legacy control, manifest deps | Legacy directory exemption and file control removed. `crates/dctx-core/tests/purity.rs` scans every source module's production code and uses forbidden/allowed snippet controls. Cargo manifests and the first-party dctx-hash dependency are unchanged; no claim of literal std-only dependencies. |
| templates.rs asset conflicts, syntax, counts, critical rules | `crates/dctx-core/tests/templates.rs` (asset tests stay until installer ownership moves) |
| legacy_parity.rs normal and debug trees: path, kind, decision, ignored, size, time, content, binary class, child order, on the ignored/omitted/binary/SVG/nested corpus | Live oracle deleted. Replaced by `crates/dctx-pack/tests/compatibility.rs:normal_scan_matches_pinned_legacy_facts`; `debug_scan_matches_pinned_legacy_facts`; bytes in `fixtures/legacy-binary.hex`. |
| content_reader.rs ignored, omitted and subtree read denial, plus classification | Wrapper deleted; replaced by the same two compatibility tests (`NOT_READ`, `UNTOUCHED`, PNG row). A read spy fails if denied bytes are opened, beside the README.md read control. |
| walker.rs excluded recursion, included and debug metadata, omit | Wrapper deleted; replaced by the same two compatibility tests. CLI filtering is covered by `crates/dctx-cli/tests/legacy_alias.rs:filters_do_not_change_state_and_local_omit_stays_presentation_only`. |
| ignore.rs patterns, negation, depth, layers, errors, diagnostics | Wrapper deleted; replaced by pure selection unit tests, `crates/dctx-core/tests/selection_legacy.rs` and `crates/dctx-adapter-fs/tests/legacy_rules.rs`. Cache-only assertions retired in D3-07; the two existing ignored built-in differences (`.turbo/`, `.pnpm-store/`) remain documented in selection_legacy.rs. |
| omit_content.rs lockfiles, SVG, env, depth | Wrapper deleted; replaced by `crates/dctx-core/tests/selection_omit_legacy.rs`, compatibility corpus rows for Cargo.lock/SVG and adapter `legacy_rules.rs`. CLI fallback is pinned by `legacy_alias.rs:configured_template_binary_only_and_rule_read_errors_keep_legacy_behavior`; adapter rule IO refuses unreadable files. |
| template.rs substitution, parse, validation, compile, magic, roundtrip | Wrapper deleted; replaced by `crates/dctx-template/tests/v1_parity.rs` and `legacy_envelopes.rs:compiled_bytes_match_sorted_fixture_and_accept_legacy_order`. |
| llm_formatter.rs headers, version, diff, tree-only, ids, indentation, project variables, changed-content-only, ignored markers, variable safety | Wrapper deleted; replaced by `crates/dctx-template/tests/legacy_envelopes.rs` (closed envelope gaps below), `markers.rs` and `crates/dctx/tests/legacy_output.rs`. |
| reader.rs and writer.rs magic, truncation, nesting, version, timestamp, offsets, node encoding | Wrappers deleted; replaced by `crates/dctx-pack/tests/legacy_import.rs`, `legacy_export.rs`, `compatibility.rs` and `crates/dctx/tests/legacy_import.rs`. |
| state.rs fresh, valid, corrupt, recreation timestamps, export deletion | Wrapper deleted; host state loading lives in CLI compat.rs. Replacement tests: `crates/dctx-cli/tests/legacy_alias.rs:second_run_diffs_unchanged_stays_put_and_resets_retain_exports` and `tui_loads_legacy_version_and_reset_uses_the_alias_host`. Automatic export deletion retired; reset/corruption retain exports. |
| registry.rs FNV stability, Unicode, case, hex, separator; config.rs aliases and implicit paths | Wrappers deleted; pure settings remain. Replacements: settings.rs unit tests and `crates/dctx-cli/tests/legacy_alias.rs:registry_keeps_existing_mapping_and_known_fnv_vectors`, `configured_template_binary_only_and_rule_read_errors_keep_legacy_behavior`. |
| git_safety.rs missing ignore stays missing; ignore_debug.rs has no linked tests | No-op and unlinked debug module deleted. Missing/unchanged ignore assertions remain in CLI `legacy_alias.rs:alias_snapshot_and_tree_are_the_pinned_bytes` and `second_run_diffs_unchanged_stays_put_and_resets_retain_exports`; repository no-Git enforcement remains in `crates/dctx-map-query/tests/no_git.rs`. Installer/benchmark decisions are separate from this deletion. |
| benches/pattern_matching.rs glob, cold/warm cache, 1000-path load | Retained; D3-07 repointed to pure SelectionPolicy. Cold/warm cache comparison retired in favor of cold policy construction/repeated decisions. Criterion and tempfile dev-dependencies stay. |
| tools/run-tests.sh, test-cross-platform.sh, test-integrity.sh, performance-test.sh, benchmark-ignore-performance.sh | output and alias promises: needs Eris decision 3. Git and ignore edits in those tools: needs Eris decision 5 |

## D3-08b consumer audit and recall migration

Before deletion, `grep -rn --exclude-dir=dctx-core --exclude-dir=target dctx_core crates apps plugins tools` found 196 matches in 73 files. Grouped CLI imports were read separately. The only executable consumer of a removed API was `crates/dctx-pack/tests/legacy_parity.rs`: line 7 imported `content_reader::ContentReader`, line 12 `ignore::IgnoreEngine`, and line 14 `walker::walk_directory_structure`. That live oracle is now deleted in favor of the fixed `compatibility.rs` tests above.

All other imported or qualified core items stay:

| Module | Items used outside dctx-core |
|---|---|
| budget | PackLimits, Reservation, byte_ceiling, estimate_tokens |
| datatypes | DirContextTreeNode, NodeType |
| diff | ChangeType, DiffEntry, DiffReport, compare_trees (including CLI's module import) |
| domain | FileHandlingDecision, ItemContext; FileHandlingDecision is also used through the root re-export |
| error | DctxError (also aliased CoreError), ErrorCode, Resource (also aliased CoreResource) |
| facts | ContentDigests, Diagnostic, EntryMeta, PackSnapshot, ReadGrant, SourceEntry, apply_content, mark_subtree_ignored |
| filter | FilterConfig |
| ids | RelativePath, Timestamp, ViewKey; Timestamp is also re-exported by the facade through the core root |
| selection | RuleTexts, SelectionPolicy, glob_match, rule::last_match; SelectionPolicy also uses the root re-export |
| settings | AppConfig, APP_NAME, CONFIG_FILENAME, parse_config via CLI's settings module import |
| version | Version, version_after_diff |

The desktop ADR's two `config` references are historical statements that it never calls those APIs. `crates/dctx-pack/tests/purity.rs` names removed modules only as forbidden strings and stays unchanged. Dependency declarations use the dctx-core name without aliases. Core's own migrated selection tests and benchmark use pure APIs; template asset tests and assets stay. Retained source comments and the settings legacy-output test name remain as allowed by the packet.

`crates/dctx-map-query/tests/recall.toml` replaces all 13 deleted-symbol rows with inspected live equivalents: pack scan/content, CLI tree filtering, explicit template listing, CLI settings/registry/state, filesystem rule loading, v1 template compilation/validation/serialization/substitution and pack binary encoding. Question text now describes those APIs (including caller-supplied sinks and explicit directories). All 30 questions remain; no rows were removed. `crates/dctx-map-query/tests/repository.rs` checks that every expected symbol exists when the kernel runs it.

## Closed renderer gap tests

DX-D3-03b closes all four gaps against the unchanged D3-01 pinned bytes. These tests now run without `#[ignore]`. `render_markers` remains the body-only API; `render_legacy_snapshot` and `render_legacy_diff` add the legacy envelopes over injected trees, templates, versions and changes.

| Test | Gap |
|---|---|
| `crates/dctx-template/tests/legacy_envelopes.rs:snapshot_envelope_matches_legacy_bytes` | ~~D3-03 gap: render_markers does not emit the snapshot version header~~ Closed by `render_legacy_snapshot`. |
| `tree_only_envelope_matches_legacy_bytes` | ~~D3-03 gap: render_markers does not emit the snapshot version header~~ Closed by the snapshot renderer's tree-only option. |
| `diff_envelope_matches_legacy_bytes` | ~~D3-03 gap: diff header, version line, summary, updated tree, changed-content-only, SVG diff sentence~~ Closed by `render_legacy_diff`. |
| `svg_snapshot_sentence_matches_legacy` | ~~D3-03 gap: snapshot SVG sentence is `[SVG file (omit_svg_content=true), content omitted]`~~ Closed by explicit legacy SVG handling. |

The deleted diff writer (`llm_formatter.rs` `generate_diff_context_with_template`) trimmed the tree and then wrote it, so the closing tag was glued to the last tree line. `legacy-diff.txt` pins that glue and the diff SVG sentence `[SVG file, content omitted (omit_svg_content=true)]`. The snapshot SVG sentence differs from the diff sentence. `render_markers` emits `[Binary file: SVG file, content omitted]` when `binary_label` is `SVG file`, and `[Content omitted by pattern rules]` when `omit_content` is set. The parity corpus takes the pattern path for `*.svg` because the file is OmitContent and its bytes are never classified.

Passing renderer checks already lock the layout body: project variable `[DIRECTORY:Corpus]`, ids (`ROOT`, `D001`, `F002`, …), two-space indentation, `[IGNORED]` markers, pattern and PNG omission sentences, and the absence of ignored bytes (`SECRET=1`, `sensitive`). Tree-only drops the `LINES=` file blocks and the included file body. The instructions text still names the marker. A file whose bytes end in a newline keeps that newline inside the block, so a blank line sits before the end marker.

## Facade legacy output

`PackRequest::legacy_output` is `None` by default: text and JSON summaries retain their existing bytes. To opt in, supply `dctx_template::LegacyOutput::new(template, layout)` with `LegacyLayout::{Snapshot, TreeOnly, Diff}`. The template already holds project variables. `omit_svg_content` defaults to true and `debug_ignored` to false; both are explicit caller settings. Selection rules remain injected through the session. Turning SVG omission off never restores bytes withheld by a pattern decision.

Diff summaries preserve the injected report's order, while file blocks follow the full tree and retain its ids. A first run or major reset emits a snapshot; `PackReport::legacy_layout` records the actual layout. An unchanged diff has the current version on both sides, an empty summary and no file blocks. Rendering reads no config, environment, filesystem or Git state.

`crates/dctx/tests/legacy_output.rs` checks facade snapshot/tree bytes against the pinned fixtures, a fixed diff document (including SVG, unchanged-file exclusion and full-tree ids), first-run/major-reset behavior, unchanged summaries, and explicit custom-template/SVG settings with a source read spy. It checks both request and session byte caps, including the header and UTF-8 bytes, with exact-cap success and a one-byte-short refusal. Refusal writes no partial page. As in the existing pack operation, the engine publishes before final output encoding; an output-budget refusal does not roll back the published snapshot.

## Counts

`compatibility.rs` asserts both modes, fixed timestamps, child order, binary classification, a read of `README.md`, and denial for `.env`, `Cargo.lock`, `icon.svg`, `node_modules` and `private`. `malformed_compiled_bytes_are_refused` refuses empty input, a bad magic, a short buffer, version 99 and a non-UTF-8 name, with the sorted fixture as the positive control.

DX-D3-03b remote validation: `cargo test -p dctx-template --test legacy_envelopes --test markers --test v1_parity --test purity` passed 47 tests (zero ignored); `cargo test -p dctx --test legacy_output --test golden` passed 12 tests; `cargo clippy -p dctx-template -p dctx -- -D warnings` passed. All Cargo commands used the packet's remote predicates. No local Cargo or full workspace suite was run. The facade read spy initially assumed output order was scan order; it now checks the exact read multiset, since the engine sorts the rendered tree after scanning.
