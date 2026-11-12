<!-- SPDX-License-Identifier: Apache-2.0 -->
# Windows one-shot map-find latency hypotheses (2026-11-12)

The reported tester result is about 2.7 s p50 per Windows process on a
1,551-file repository, versus about 0.3 s on Linux. These are input observations,
not measurements reproduced by this code-reading pass. Every possible cause
below is a hypothesis; the new timings do not establish causation.

Run a release `dctx map find NAME --place REPO` with `DCTX_TIMINGS=1` to append
one JSON line to stderr at normal or handled error exit. Stdout remains the
command answer. The `dctx_timings` object's integer values are microseconds:

| Phase | Boundary |
| --- | --- |
| `config_and_root` | Main entry through argument, root, rule configuration and session setup; also unclassified main time, auto-clean if enabled, and teardown. |
| `generation_open` | Nested rule/inventory discovery plus snapshot opening, retained-object reads, file stat/hash verification, lease work, or cold construction/persistence. |
| `decode` | Retained-generation decode or the in-process decoded-cache lookup/copy. |
| `query` | Facade query validation, selection, fallback reads, ranking, answer assembly and sink copy; includes the CLI's indexed-name follow-up query. |
| `render` | Facade answer encoding (including budget-driven retries) and CLI page conversion/outside-notice preparation. |
| `write` | CLI page write to stdout and explicit buffered flush. |

Phases are exclusive: a nested phase pauses its parent, including when nested
with the same phase. Totals accumulate across calls in the process. Sub-microsecond
values truncate to zero; phases not reached also remain zero. There is no portable
process-start timestamp available at main entry here, so `process_start_to_main`
is omitted. Loader/CRT work and process creation remain in the subprocess wall
measurement only; JSON report serialization and OS process exit are outside the
phase counters. Their sum is therefore not the subprocess latency.

The host checks `DCTX_TIMINGS` once. Only the exact value `1` enables counters.
Disabled hooks check constant-initialized thread-local state without allocation
or clock reads. Library use alone does not inspect the environment or emit output.
The counters follow the calling thread; this synchronous map-find path runs on
that thread. The independent existing `--profile`/`DCTX_PROFILE` facility is unchanged.

The existing Windows latency step enables timings. `map_find_latency.py` excludes
its single warmup from both wall and phase distributions, requires one valid
report per measured process, and writes `phase_medians_us` alongside existing
p50/p95 and raw wall samples. Both medians use the existing nearest-rank rule
(sample 25 of 50). It prints the phase table directly in the job log. Existing
informational/continue-on-error behavior and thresholds remain unchanged.
The fixture has one newly written Rust file, so it cannot reproduce the tester's
repository scale, and its first calls can exercise the two-second racy window.

## Concrete suspects from the path

This table records the pre-fix investigation. References are repository-relative
`file:line` locations from that original revision; the fix is described below.
Windows effects listed here are hypotheses, not externally verified platform
performance claims.

