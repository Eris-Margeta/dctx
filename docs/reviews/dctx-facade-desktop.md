| Unit | Verdict | Finding |
| --- | --- | --- |
| DX-FX — new-language dispatch and `map_languages.rs` | PASS | no finding in the added dispatch/selection assertions; they include positive controls |
| DX-FX — shared extraction/default Rust path | FAIL | F12: decision 077 default parser violation and fabricated declarations; F20 |
| DX-D3c/d — facade cache wiring and administration | FAIL | F05–F08: concurrent writes, absent persisted leases, ineffective purge and disk accounting |
| DX-CA1 — cache adapter | FAIL | F05, F08–F10, F14, F19–F20 |
| DX-E4a/b — desktop API and Tauri commands | FAIL | F01–F04, F11, F14, F17–F18, F20 |
| PL-APPa4/b1b — plugin API, commands and ports | FAIL | F13–F14; consent payload comparison itself: no finding |
| PL-APPb2b — plugins UI/model and settings manager | FAIL | F15–F16 in settings; explicit plugin confirmation/text rendering: no finding |
| DX-E2Esb — browser specs and transport | PASS-WITH-NOTES | F21: mock/native behavior mismatch; DOM assertions are non-vacuous but do not verify native effects |

Static independent review, 2026-10-18. Read the assigned implementation and tests in bounded ranges, plus named dependencies needed to trace behavior. No project code, tests, Cargo, browser runner or packet predicates were executed. Reproducers below are deductions or exact tests to add, not executed results. Only this report was written.

**F01 — HIGH: desktop filters never constrain content reads.**
`crates/dctx-desktop-api/src/snapshot.rs:251` and `:262`.
`survey` uses extension/hidden/size/empty predicates, but `render` passes only ignore text to `open_session`; the pack receives none of those filters. One matching file admits every otherwise included file into the content-reading scan.
Reproducer: create `main.rs = "fn main() {}"` and `private.txt = "PRIVATE_SENTINEL"`; set `filter_extensions = Some(["rs"])`. Generation succeeds with `files=2` and reads `private.txt`. The success half of `extension_filter_refusal_has_a_matching_success` (`:378`) never asserts exclusion.
Fix: translate the complete desktop selection into the facade policy/source before any content access. Add a read-spy assertion for the filtered file and a visible positive control; output absence alone is insufficient.

**F02 — HIGH: project privacy rule sources are dropped.**
`crates/dctx-desktop-api/src/snapshot.rs:77` and `:262`.
The desktop loads only `.dctxignore`; `.gitignore` and `.dctxomitcontent` never enter `RuleTexts`. The facade deliberately does not discover these files, so decision 078's retained plain-text gitignore behavior and ADR-014's project omit rules are lost at the host boundary.
Reproducer: put `private.txt` in `.gitignore`, or separately in `.dctxomitcontent`, alongside `keep.rs` and a readable `private.txt`. Generate with default settings: the private file is read instead of excluded/omitted. Built-in omit rules do not cover this arbitrary filename.
Fix: load the supported rule sources through bounded, confined host reads and supply their ordered ignore/omit layers. Test both cases with a read spy and an included control.

**F03 — HIGH: wildcard negation reopens `.git` in the real scan.**
`crates/dctx-desktop-api/src/snapshot.rs:100`; supporting paths: `crates/dctx-core/src/selection/rule.rs:90`, `crates/dctx-pack/src/snapshot.rs:151`.
`negation_lifts_builtin` compares a literal first component against built-in names. The preliminary survey always skips `.git`, but the subsequent pack honors accepted wildcard negations with no immutable `.git` exclusion.
Reproducer: `.dctxignore = "!.g*/\n"`, `keep.txt = "keep"`, `.git/config = "PRIVATE_SENTINEL"`. The negation survives filtering, matches `.git`, and the pack reads `.git/config`; the count can exceed the survey's count. The existing test covers only literal `!.git/`.
Fix: enforce the no-`.git` rule at the actual source/selection boundary, independently of editable patterns. Test literal, wildcard and nested negations with a source that fails if `.git` is listed or read.

