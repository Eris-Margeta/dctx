# DCX-NL-01b — no legacy behavior

2026-11-07. Worktree `/path/to/workdir/codex/DCX-NL-01b`, starting HEAD
`09407806d0bba20206271c8922628787dd84f58f` (`v1.1.0-18-g09407806`).
Source: `/path/to/workdir/docs/DESIGN-REVIEW-SITDOWN-12-PLAN-20261106.md`, section 5,
paragraph **dctx — card DCX-NL-01**, and the owner's 06.11 no-legacy instruction.
No commit created.

Removed `.legacyignore` and `.dctxomitcontent` loading/stamping from the filesystem,
map, CLI and desktop adapters, including the obsolete core filename constant.
Project rule files are `.gitignore` then `.dctxignore`; built-in and explicitly supplied/global
omission policies remain. The read-spy privacy tests still prove excluded/omitted content is not opened.
Filesystem `legacy_rules.rs` is now `rules.rs` without the retired file cases.

Deleted `compat.rs`, the implicit `dctx PATH` command, its top-level snapshot flags and
its integration tests. No legacy project registry, state loading/saving, recreated-folder
heuristic or implicit project-text export is reachable through CLI/TUI. Clipboard IO is in
`clipboard.rs`; the TUI invokes explicit pack/template/managed-cache commands and has no diff/reset
or old snapshot/filter menu paths. Removed the exposed `cache gc --legacy` inspector and its
selection flags so the help page advertises no legacy forms. Shared pack rendering/codec library
APIs remain the current explicit pack format; this card does not replace that format.

The named core selection files were integration tests, not separate runtime implementations:
renamed to `selection.rs` and `selection_omit.rs`, retained pure policy/negation/privacy coverage,
and removed obsolete references to the retired core ignore engine. Updated the recall corpus's
four now-removed CLI symbols to current explicit-command functions. Added Unreleased / Removed
changelog entries for the planned 1.2.0 release; no version bump or workspace member changes.

## Commands and evidence

All Cargo commands ran from the worktree with:

```sh
export PATH=$HOME/.cargo/bin:$PATH
umask 022
```

Toolchain: `rustc 1.99.0 (b940084d7 2026-09-28)`,
`cargo 1.99.0 (5f94df478 2026-08-27)`; both version commands exit 0.
The configured `sccache` wrapper is forbidden by this sandbox, so subsequent Cargo commands use
`RUSTC_WRAPPER=` to invoke the installed toolchain directly. No dependencies were downloaded.
Logs are under `/tmp/DCX-NL-01/` in this workspace session.

| Exact command | Exit | Result |
| --- | ---: | --- |
| Initial seam grep (exact command below) | 0 | 131 matching lines in 59 files; full own-set disposition below; `seams-before.txt` |
| `cargo test --workspace --offline --exclude dctx-desktop --exclude dctx-desktop-api` | 101 | 0 tests executed: `sccache ... rustc -vV` failed with `Operation not permitted`; `test-1.log` |
| `RUSTC_WRAPPER= cargo test --workspace --offline --exclude dctx-desktop --exclude dctx-desktop-api` (attempt 2) | 101 | 0 tests executed: two removed empty argument slices in cache test `publish_pack` calls; fixed; `test-2.log` |
| `RUSTC_WRAPPER= cargo test --workspace --offline --exclude dctx-desktop --exclude dctx-desktop-api` (attempt 3) | 101 | 1528 passed, 1 failed, 0 ignored, 138 completed targets; old read-count assertion expected 5 rather than 3 after removing the extra policy file; fixed; `test-3.log` |
| `RUSTC_WRAPPER= cargo test --workspace --offline --exclude dctx-desktop --exclude dctx-desktop-api` (attempt 4) | 101 | 1553 passed, 1 failed, 0 ignored, 145 completed targets; results draft was written during the repository cold index, so its warm-reuse assertion correctly detected 1 changed document; `test-4.log` |
| `RUSTC_WRAPPER= cargo test --workspace --offline --exclude dctx-desktop --exclude dctx-desktop-api` (final, unchanged worktree) | 0 | **2219 passed**, 0 failed, 0 ignored, 237 targets including doc tests; `test-5.log` |
| `RUSTC_WRAPPER= cargo test -p dctx-desktop-api --offline` | 0 | 43 passed, 0 failed, 0 ignored, 6 targets including doc tests; `desktop-api-test.log` |
| `RUSTC_WRAPPER= cargo clippy --workspace --all-targets --offline -- -D warnings` | 101 | Blocked by unavailable desktop native libraries (`atk`, `gdk-3.0`, `gdk-pixbuf-2.0` pkg-config inputs); `clippy-1.log` |
| `RUSTC_WRAPPER= cargo clippy --workspace --all-targets --offline --exclude dctx-desktop --exclude dctx-desktop-api -- -D warnings` | 0 | Clean, no warnings; `clippy-2.log` |
| `RUSTC_WRAPPER= cargo clippy --workspace --all-targets --offline --exclude dctx-desktop -- -D warnings` | 0 | Clean, includes the changed desktop API; `clippy-3.log` |
| `scripts/release-check.sh --quick target/debug/dctx-cli` | 0 | `PASS code-scan`, `PASS determinism`, `PASS unrelated-edit`, `RELEASE-CHECK PASS`; network gate explicitly SKIPPED because ptrace is denied; uses this worktree's debug binary, no release-profile claim; `release-check.log` |
| Card exit grep (exact command below) | 1 | 0 matches, no output; `grep-after.txt` |
| Expanded exit grep (exact command below) | 1 | 0 matches, no output, also checks the second removed core filename; `rg-after.txt` |
| `target/debug/dctx-cli --help` | 0 | Only current explicit command forms; `help.txt` |
| Help-form grep (exact command below) | 1 | 0 obsolete help forms, no output |
| `git diff --check` | 0 | No whitespace errors |
| `git diff --name-only -- legacy-c/ docs/LEGACY/ presentation/ website/ tools/website/ .github/ Cargo.toml scripts/` | 0 | No output; protected history, website, workspace lists, CI and gates unchanged |

