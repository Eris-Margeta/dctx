<!-- SPDX-License-Identifier: Apache-2.0 -->
# Bash lookup citation audit

At base `ae4944fa1a0c`, the CLI returned enough evidence to locate the call to `helper()` and the value it returns in a second file. A one-record text find hid the other meaning of `run`. A read window retained the enclosing symbol's coordinates. A task pack could return matching names with zero source fragments. Confidence is high for these observations on the fixture. The evidence for each is below.

## Public fixture and method

The question was “Which implementation of `run` calls `helper()`, and where does that helper return its value?” The source gives three independently checkable anchors.

| Role | Checked-in source | Evidence |
| --- | --- | --- |
| Competing declaration | crates/dctx-map-parse/tests/fixtures/rust/src/lib.rs:19 | The trait method ends in a semicolon. |
| Call in the implementation | crates/dctx-map-parse/tests/fixtures/rust/src/lib.rs:24 | The body of `Widget::run` calls `helper()`. |
| Value returned by the callee | crates/dctx-map-parse/tests/fixtures/rust/src/support.rs:9 | The function body evaluates to `3`. |

I built the default `dctx-cli` binary from the locked base offline and ran each command in a separate process in a disposable copy of `crates/dctx-map-parse/tests/fixtures/rust`, named `rust`. The copy retained its manifest and both source files. File mtimes were fixed at Unix time 1700000000. The processes shared a disposable map cache and isolated configuration. All sizes below count raw stdout bytes including the final LF. The shell examples use `dctx` for the built binary. The returned paths are relative to the fixture root.

## Evidence along the lookup

**1. Find distinguishes the two methods if the caller sees both rows.** `dctx map find run` returned `s:src/lib.rs#Widget::run` at L23–25 and `s:src/lib.rs#Work::run` at L19. `--limit 1` returned just the first row in 51 text bytes, with no continuation notice on either stream. With `--limit 1 --json`, the first record had `path="src/lib.rs"`, `lines=[23,25]`, null span and source, `more=1` and a cursor. Repeating that JSON request with `--cursor "$next"` returned the trait declaration once, `more=0` and `next=null`. Text renders the line range beside an ID that embeds the path. JSON serializes the remaining count and token. An agent stopping at the first text page can mistake a visible candidate for the only candidate. Confidence is high. See crates/dctx/src/domain/wire.rs:100, crates/dctx/src/domain/wire.rs:63 and crates/dctx/src/session/query_op.rs:149.

**2. Read identifies a body, but a window needs its own line origin.** Bare `dctx map read run` chose the first find record. Its text supplied the ID, path and unnumbered body. Reading the two exact IDs in JSON distinguished the implementation at lines 23–25 from the declaration at line 19. Following the body to `dctx map find helper` yielded `s:src/support.rs#helper` at L8–10. Reading that ID returned the body containing `3`. For full reads, counting from the record's first line locates the call at 24 and the value at 9. The CLI's first-hit resolution and text rendering explain why retaining the find row or requesting JSON matters. Confidence is high. See crates/dctx-cli/src/map/mod.rs:281, crates/dctx/src/domain/wire.rs:112 and crates/dctx/src/domain/wire.rs:187.

With `map read 's:src/lib.rs#Widget::run' --lines 24-24 --json`, the body contained only the call, while `lines=[23,25]` and `span=[438,483]` still described the whole method. On the same fixture, `map read m:src/lib.rs --budget 250 --json` plus two cursor resumes delivered lines 1–14, 15–42 and 43–50. All three records reported `lines=[1,50]` and `span=[0,799]`. Removing the two trailing `[DCTX_TRUNCATED]` marker lines and concatenating the bodies reproduced the 799-byte file. The text read at budget 150 ended with `[DCTX_TRUNCATED] lines 23-50`. A consumer numbering each excerpt from the record start would label the second JSON page's first line as 1 instead of 15. Preserve the requested window or the preceding marker's absolute next line. Confidence is high. See crates/dctx/src/session/query_op/read_page.rs:45, crates/dctx/src/session/query_op/read_page.rs:159 and crates/dctx/src/session/query_op/read_page.rs:181.

**3. Pack verifies the source it selects. Hit count alone cannot establish coverage.** The following commands reproduce the format and budget comparison.

~~~sh
task='`run` `helper`'
dctx pack --for "$task"
dctx pack --for "$task" --budget 3000 --json
dctx pack --for "$task" --budget 212 --json
dctx pack --for "$task" --budget 100 --json
~~~

| JSON selection budget | Hits | Source fragments (inclusive lines, half-open bytes) | Display lines dropped |
| ---: | ---: | --- | ---: |
| 3000 | 3 | `src/lib.rs` L23–25 [438,483) and `src/support.rs` L8–10 [235,267) | 0 |
| 212 | 3 | `src/lib.rs` L23 only [438,461) | 6 |
| 100 | 3 | Empty array, exit 0 | 13 |