**F04 — HIGH: the rule-file loader follows a symlink outside the selected root.**
`crates/dctx-desktop-api/src/snapshot.rs:77`.
`is_file` and `read_to_string` follow `.dctxignore` symlinks before the confined `FsSource` is constructed. A selected project can cause bytes from an unselected outside file, including `.git` metadata, to be read as rules; no race is necessary.
Reproducer on Unix: create a harmless outside fixture containing `keep.txt\n`, symlink the project's `.dctxignore` to it, and add `keep.txt`. Generation consumes the outside rule and excludes the control file instead of refusing the symlink.
Fix: reject symlink/non-regular rule inputs and enforce root confinement on the opened file. Add the outside-fixture case beside a normal in-root rule-file control.

**F05 — HIGH: independent cache sessions overwrite each other's objects and ledger.**
`crates/dctx/src/session/ledger.rs:127`, `:163`, `:180`; `crates/dctx-adapter-cache/src/adapters/mod.rs:37`.
Each session retains its own `Lifecycle`, allocates numeric object IDs locally, and stores the whole image without a shared lock or revision check. The fixed `ledger.v1.tmp` also permits one store's staged image to be committed by another; `put_object` truncates an existing numeric object.
Reproducer: open sessions A and B against the same empty cache before either snapshots. Snapshot different source trees in A, then B. Both allocate object 1; B overwrites A's bytes and commits its stale ledger. This requires overlapping session lifetimes, not simultaneous filesystem calls.
Fix: serialize reload/admit/write/commit under a cross-process coordinator and use immutable, collision-safe publication. Add two independently opened sessions, distinct bytes and a cap that their combined admissions exceed.

**F06 — HIGH: persisted segments have neither retaining manifests nor handle leases.**
`crates/dctx/src/session/ledger.rs:174`; `crates/dctx/src/session/mod.rs:191`.
`admit_segment` inserts an unattached segment and writes it; it never publishes a view or acquires a lifecycle lease. `SnapshotHandle` only pins an in-memory generation. Persisted reachability starts at manifests, so every such segment is garbage even while its handle lives and its root is recently touched.
Reproducer: hold A's snapshot handle from day 0; open the same cache at days 1, 2 and 3 with default auto-clean. The segment is marked unreachable, moved to pending and unlinked despite that live handle and the 30-day root TTL. A's in-memory queries may still work, masking the disk lifetime failure.
Fix: publish segments through retaining manifests and tie persistent leases to handle/session lifetime under the coordinator. Test a live handle across other sessions' cleanup, then release it and prove reclamation.

**F07 — HIGH: facade purge/GC report reclaimed bytes while leaving persisted data.**
`crates/dctx/src/session/cache_op.rs:48`.
`reclaim` removes only `self.generations` and releases `AccountingCache` bytes. It never changes `PersistedCache`, retires a persisted manifest or unlinks an object. Fresh sessions also report zero in-memory generations while disk objects remain.
Reproducer: configure a cache root, take a snapshot, drop its handle, then call applying, confirmed `cache_purge(All)`. The report claims reclaimed segment bytes; `objects/1` and `cache_ledger().admitted_bytes` remain. Reopening preserves those bytes.
Fix: connect facade administration and reports to the same persisted coordinator used for publication. Assert disk contents, reloaded accounting and live-handle blocking, not just `generation_count`.

**F08 — HIGH: disk space is released in accounting before successful unlink.**
`crates/dctx-adapter-cache/src/adapters/mod.rs:113`; `crates/dctx-adapter-cache/src/domain/reserve.rs:221`; `crates/dctx/src/session/ledger.rs:180`.
`auto_clean` removes objects from `life` before calling `unlink_object`; an unlink error leaves the caller's ledger already reduced, and a retry no longer knows which file to unlink. Separately, `reserve_for` calls `reclaim_safe`, which removes objects without any filesystem deletion notification; facade admission then persists this reduced ledger. These violate the required pending-byte accounting.
Reproducer: with `Quota::scaled(100)`, insert/write a 10-byte segment, move it to `PendingUnlink` with one bypass-grace GC, then insert/write a 95-byte segment and store the ledger. Admission succeeds at 95 while both files occupy 105 payload bytes. For the error path, replace a pending object's file with a directory, run adapter auto-clean and observe an error with its ledger entry already gone.
Fix: retain charged pending records until the adapter confirms unlink, including admission-triggered reclamation. Test real unlink failure/retry and compare on-disk payload totals with accounting; the synthetic `fail_unlink` test does not exercise the failing adapter ordering.