| Hypothesis | Concrete evidence and expected phase |
| --- | --- |
| Reopening all ancestor directories for every file could dominate deep/wide Windows trees. | `crates/dctx/src/index/walk.rs:283` falls back to `Source::stat`; `crates/dctx/src/adapters/fs.rs:237` calls `reject_links` before the Windows stat. `crates/dctx/src/adapters/fs.rs:115` delegates to `windows::pin_directories`; `crates/dctx/src/adapters/fs/windows.rs:126` opens every absolute ancestor with restrictive sharing and checks metadata. The Windows source has no inventory cursor override, whereas `crates/dctx/src/adapters/fs/unix.rs:230` supplies one. Expect `generation_open` to scale with file count and depth. |
| Per-file handle-based identity queries could cost more than Linux directory-relative stats. | `crates/dctx/src/adapters/fs/windows.rs:36` opens an attribute handle, calls `file.metadata()` and `GetFileInformationByHandle`; `crates/dctx/src/adapters/fs/unix.rs:98` uses directory-relative source stats. Expect `generation_open`, even when content reads are skipped. |
| Antivirus/indexer sharing conflicts could amplify source opens into backoffs. | `crates/dctx/src/adapters/fs/windows.rs:17` retries sharing/lock errors 32/33 with five waits totaling 310 ms, on stat, ancestor guards and content opens. Success has no backoff. Several conflicts could accumulate in `generation_open` or fallback `query`. Timings alone cannot prove a conflict occurred. |
| Recently modified files or missing Windows file identity could force full read/hash verification. | `crates/dctx/src/adapters/fs.rs:296` defines the two-second racy window; `crates/dctx/src/adapters/fs.rs:307` refuses unsettled/no-inode stamps. `crates/dctx/src/session/reuse/verify.rs:65` trusts only equal settled stamps, otherwise calling the digest path; `crates/dctx/src/session/query_op/read_page.rs:215` reads and hashes the source. `DCTX_FRESHNESS=strict_live` also forces this path. Expect `generation_open`. |
| Repeated canonicalization and rule-file opens could add root/config overhead on Windows. | `crates/dctx-cli/src/host.rs:188`, `crates/dctx/src/adapters/fs.rs:58` and `crates/dctx-cli/src/host/rules.rs:27` canonicalize roots separately. `crates/dctx-cli/src/host/rules.rs:28` resolves config, then reads global rule files; nested discovery uses `crates/dctx-cli/src/host/rules.rs:269`. Expect `config_and_root` and inventory work in `generation_open`. |
| Warm cache lease/ledger traffic could still create antivirus-sensitive temporary files. | `crates/dctx/src/session/ledger/cache_bind.rs:249` updates the persisted reader lease even on reuse. `crates/dctx-adapter-cache/src/adapters/mod.rs:149` stages ledger bytes, `:194` renames the staged file, and `:556` creates/writes/syncs files. Cold or refreshed segments also use `crates/dctx-adapter-cache/src/adapters/publish.rs:16` (temporary file, hard link, unlink). Expect `generation_open`, with some setup/teardown in `config_and_root`. |
| Lock acquisition and concurrent cache users could add Windows-specific contention. | `crates/dctx-adapter-cache/src/adapters/lock.rs:42` takes entry and ledger locks; `:58` opens/stat-checks the lock files and retries blocked locks with exponential waits. `crates/dctx/src/session/reuse/entry.rs:26` can retry reuse conflicts up to 32 times. Expect `generation_open` or cache setup in `config_and_root`. |
| Reading and decoding a complete generation per new process could grow with repository size. | `crates/dctx/src/session/ledger/cache_bind.rs:216` reads manifest and generation under a shared ledger lock; `:362` uses a reserved `Vec` and `read_to_end`, not mmap. `crates/dctx/src/session/reuse.rs:150` materializes the graph; `crates/dctx/src/session/reuse/warm.rs:21` only caches within a process, so separate invocations cannot reuse the decoded graph. Expect object I/O in `generation_open`, materialization in `decode`. A Windows read/allocation penalty is a hypothesis, not a demonstrated need for mmap. |

Git subprocess startup is not a supported explanation for the default find path:
`crates/dctx-cli/src/host/roots.rs:244` opens the notice session and
`crates/dctx-cli/src/host.rs:222` builds it without injecting a plugin host.
`crates/dctx/src/session/reuse/entry.rs:45` invokes hook machinery, but this host
configuration supplies no git capability. Ignore files are read directly.
The executable's bundled worker route is separate at
`crates/dctx-cli/src/main.rs:58`; code reading found no git spawn on this route.
A deployment with different injected hooks would require separate investigation.

Compare a repository-scale Windows/Linux run with matching freshness, config,
cache and auto-clean settings before prioritizing a fix. A large subprocess wall
minus phase sum suggests investigating startup/loader/exit overhead separately;
a large `generation_open` suggests distinguishing ancestor opens, stats, hashing
and cache traffic with OS tracing or the existing detailed map profile.

## Scale fixture

The one-file fixture from CI run 43241496123 measured 10.7 ms p50 on Windows,
with median `config_and_root` 3,465 us and `generation_open` 2,096 us. It does
not reproduce the tester's 2.7 s p50 on a 1,551-file repository.