The full text pack (821 bytes) showed the caller site L24 in the helper's neighbors and printed both bodies. The full JSON pack (1,606 bytes) bound each body to its path, original line range, byte range and SHA-256. I checked both fragments against the exact-ID reads and the bytes on disk. The trait declaration appeared as find metadata with no source fragment, even with `dropped=0`. Selection keeps up to three find rows per term, chooses one best ID per term, and reads at most three chosen IDs overall. A tight budget can then cut the selected source to a prefix. At 212 the lone fragment supports the signature but cannot corroborate the call on the next line. Confidence is high. See crates/dctx/src/context/mod.rs:16, crates/dctx/src/context/mod.rs:118, crates/dctx/src/context/mod.rs:132, crates/dctx/src/context/page.rs:46 and crates/dctx/src/context/pack_json.rs:47.

**4. A miss has a different observable result at each step.** With the absent term `absent_probe`, text find emitted only its header. JSON find exited 0 with `records=[]`, `more=0`, `next=null`. There was no returned ID to read. As a separate negative control, reading the explicitly missing ID `s:src/lib.rs#absent_probe` exited 1 with `outcome="gone"`, no records and a diagnostic. JSON task pack exited 1 with a valid envelope, one term, zero hits, zero fragments and a hint on stderr. Compare the successful 100-budget pack above. Treating every empty array as a miss loses that distinction, and citing the missing ID would invent evidence. Confidence is high within this root. See docs/CLI-EXIT-CODES.md:22, crates/dctx/src/session/query_op.rs:78 and crates/dctx-cli/src/pack_for/mod.rs:65.

## What the existing tests verify

| Behavior | Assertion at the audited base | What still needs an offline acceptance case |
| --- | --- | --- |
| Find and target choice | The parser expects distinct IDs for the two methods and a call reference to `helper` (crates/dctx-map-parse/tests/fixtures.rs:32, crates/dctx-map-parse/tests/fixtures.rs:117). The wire test checks exact find text for a path-bearing ID (crates/dctx/src/domain/wire.rs:388). The CLI feeds a single `Beacon` find ID to read and checks its source contains the name (crates/dctx-cli/tests/map_gaps.rs:216). | Compare a bounded text find with both JSON cursor pages. Check which ID a bare-name read resolves to and compare its body with the exact implementation read. |
| Pagination and read windows | Tied find hits are tested across offsets in the query crate (crates/dctx-map-query/tests/n1118_hostile.rs:160). The cross-process CLI cursor test walks **map** records (crates/dctx-cli/tests/map_gaps.rs:258). CLI tests compare an inner symbol window with disk and reconstruct a whole file across budgeted reads (crates/dctx-cli/tests/map_gaps.rs:333, crates/dctx-cli/tests/map_read_followups.rs:219). | Pin which coordinates describe the node and which line actually starts each delivered window. Body equality alone leaves this open. |
| Pack selection and provenance | The CLI checks exact-case ranking against a test function and the selected read heading (crates/dctx-cli/tests/pack_for.rs:279). Span tests read the claimed file, compare bytes, count original lines and recompute SHA-256 (crates/dctx-cli/tests/pack_schema.rs:90). They cover CRLF, Unicode, missing final LF and a budget-cut prefix (crates/dctx-cli/tests/pack_schema.rs:156, crates/dctx-cli/tests/pack_schema.rs:181). On this same Rust fixture, the one-term text pack for `helper` at budget 103 must retain the find heading, fit the budget and omit any standalone closing brace (crates/dctx-cli/tests/bug_01.rs:35). | Check two-term JSON provenance across both files and competing IDs, including the case where hits survive but no fragments do. |
| Empty output | The engine checks empty records for excluded symbols (crates/dctx/tests/map_languages.rs:386). A missing module read must emit a gone page before exit 1 (crates/dctx-cli/tests/cli_errors.rs:329). The CLI pins the unmatched JSON pack to a golden at exit 1 (crates/dctx-cli/tests/pack_schema.rs:147). | Assert the three command outcomes together for an absent name in a nonempty root, with a known-hit control. |

Confidence is high in the cited assertions. The gaps refer to combinations I did not find asserted in the inspected tests. I inspected those tests as source and ran the fixture commands independently.

## What the benchmark measured

On the private Rust workspace, the warm Bash arm had 13.41% median strict citation recall and the harness-prefix pack arm had 70.91%. A match required the repo-relative path and a line within ±3. The pack arm received verified paths and original line numbers alongside excerpts before answering. Its median priced cost was 8.8% above grep, missing the cost target. Confidence is high in these reported aggregates. They give neither the missing citations for a particular Bash call nor the effect of changing that call. The public CLI task pack below is a different delivery path. See docs/BENCHMARK-20261111.md:13, docs/BENCHMARK-20261111.md:20, docs/BENCHMARK-20261111.md:22 and docs/BENCHMARK-20261111.md:25.