**F09 — HIGH: an actual abrupt writer crash cannot be recovered.**
`crates/dctx-adapter-cache/src/domain/writer.rs:291`; `crates/dctx-adapter-cache/tests/recovery.rs:54`.
`recover_writer` handles only `WriterLock::Dead`. A durable image written before abrupt process death contains `Held`; absent OS-lock evidence does not change that state. Reservations and owned staging remain, and future writer acquisition stays busy. Existing recovery tests explicitly call `crash_writer` first, which a killed process cannot reliably persist.
Reproducer: register a root, acquire/reserve/stage, encode and decode the ledger without calling `crash_writer`, then reconcile with an empty `held_sessions` set. The writer remains `Held` and its admitted bytes remain charged. Repeat with a changed boot ID.
Fix: derive dead ownership from authoritative restart evidence for both held and dead records. Keep the positive control where the same session's OS lock is still held.

**F10 — HIGH: checksum-valid hostile ledgers reach debug panics instead of typed refusals.**
`crates/dctx-adapter-cache/src/domain/ledger_decode.rs:92`; `crates/dctx-adapter-cache/src/domain/mod.rs:488`.
Decoding validates cursor bounds but not lifecycle invariants, then calls `debug_check`. Invalid accounting/references panic in debug builds; release builds accept the invalid state because the assertions disappear. SHA-256 is an integrity checksum, not authentication against a crafted image.
Reproducer: encode a `Quota::scaled(100)` ledger with one 10-byte fact. Change the three little-endian quota words at file offsets 16, 24 and 32 to 1 and recompute the trailing first-party SHA-256. Decode: quota ordering and IDs pass, but P4 panics in debug or the over-cap, unfrozen state is accepted in release.
Fix: validate accounting, ownership and referential/state invariants with `CorruptLedger` errors before constructing an accepted lifecycle. Test checksum-valid semantic corruptions with an unchanged valid image as control.

**F11 — HIGH: desktop snapshot generation returns no context or tree.**
`crates/dctx-desktop-api/src/snapshot.rs:284` and `:291`; `crates/dctx/src/session/pack_op.rs:144`.
`render_pack` uses `PackRequest::new()` without a content-output request, so the facade emits only `# pack <version> files=N changes=N`. `tree_only` emits only a generation/digest line. Both are labeled successful/formatted snapshots; selected source contents and tree entries never reach the UI.
Reproducer: a project containing `keep.txt = "VISIBLE_SENTINEL"` produces no `keep.txt` or sentinel in either output mode. The snapshot tests assert the status-line prefix, and their negative secret-content assertion cannot detect a privacy leak because no source text is emitted at all.
Fix: request/render the facade's selected context output and an actual metadata tree for tree-only mode. Require an included filename/body in normal output, filenames without bodies in tree mode, and excluded/omitted read controls.

**F12 — MEDIUM: default Rust extraction violates decision 077 and fabricates symbols.**
`crates/dctx/Cargo.toml:14`; `crates/dctx/src/domain/policy.rs:66`; `crates/dctx/src/index/extract.rs:112`.
The shipped default omits `syntax-rust`; with no installed plan, the facade selects `Structural` even when that feature is linked. The line scanner searches for declaration keywords inside comments/strings, contrary to the required default `syn` path in `PARSERS-v1`.
Reproducer: default snapshot of `a.rs` containing `// fn Phantom() {}\nfn Real() {}\n`; `find("Phantom")` returns a fabricated function. Existing Rust extraction tests cover real declarations but no comment/string decoys.
Fix: route default Rust through the approved bounded `syn` adapter; retain explicit unsupported behavior in minimal builds. Add the decoy case with `Real` as a positive control.