`tools/bench/map_find_latency.py --scale-files 1551` generates exactly 1,551
small files in a deterministic order, cycling through `.rs`, `.py`, `.md` and
`.json`. Files live four directory levels below the fixture root, with 20 files
per leaf directory except the final partial group. Groups share base-four
directory prefixes. The first Rust file contains the same `latency_probe`
symbol as the default fixture; other files contain distinct numbered items.
The JSON `fixture_shape` records `files`, `dirs` (including the fixture root),
and `depth` (directory levels below that root). At 1,551 files these are 1,551,
106 and 4 respectively. `--scale-files 0` remains the default one-file layout,
with shape 1 file, 1 directory and depth 0.

All generated source mtimes, including the default fixture's, are set ten
seconds before generation starts, leaving at least five seconds of margin and
avoiding the two-second racy window without sleeps. This supersedes the freshly
written mtimes described for the earlier one-file benchmark above. Fixture and
isolated cache/config preparation are outside measurement. One unmeasured
map-find process builds the index, followed by 50 separate warm processes.
Wall p50/p95, raw samples and opt-in phase medians keep the existing accounting.

Windows CI runs both the default and scale fixtures and uploads separate JSON
files in the existing per-matrix artifact. One existing Linux stable matrix
entry runs the same scale fixture with `DCTX_TIMINGS=1` and uploads
`map-find-latency-linux-scale`. Both platforms use release builds and the same
cache/config isolation and disabled auto-clean. All steps remain informational
with `continue-on-error`; no performance threshold changes. Compare wall and
phase distributions side by side when CI artifacts are available. No scale
results were available in the initial investigation; the measurements follow
below. The synthetic mix and tree shape approximate repository scale, not the
tester's actual content or filesystem environment.

## Scale results

CI run 43259066838 used release builds, one excluded warmup and 50 warm
processes with `DCTX_TIMINGS=1`. These are the supplied artifacts under
`.inputs/winlat2/`, before this fix. Wall medians include process startup;
phase medians do not.

| Platform/toolchain | Files | Wall p50 (ms) | `generation_open` median (us) |
| --- | ---: | ---: | ---: |
| Linux stable | 1,551 | 9.669 | 5,367 |
| Windows stable | 1,551 | 287.285 | 276,760 |
| Windows MSRV | 1,551 | 274.257 | 263,297 |
| Windows stable | 1 | 10.114 | 1,926 |
| Windows MSRV | 1 | 9.793 | 1,689 |

The stable Windows scale delta is about 177 us per additional file; Linux's
scale phase averages about 3.5 us per file. The external tester's real
1,551-file repository measured about 2.7 s wall p50 on Windows. That is a
different environment, not a measurement reproduced here.

### Linux syscall baseline

A local release build was warmed on the same 1,551-file/106-directory fixture.
`strace -f -c` counted 1,656 `newfstatat` calls for a warm process. A second
`strace -f -yy` trace attributed exactly 1,551 of them to source files and
105 to child directory entries. Each source file incurred **one stat, zero
content opens and zero content reads** during `generation_open`. These
per-file counts are attributable to the phase by their call sites, rather
than treating the whole-process syscall total as phase-local.

The call chain is `host/rules.rs::project_rules_inventory` (inside its
`GenerationOpen` span), `host/inventory.rs::RuleWalk::entries`,
`adapters/fs/unix.rs::FsWalk::entries`, then `list_directory`'s
`statat(..., SYMLINK_NOFOLLOW)`. `PreparedSource` replays those stats into
`index/walk.rs::visit_file`; it does not stat each path a second time.
`session/reuse/verify.rs::verify_files` finds matching settled stamps and
skips hashing. The later snapshot span in `map/mod.rs` covers this reuse
validation and retained-object I/O; retained decoding has its own `Decode`
span. Cache/config/lease operations contribute other filesystem calls,
but are not per source file.