The separate shell-size study captured complete stdout. Its largest non-export answer was 824 bytes across 120 calls, and each full export exceeded one million bytes. Its 30,000-byte screen approximated a client character limit. The external Windows comparison lacked individual arguments, truncation notices and retry traces. Confidence is high that output size depended on the operation in that study. They cannot attribute the private recall gap to clipping or retries. See docs/research/bash-path-20261112.md:7, docs/research/bash-path-20261112.md:20 and docs/research/bash-path-20261112.md:72.

## Which hypotheses the fixture can discriminate

- **Missing or misleading coordinates.** The full JSON read and pack make both required locations recoverable. Following the find cursor exposes an overlooked candidate. Comparing the narrow read with disk exposes a line numbered from the wrong origin. Confidence is high for these mechanisms on the fixture. See crates/dctx/src/domain/wire.rs:63 and crates/dctx/src/session/query_op/read_page.rs:181.
- **Selection loss versus shell clipping.** At budget 212 the pack itself omitted the callee body from its fragments. In a separate simulated transport cut, taking the first 700 bytes of the complete 821-byte text pack preserved the caller's literal `helper()` line but removed the callee's literal `3` line and the end marker. These are distinguishable offline by retaining the original stdout, the delivered prefix and the selected fragments. The 700-byte cutoff was chosen for this fixture. Confidence is high for that byte comparison. The earlier size probe only counted complete stdout. See docs/PACK-SCHEMA.md:74 and tools/bench/shell_output_size.py:107.
- **Answer behavior remains untested.** A fixture can identify a wrong basename, a declaration used as a call site, or a missing second-page citation in a supplied answer. It cannot tell which mistake a historical agent made. This trial used one unchanged Rust root, two named terms, fixed budgets and complete local capture. It did not exercise a real client cap, changed snapshots, different root scopes, large fanout, model tool choice, retries, conversation replay, grading or cost. The earlier pilot reports shorthand paths in answers, but attributing the round-two gap would require the original per-question arguments, pre- and post-client output, answers and grader matches. Confidence in any such historical attribution from this trial is low. See docs/BENCHMARK-20261111.md:27, docs/BENCHMARK-20261111.md:80 and docs/research/bash-path-20261112.md:20.

## Follow-up cards

Each proposed check runs offline on the same disposable fixture. Each card has one write seam.

| Proposed title | Write seam | Minimum acceptance check |
| --- | --- | --- |
| Trace both meanings of a paged Bash find | `crates/dctx-cli/tests/map_gaps.rs` | `cargo test --offline -p dctx-cli --test map_gaps paged_run_ids`. With limit 1, assert the text page shows only the implementation and gives no token. Collect both JSON pages using the returned token, require distinct IDs in unpaged order, and read each exact ID in JSON to distinguish L19 from L23–25. Run `dctx map read run --json` and require exit 0 and one record. Its ID must equal both the first find ID and `s:src/lib.rs#Widget::run`. Its body must equal the body from `dctx map read 's:src/lib.rs#Widget::run' --json` byte for byte and contain the call to `helper()`. |
| Pin the origin of a windowed and resumed read | `crates/dctx-cli/tests/map_read_followups.rs` | `cargo test --offline -p dctx-cli --test map_read_followups read_page_coordinates`. Request only L24 of the method and assert its body equals that disk line while the node range stays 23–25. Resume the module at budget 250, compare each marker-free body with its absolute disk slice and require their concatenation to cover the file once. |
| Verify both files in an ambiguous task pack | `crates/dctx-cli/tests/pack_schema.rs` | `cargo test --offline -p dctx-cli --test pack_schema cross_file_citations`. For the two-term task, require three hits and exactly the two full fragments above at 3000, the signature-only prefix at 212, and exit 0 with zero fragments at 100. Verify each emitted fragment against its own path, line counts, bytes and hash. |
| Distinguish an empty find, gone read and unmatched pack | `crates/dctx-cli/tests/cli_errors.rs` | `cargo test --offline -p dctx-cli --test cli_errors empty_citation_lookup`. First find the known helper. For `absent_probe` assert the statuses 0/1/1 and the three JSON shapes in finding 4, including the gone ID and absence of source in each. |
| Measure citation loss at a controlled stdout boundary | `tools/bench/test_shell_citation_prefix.py` | `python3 -B tools/bench/test_shell_citation_prefix.py` with the built CLI. Capture the full two-term text pack once. Assert both source lines survive unrestricted capture, only the caller's survives a 700-byte prefix, and the original still contains the end marker. Record this as a simulated cutoff. |