**F13 — MEDIUM: plugin mutations remain permanently busy after a crash.**
`apps/dctx-desktop/src-tauri/src/plugin_ports.rs:99`.
The lock is an empty `create_new` file removed only by `unlock`. There is no OS-released lock, owner identity or stale-lock reconciliation, so a crash after acquisition makes subsequent enable/disable/consent operations fail indefinitely. The pure API tests use `MemoryStore` and cannot detect this.
Reproducer: acquire `FsStore::try_lock`, abandon that store without `unlock`, construct a new store for the same root and try to disable a plugin. `registry.lock` makes every attempt `Busy` although no owner remains.
Fix: use an OS-released lock with appropriate session identity and recovery. Test abandoned ownership and a genuinely live competing owner separately.

**F14 — MEDIUM: hostile files/directories bypass resource bounds before decoding.**
`apps/dctx-desktop/src-tauri/src/plugin_ports.rs:129`; `crates/dctx-desktop-api/src/snapshot.rs:82`; `crates/dctx-adapter-cache/src/adapters/mod.rs:238`.
Plugin registry and ignore-rule reads allocate to EOF before applying any codec limit. Cache load recursively enumerates every unknown path and accumulates all orphan names, without a count, depth or byte budget; the 128-object cleanup slice does not bound this preceding traversal.
Exact missing tests: oversized `registry.json` and `.dctxignore` must refuse after a bounded read; a cache containing more orphan names/depth than its work budget must return a bounded result/refusal, alongside small valid controls. Currently increasing these inputs increases allocation/work without a configured ceiling.
Fix: enforce bounded reads on opened handles before parse and paginate/cap orphan enumeration with explicit depth/work limits. Apply equivalent size limits to IPC strings/collections before extra cloning/serialization.

**F15 — MEDIUM: “Clear All Cache” resets settings; “Reset to Defaults” is unwired.**
`apps/dctx-desktop/src/settings-manager.ts:227`; `apps/dctx-desktop/index.html:1389` and `:1397`.
`button.danger:last-of-type` means the last button among siblings, not the last button in the section. Each action sits in a separate row, so `querySelector` selects the first, “Clear All Cache,” and attaches the reset handler there.
Reproducer: open Advanced settings and click “Clear All Cache”: it asks to reset settings and calls `reset_desktop_settings` on confirmation. Clicking the actual “Reset to Defaults” has no handler from this module. The browser spec checks headings, not these actions.
Fix: bind each action through an explicit stable ID and test the command emitted by both buttons, including cancellation.

**F16 — MEDIUM: settings reset accumulates handlers holding stale settings.**
`apps/dctx-desktop/src/settings-manager.ts:108` and `:241`.
Every initialization adds new anonymous listeners to the same controls, closing over a distinct settings object. Reset recursively initializes again without removing old listeners. Subsequent edits submit multiple full configurations; an older request completing last can restore pre-reset privacy/clipboard settings. Repeated resets also multiply confirmation handlers.
Exact missing test: initialize, change a filter, reset through the currently bound reset control, then change theme. Assert exactly one save, containing reset defaults plus the new theme; also reverse the completion order of any saves. The implementation emits both old and new settings objects.
Fix: install listeners once against one current state, or explicitly dispose them before rebinding. Exercise repeated resets with counted IPC calls.

**F17 — MEDIUM: settings IPC accepts values that corrupt its own saved format.**
`apps/dctx-desktop/src-tauri/src/main.rs:60`; `crates/dctx-desktop-api/src/settings.rs:174`.
There is no semantic validation, and string values are interpolated into a line-based format without escaping. Quotes/newlines in a selected path or IPC string fail round-trip or become new settings; numeric/filter combinations are also accepted without range checks.
Reproducer: save an otherwise default config with `filter_extensions = Some(["rs\"\nauto_copy_to_clipboard = true\n#"])`. Reloading sets clipboard auto-copy to true although the submitted boolean was false, because the injected line occurs after the generated boolean. An extension `"rs"` is the positive control.
Fix: validate enum/range/extension fields at the native boundary and use a reversible escaped format for arbitrary paths. Add quote/newline round-trips and invalid-value refusals through the command seam.