Exact grep commands (in order; exits **0, 1, 1, 1**, respectively; the last three print nothing):

```sh
rg -n 'legacyignore|dctxomitcontent|legacy_alias|selection_legacy|selection_omit_legacy|compat' crates
grep -rn 'legacyignore\|dctxomitcontent\|legacy_alias\|selection_legacy' crates
rg -n 'legacyignore|dctxomitcontent|legacy_alias|selection_legacy|selection_omit_legacy' crates
grep -nE 'legacy|dctx \[PATH\]|dctx \.|--diff|--tree[[:space:]]|--reset|--legacy' /tmp/DCX-NL-01/help.txt
```

The test-count command was:

```sh
python3 - <<'PYCOUNT'
from pathlib import Path
import re
for name in ['test-1.log', 'test-2.log', 'test-3.log', 'test-4.log', 'test-5.log', 'desktop-api-test.log']:
    text = (Path('/tmp/DCX-NL-01') / name).read_text()
    rows = re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out', text)
    print(name, [sum(int(row[i]) for row in rows) for i in range(5)], 'targets', len(rows))
PYCOUNT
```

The fourth run reached repository-scale evidence: recall **41/50**, 2322 parsed files,
full cold index **331.290936971 s**, prepared-query p95 **67.07768 ms**. Its single failure
was caused by writing this results draft while the root was being indexed (one new/changed
document). No source or gate fix was made for that failure. All worktree edits were completed
before the final run, and no writes were made until that run exited.

Test counts sum the Cargo `test result:` rows, including doc tests, without adding separate
runs together. Supplied pre-card baseline: **2279**. Final requested run: **2219 passed** (baseline difference: -60).
This change removes 23 obsolete tests (15 alias, 4 old menus, 2 alias-routing, 1 local-omit loader,
1 legacy-cache inspector) and adds 2 current CLI regression tests, a net change of **−21**.
The requested run is **2215 ordinary tests + 4 doctests**. Desktop API contributes 43 ordinary
supplemental tests, recorded separately. The CI baseline uses nextest with only desktop excluded
(`.github/workflows/ci.yml`), so a like-for-like count is **2215 + 43 = 2258**, exactly **21 fewer**
than 2279. Doctests explain the remaining four-count difference between Cargo and nextest totals.
The supplied baseline was not rerun.
The new command regression tests reject existing relative/absolute paths and all former top-level
snapshot flags without project writes, and verify explicit `pack` still succeeds.

Final repository integration output:

```text
CM-6b evidence: recall=41/50; full_index=353.713226021s; prepared_query_p95=83.067909ms; parsed=2323; scan=3.628921781s, parse=3.356511934s, resolve=17.443864393s, store=176.763410098s; stale={}
CM-6c dctx reopened_query_p95=1.959010186s; content_reads=0; reindexes=0
CM-6c 5k pre-filter=64.838587ms; content_reads=0
CM-6c 500 files: cold=2.263953636s, reopened_query_p95=1.94067909s, one_file=4.054481702s; warm_reads=0, warm_reindexes=0
```

## Own set from the first seam grep

All **59** hit files were included in this review set. `compat` also matches legitimate
`compatible`/`incompatible` protocol, grammar, cache-schema and sandbox API terms; those hits are
not legacy aliases or implicit rule-file paths and were not mechanically renamed.
The additional core test filenames (which the content grep does not enumerate), current replacement
files and the supporting CLI cache/menu/pack modules are included in the implementation above.