For recent/future mtimes, changed identities, missing identities, or
`strict_live`, the existing digest fallback remains: `digest_matches_source`
in `session/query_op/read_page.rs` calls `Source::read`. On Windows that
read still checks ancestors, calls `symlink_metadata`, opens content without
following reparse points and checks handle metadata. Cold construction also
reads content. The settled warm fixture exercises none of those per-file
fallback reads. The raw traces and timing reports are in the ignored
`target/D1112-WINOPEN/` directory.

The final local before/after runs were sequential, with compilation and cache
preparation finished. Both used the required scale benchmark, release binaries,
50 measured warm processes and `DCTX_TIMINGS=1`:

| Linux local measurement | Before | After |
| --- | ---: | ---: |
| Wall p50 (ms) | 16.331 | 16.244 |
| `generation_open` median (us) | 9,163 | 9,239 |
| Source-file `newfstatat` calls | 1,551 | 1,551 |
| Source-file opens / content reads | 0 / 0 | 0 / 0 |
| Whole-process `newfstatat` calls | 1,656 | 1,656 |

The Linux code path is unchanged; these timings are consistent with local
measurement variation. `before.json` and the required `after.json`, plus the
before/after `strace` summaries and detailed traces, are under
`target/D1112-WINOPEN/`.

## Fix

Windows had no `Source::inventory` override. Its listing supplied names and
kinds, after which every selected file's `FsSource::stat` called
`reject_links`, `windows::pin_directories`, and `windows::stat`. For a leaf
at depth four, each file reopened **every absolute root ancestor plus four
repository directories**, checking handle metadata on each, before its own
attribute open, `File::metadata` and `GetFileInformationByHandle`. Directory
listings repeated that ancestor walk too. This code establishes repeated
work; it does not establish how much antivirus contributes to its cost.

The Windows inventory now keeps a DFS stack of guarded directories. The
source already pins the absolute root chain; descending opens only the new
child, and leaving a subtree releases its guards. The fixture needs 105
child guards per traversal instead of thousands of repeated ancestor opens.
The guards retain the same access, sharing, retry and reparse-point checks.
No persistent directory-mtime shortcut or process-global metadata cache is
introduced. Rule discovery and the engine reuse the existing one-use
prepared inventory, as they already do on Linux.

`DirEntry::file_type` and `DirEntry::metadata` supply enumeration data.
For a file, `stat_entry` retains **one attribute open and one
`GetFileInformationByHandle`** to preserve the old access check and obtain
the file index. It replaces the separate `File::metadata` query with
enumeration metadata, accepting it only when the handle query agrees on
size, modification time and attributes. A mismatch or unsupported identity
query uses the original stat. A failed file stat falls back after policy
selection to the original per-file partial/error handling. Ordinary stats
and content reads remain live.

Removing the final per-file identity/access open would change semantics:
enumeration metadata alone cannot distinguish a same-size/same-mtime file
replacement, and a directory listing can succeed when file attributes are
denied. The optimization removes the depth multiplier and redundant
metadata queries while preserving these checks. The clock is sampled
before enumeration, and the unchanged two-second settled-stamp rule still
requires hashing for racy timestamps.

`crates/dctx/tests/inventory_equiv.rs` compares the inventory path with a
wrapper that disables it, using separate retained caches over the same
source. It checks identical reuse verdicts and answer bytes for settled
same-size edits, additions, deletions, renames, touches, inode replacements
preserving size/mtime, future-mtime same-stamp edits, and permission errors.
Windows cases additionally exercise a locked content file and denied file
`READ_ATTRIBUTES`. Windows unit tests check cached enumeration metadata
against live stats, a change after enumeration, and guard reuse/release.
Existing staleness tests are unchanged. The Windows-specific cases can be
cross-compiled here but require Windows to execute; no post-fix Windows
latency measurement is claimed from this Linux host.

Validation: `cargo test --offline --workspace` passed 2,599 tests with zero
failures or ignored tests. The three new Linux differential tests also passed
in isolation. `cargo clippy --offline --workspace --all-targets -- -D warnings`
and `cargo fmt --check` exited successfully. The Windows dctx library and test
targets passed cross-target clippy with warnings denied; those Windows-only
test cases were compiled, not executed.