**F18 — MEDIUM: recent-project parse errors echo file contents into IPC errors.**
`crates/dctx-desktop-api/src/recent.rs:51`; `crates/dctx-desktop-api/src/error.rs:125`.
The raw serde error is stored and returned verbatim. Type errors include offending JSON string values, so corrupt/private registry content can appear in diagnostics rather than a content-free typed error.
Reproducer: set a fixture `recent_projects.json` to `["PRIVATE_SENTINEL"]`, then call `get_recent_projects`; serde's invalid-type error includes `PRIVATE_SENTINEL`. A valid single-project array must still load.
Fix: return a stable parse error code and bounded location/category without offending values. Add an assertion that no fixture sentinel appears in the command error.

**F19 — MEDIUM: purging a leased generation never schedules its eventual removal.**
`crates/dctx-adapter-cache/src/domain/purge.rs:81`; `crates/dctx-adapter-cache/src/domain/lease.rs:76`.
The leased branch only sets `deletion_requested` and leaves availability/current pointers intact. Release does not process that flag, and retirement does not consult it. Once the lease ends, the current generation remains served and retained until unrelated TTL/pressure or a second purge. `tests/lifecycle.rs:120` hides this by issuing that second purge.
Reproducer: publish one small current view, confirm a reader lease, purge with confirmation, release the lease, then drain bypass-grace GC at the same time. `open_current` still succeeds and bytes remain. No competing root or live lease explains retention.
Fix: drop logical retention at purge while preserving leased physical bytes, or process the deletion request when the last lease releases. Replace the second-purge test step with GC and assert eventual removal.

**F20 — LOW: file-size and crate-header requirements are violated.**
`docs/standards/RUST-STANDARDS.md:9` and `docs/standards/CODE-GUIDELINES.md:17` require small files and header documentation. Reviewed over-300-line files are `crates/dctx/src/index/extract.rs:301` (302); cache `src/adapters/mod.rs:301` (385), `src/domain/mod.rs:301` (492), `ids.rs:301` (304), `legacy.rs:301` (315), `maintain.rs:301` (304), `publish.rs:301` (439), `reserve.rs:301` (389), `writer.rs:301` (437); desktop API `src/snapshot.rs:301` (409), `tests/plugins.rs:301` (364). The substantial overruns exceed the “about 300” guideline; 302/304 are recorded because this packet explicitly asks for over-300 files.
`apps/dctx-desktop/src-tauri/src/lib.rs:1` is only a regular comment: no `//!` header, `forbid(unsafe_code)` or `deny(missing_docs)` for this separate library target. The binary's attributes do not cover it.
Exact missing check: audit line counts and required headers/attributes per Rust target, including the otherwise empty Tauri library. Fix by separating concepts/tests in oversized files and adding the required library header/attributes.

**F21 — NOTE: browser transport asserts behavior different from the native commands.**
`apps/dctx-desktop/e2e/tauri-mock.mjs:94` and `:108`; `apps/dctx-desktop/e2e/plugins.spec.mjs:164`.
The mock sets `enabled=true` when enabling a plugin lacking consent, whereas `PluginOps::enable` returns a needs-consent row without persisting enablement. Mock independent update checks succeed with `version:null`; the actual command returns `Network(Unreachable)`. The browser test therefore cannot exercise the real offline-error UI. This is not a claim that its DOM assertions cannot fail.
Exact missing tests: disabled/no-consent Enable must preserve enablement; an independent update must reject and leave controls usable; stale/tampered consent must reject without changing grants, next to a valid approval. The current happy-path mock will not catch a regression in the native refusal logic.
Fix: align fixtures with actual command results and add native command contract tests for the exact payload comparison/refusal paths.

No finding in the inspected production paths for spawning Git, using external SHA-256 instead of `dctx-hash`, or an existing first-party unsafe block; `.git` reads are covered by F03/F04. Inspected crate manifests declare Apache-2.0. No new external dependency was identified from this snapshot, but historical dependency additions cannot be established without a before/after baseline. No ignored Rust tests were encountered in the reviewed test bodies. These statements do not cover unaudited transitive dependencies.