| Initial hit file | Disposition |
| --- | --- |
| `crates/dctx-adapter-cache/src/domain/ledger_decode.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-adapter-cache/src/domain/ledger_encode.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-adapter-cache/src/domain/maintain_slice.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-adapter-cache/src/domain/mod.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-adapter-cache/src/domain/retire.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-adapter-cache/src/domain/status.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-adapter-cache/tests/lifecycle.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-adapter-fs/src/adapters/rules.rs` | updated for removal and current API/test references |
| `crates/dctx-adapter-fs/tests/legacy_rules.rs` | renamed to `tests/rules.rs`; removed retired file cases, retained current loading/privacy checks |
| `crates/dctx-adapter-fs/tests/privacy.rs` | updated for removal and current API/test references |
| `crates/dctx-cli/src/compat.rs` | deleted; snapshot registry/state/export and rule fallbacks removed; clipboard IO moved to `clipboard.rs` |
| `crates/dctx-cli/src/help.rs` | updated for removal and current API/test references |
| `crates/dctx-cli/src/host/rules.rs` | updated for removal and current API/test references |
| `crates/dctx-cli/src/lib.rs` | updated for removal and current API/test references |
| `crates/dctx-cli/src/main.rs` | updated for removal and current API/test references |
| `crates/dctx-cli/src/pack/emit.rs` | updated for removal and current API/test references |
| `crates/dctx-cli/src/plugin/mod.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-cli/src/template.rs` | updated for removal and current API/test references |
| `crates/dctx-cli/src/tui.rs` | updated for removal and current API/test references |
| `crates/dctx-cli/tests/legacy_alias.rs` | deleted with the implicit command; current refusal/pack controls in `tests/commands.rs` |
| `crates/dctx-core/Cargo.toml` | updated for removal and current API/test references |
| `crates/dctx-core/src/datatypes.rs` | retained: ordinary Include default documentation; no legacy selection implementation |
| `crates/dctx-core/src/settings.rs` | updated for removal and current API/test references |
| `crates/dctx-desktop-api/src/snapshot_rules.rs` | updated for removal and current API/test references |
| `crates/dctx-desktop-api/tests/snapshot_privacy.rs` | updated for removal and current API/test references |
| `crates/dctx-map-parse/src/domain/facts.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-map-query/src/adapters/inventory.rs` | updated for removal and current API/test references |
| `crates/dctx-map-query/src/domain/parse_status.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-map-query/tests/no_git.rs` | updated for removal and current API/test references |
| `crates/dctx-map-query/tests/recall.toml` | updated for removal and current API/test references |
| `crates/dctx-map-query/tests/recovery.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-map-resolve/src/domain/facts.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-map-resolve/src/domain/resolve.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-map-scan/src/adapters/rules.rs` | updated for removal and current API/test references |
| `crates/dctx-map-scan/src/domain/ignore/parity_0.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-map-scan/src/domain/ignore/parity_1.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-map-scan/src/domain/ignore/parity_2.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-map-scan/src/domain/ignore/parity_3.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-map-scan/src/domain/ignore/parity_4.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-map-store/src/adapters/tests.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-pack/src/legacy_codec.rs` | retained: bounded codec/library format evidence; no implicit CLI route or file discovery |
| `crates/dctx-pack/tests/compatibility.rs` | retained: bounded codec/library format evidence; no implicit CLI route or file discovery |
| `crates/dctx-plugin-distribution/src/domain/registry.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-plugin-host/src/domain/lifecycle.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-plugin-host/src/service/supervisor.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-plugin-host/tests/lifecycle.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-plugin-host/tests/liveness.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-plugin-host/tests/resources.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-plugin-host/tests/trace.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-plugin-platform/DEPENDENCIES.md` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-plugin-platform/src/linux/confinement.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-plugin-protocol/tests/negotiation.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-template/src/plugin/core.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx-template/src/plugin_codec/values.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx/src/domain/request.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx/src/domain/wire.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx/src/plugins/boundary.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx/src/session/reuse.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |
| `crates/dctx/src/session/reuse/keys.rs` | retained: format/schema/grammar/protocol/ABI compatibility or source documentation, unrelated to retired CLI/rule paths |

## Boundaries and incomplete acceptance

`legacy-c/`, `docs/LEGACY/`, `presentation/`, `website/` and `tools/website/` remain untouched.
No Cargo workspace lists, CI files or gates were edited. The only Cargo.toml change is the core
package description, which no longer claims removed compatibility modules.

DIDN'T obtain a clean **full-workspace** Clippy run: the sandbox has no desktop GTK/ATK development
libraries and forbids installation outside writable roots. The desktop-excluded check includes
all other crates, including desktop API; it is supplemental evidence, not a substitute claim for
the requested command. No gate was relaxed, no missing library was stubbed, no checks were ignored.
The release script's network gate also could not trace due to denied ptrace and is reported as skipped.
