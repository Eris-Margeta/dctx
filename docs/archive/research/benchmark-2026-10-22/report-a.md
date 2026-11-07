Historical design record; superseded by [the current documentation](../../../BENCHMARK-20261030.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# BENCH-01a: dctx vs Trail-Code, 2026-10-22

Status: **ESC — incomplete paired benchmark.** Trail completed the root build and returned lookup outputs in this run; dctx's immutable release cannot build this snapshot under its compiled source-content ceiling. This is an observed functional advantage for Trail on the supplied root, with exact selected coverage and answer quality still unqualified. A failed scan returning zero bytes is not a token-efficiency win. Graphify belongs to BENCH-01b and was not run.

| Metric, whole snapshot unless stated | dctx | Trail-Code |
|---|---|---|
| Cold index / first lookup | 0/3 completed; median 77.89 s; 64 MiB source limit | 1/1 completed; single attempt 680.17 s; fresh scope stores |
| Find output / estimated tokens | rc 1; 65.11 s; 0 stdout bytes; 0 estimated tokens | rc 0; 24.87 s; 1,281 stdout bytes; 366 estimated tokens |
| Explain / source output / estimated tokens | rc 1; 60.02 s; 0 stdout bytes; 0 estimated tokens | rc 0; 14.55 s; 3,220 stdout bytes; 920 estimated tokens |
| Map output / estimated tokens | rc 1; 67.02 s; 0 stdout bytes; 0 estimated tokens | rc 0; 6.80 s; 136,375 stdout bytes; 38,965 estimated tokens |
| Call-edge output / estimated tokens | rc 1; 63.43 s; 0 stdout bytes; 0 estimated tokens | rc 0; 13.05 s; 161,200 stdout bytes; 46,058 estimated tokens |
| Pack output / estimated tokens | rc 1; 60.52 s; 0 stdout bytes; 0 estimated tokens | Unsupported: no whole-corpus pack command; source lookup is reported separately |
| Correctness / source provenance | No answer from completed full-corpus probes; gold-task score not measured | Gold-task score not measured; no paired correctness verdict |
| Index size | No complete index; last cold cache contains 481 bytes in 3 bookkeeping files | Completed single-build index: 692,416,225 bytes / 9,927 files / 60 scope stores, after initial queries |
| Incremental-update cost | Not measurable from a failed baseline | Update cost unqualified: new Python file query 17.17 s, source visible False; no-op 16.43 s; removal query 16.97 s, source present False. Retained index delta after add/remove +0 bytes |
| Peak resident memory | 71,672 KiB maximum across attempted full-corpus probes | 7,439,632 KiB maximum across attempted full-corpus probes |
| Exact scan / exclusion accounting | 2965 distinct regular paths successfully opened in a separate traced attempt; 0 files in a complete index. Exact policy-excluded count unavailable after abort | 9,674 file nodes; 9,674 extraction and 9,674 fingerprint records; 60 graphs. Exact exclusions unclassified |
| Env-shaped omit by default, synthetic fixture | Artificial fixture only: pack rc 0; canary emitted for .env, .env.local, settings.env; 0 canary-bearing store-file occurrences | Artificial fixture only: grep rc 0; canary emitted for none; 0 canary-bearing store-file occurrences |

## Corpus and verification

Input: `/path/to/workdir/bench/corpus/` on build-host, `user@192.0.2.11`. Corpus receipt: local and build-host regular files both **1,975,840**; `rsync -aHn --itemize-changes` exited 0 with zero changes. This is **file-count agreement plus size/mtime/metadata comparison, not checksum verification**. No corpus checksums were performed by this card.

Independent recursive `lstat` inventory, without following symlinks:

| Item | Exact value |
|---|---:|
| Regular-file paths | 1,975,840 |
| Logical bytes, summed per regular-file path | 152,757,321,322 |
| Allocated bytes, summed per path; hardlinks can be counted repeatedly | 157,741,256,704 |
| Directories, including root | 285,562 |
| Symlinks | 14,590 |
| Other entries | 2 |
| Env-shaped regular files | 66 |
| Top-level entries | 131 |
| Inventory stat errors | 0 |

Env-shaped means basename matching `.env`, `.env.*`, `*.env`, or `.envrc`; it does not assert that any file contains a credential. Inventory took 344.836 s. The snapshot has no root `.git`; 57 top-level directories have `.git` directories. There was no root `graft/`, `.trail/`, or `.trail/cache/` before the run. It was supplied as one whole root to each tool, with each tool's default selection policy, not as a sampled subcorpus.

Post-main-run metadata verification, completed before the overlay-only incremental probe: `{"added_regular_files": 0, "comparison": "all regular-file paths: size, mtime_ns, mode, inode, nlink; recursive counts; no content checksums; atime not compared", "directories": 285562, "metadata_changed_regular_files": 0, "other_entries": 2, "regular_files": 1975840, "regular_logical_bytes": 152757321322, "removed_regular_files": 0, "stat_errors": 0, "symlinks": 14590, "wall_s": 880.16}`. This also is not a content-checksum verification. The later incremental creation and removal used the same read-only lower mount and wrote only to the retained upper view.

## Versions and build

- dctx: clean local `main` at `4660f6deb1639cf9a3495989925eb075e7e44bc5`, CLI `0.25.0`. Source mirrored into owned runner scratch without `.git`, build products, package installs or private evidence. Built with `CARGO_TARGET_DIR=$W/build-target RUSTC_WRAPPER= taskset -c 24-31 cargo build --release --locked -p dctx-cli -j 8`. Engine source was not edited. Build: 134.32 s wall, 483.38 s user, 18.17 s system, 441,828 KiB peak RSS, exit 0.
- dctx Linux binary: 9,236,952 bytes; SHA-256 `db3d6739c9351224f1b72d8054ee5d6eaddd7ab19c2b06c2c718d0f04ec89eb1`. Cargo.lock SHA-256 `c40ce5727edcf651d85e468f131ff54c68253896981175eb8649865a83ef8b57` matched locally and on build-host. These are artifact hashes, not corpus verification.
- Public release `@trailhq/trail-code@0.23.1` installed in an empty scratch project with **pnpm 12.15.0**, using `pnpm add --save-exact --ignore-scripts --store-dir $W/pmcache/store @trailhq/trail-code@0.23.1`. The project records `packageManager: pnpm@12.15.0`. Install scripts were disabled. Runtime parser success is assessed by the synthetic build rather than inferred from installation. The retained lock pins transitive dependencies; SHA-256 `ce744bb3e3ea623b94702b7263547dc22878b4c568529303772f8b3036f9f860`. Regular node_modules footprint: 369,775,358 bytes / 8,523 regular files, plus 110 symlinks; Node runtime excluded.
- Setup side effect: the initial host `pnpm --version` preflight triggered Corepack's public pnpm 12.15.0 download in the runner account's normal cache. The actual Trail installation then explicitly used scratch HOME, COREPACK_HOME, PNPM_HOME, XDG_CACHE_HOME and store. No corpus content was supplied to package installation. The account cache was not cleaned because it is outside this card's owned scratch.
- Node `v24.22.0`; `rustc 1.99.0 (b940084d7 2026-09-28)`; `cargo 1.99.0 (5f94df478 2026-08-27)`; `zsh 5.9 (x86_64-debian-linux-gnu)`. GNU time version output was `time (GNU Time) UNKNOWN`; exact downloaded package filenames: `strace_6.13+ds-1_amd64.deb, time_1.9-0.2_amd64.deb, zsh_5.9-8+b24_amd64.deb`.
- The unmodified Node runtime's reported V8 heap-size limit was 4,496,293,888 bytes. No larger heap was requested. Disabling install scripts was a documented installation choice; only the Python control parser was verified, so native/optional parser coverage for other languages remains unassessed.
- Host: `Linux 6.12.111+deb13-cloud-amd64`, 32 vCPU, 98,974,768 KiB total RAM. Commands pinned to CPUs 24–31. Observed load1 before/after measured commands: 10.76–36.10. Other workloads were not changed. The initial dctx cold attempts overlapped this card's corpus inventory / read-only count probe; timings are contention-affected and describe failures, not standalone completed-index performance. Page caches were not dropped.

## Protocol reuse, isolation, and limits

The source protocol is `docs/research/trail-code-2026-10/`: `scripts/benchlib.py`, `phase_index.py`, `phase_query.py`, `phase_tasks.py`, `phase_adversarial.py`, `phase_coverage.py`, `select_targets.py`, and `goldlib.py`. The adapter imports the original `est_tokens` and GNU-time parser unchanged. Commands use the same CLI task mappings and CPU range; cold dctx runs use three empty caches. Full cold-build deadline is 3,600 s; other full probes use 900 s; synthetic probes use 300 s; timeout has a 5 s kill grace. Trail produced 60 scope graphs, 9,674 file nodes and 9,674 extraction/fingerprint file records, totaling 236,268 nodes and 226,164 edges. Those are exact stored-graph/cache counts, not a complete input exclusion classification. All generated scope index roots were absent in the lower input before the run. Across all scope stores after queries, the index occupies 692,416,225 logical bytes in 9,927 files; graph bytes 173,631,288, cache bytes 494,288,827. The post-build writable upper, before the query probes, was `{"files": 10049, "logical_bytes": 692549577}` including ignore-file and summary writes. Measuring root `graft/` alone would miss the scope indexes.

**Tokens** always means `ceil(stdout_UTF8_bytes / 3.5)`, an output-byte estimate, not measured model tokens, tokens consumed to reach a correct answer, or answer quality. No answer-producing model was invoked. stderr is counted separately and is not an answer. Correctness/provenance scores require a valid tool-independent gold set and are not inferred from byte counts or exit codes.

Every tool runs as the unprivileged runner account with `env -i`, scratch HOME/cache, `DO_NOT_TRACK=1`, `taskset -c 24-31`, and `unshare -Urnm`. The network namespace has only loopback and a connection to the documentation-only address `192.0.2.1:443` returned errno 101 (network unreachable). This is isolation evidence, not a complete syscall audit. No `--deep`, real credential, external model API, agent dispatch, or provider substitution was used. Network was permitted only for public package/bootstrap dependency setup in scratch, without corpus access by those commands.

Within each tool namespace the original lower path is bind-remounted read-only, then mounted through an overlay with per-tool `upper`, `work`, and `merged` directories in `$W/views/`. The overlay allows Trail's `.gitignore`, `.ignore`, and index writes without changing the lower input. No second full corpus copy was made. Namespace teardown removes mounts automatically; retained upper directories are private evidence. Synthetic env files live in a distinct four-file lower fixture and are never added to the real corpus.

The current dctx `crates/dctx/src/domain/limits.rs` and `crates/dctx-cli/src/pack.rs` compile ceilings of 100,000 entries, depth 256, 8 MiB per file, and 64 MiB total source bytes. The measured failure is the source-byte ceiling. The CLI has no permitted option to raise that ceiling; changing the engine is outside this card.

The previous protocol assumes a single Git corpus: `goldlib.list_files()` uses root-level `git ls-files`. This workspace root is not a Git repository. Reusing it unmodified would silently produce an empty oracle. No zero-sized gold set, sample substituted for the whole snapshot, or invented correctness score is reported. The original copy-per-tool and `git add` freshness steps were also not used: lower input is immutable, a second full copy is forbidden, and Git state-changing operations are not authorized.

The Trail incremental probe creates one artificial Python file beside a graph-covered source, queries its unique source-body marker, repeats the query with no change, removes the overlay file, and queries again. The baseline/add/no-op/remove receipt is `{"probe": "one new untracked Python file beside a graph-covered source, followed by removal; writes only in retained overlay", "steps": [{"maxrss_kb": 1503656, "rc": 0, "source_body_marker_present": false, "step": "baseline", "wall_s": 21.74}, {"maxrss_kb": 1500332, "rc": 0, "source_body_marker_present": false, "step": "added", "wall_s": 17.17}, {"maxrss_kb": 1490856, "rc": 0, "source_body_marker_present": false, "step": "added_noop", "wall_s": 16.43}, {"maxrss_kb": 1503564, "rc": 0, "source_body_marker_present": false, "step": "removed", "wall_s": 16.97}]}`. The source marker was not surfaced after addition, so these are query timings and a negative visibility observation, not evidence of a successful incremental update. Selection policy versus refresh behavior remains unresolved. The retained index byte delta spans the whole addition/removal sequence; it is not peak temporary growth.

## Exact commands and retained evidence

`$W=/path/to/workdir/bench/benchmark-2026-10-22`; `$TARGET_FILE` is the source target selected by the adapter and stored privately in `results/target-private.json`. Actual argv and targets remain in private `results/runs.jsonl`. The card's safe scripts (`runner-env.zsh`, `runner-sandbox.zsh`, `run-benchmark.py`, `export-aggregates.py`, `collect-trail-coverage.py`, `postcheck-corpus.py`, `write-report.py`) record the adapter. Invocations on build-host use `$W/runtime/usr/bin/zsh -f`; no shell bootstrap was installed system-wide.

```text
python3 $W/scripts/run-benchmark.py dctx
  dctx map find main --limit 10                  # 3 empty-cache cold runs
  dctx map find main --json --limit 10
  dctx map explain m:$TARGET_FILE --json
  dctx map map $TARGET_FILE --depth 2 --limit 40
  dctx map neighbors m:$TARGET_FILE --dir in --edge calls
  dctx pack .
python3 $W/scripts/run-benchmark.py privacy-dctx
python3 $W/scripts/run-benchmark.py coverage-dctx # separate traced attempt; not timing comparator
python3 $W/scripts/run-benchmark.py privacy-trail
python3 $W/scripts/run-benchmark.py trail
  trail build .                                # one scale attempt; not a 3-run median
  trail ask main                               # only if build completed
  trail ask main --source --full -n 1            # only if build completed
  trail map                                    # only if build completed
  trail callers main                           # only if build completed
  trail skeleton $TARGET_FILE                   # only if build completed
python3 $W/scripts/collect-trail-coverage.py
BENV_EXTRA=BENCH_VIEW=trail $W/scripts/runner-env.zsh python3 $W/scripts/collect-index-sizes.py
python3 $W/scripts/postcheck-corpus.py
find /path/to/workdir/bench/corpus -type f | wc -l
python3 $W/scripts/run-benchmark.py incremental-trail
BENV_EXTRA=BENCH_VIEW=trail $W/scripts/runner-env.zsh python3 $W/scripts/collect-index-sizes.py
```

Exact successful opens in the separate dctx trace are content-open observations before abort, not a completed scan or policy-exclusion census. Unopened input includes unvisited, omitted and excluded files; subtracting opens from 1,975,840 would fabricate an exclusion count. Failed/timeout Trail artifacts likewise do not prove complete coverage. Exact tool selection/exclusion accounting remains an explicit unmet requirement.

Sanitized aggregate rows are in `aggregate-a.json` next to this report. Private evidence is retained owner-only on build-host under `$W/results/`: `runs.jsonl`, build/install logs, `versions.json`, `corpus-stats.json`, `corpus-layout.json`, `corpus-metadata.jsonl`, `target-private.json`, `dctx-scan-openat.trace`, count/size/privacy receipts, and per-run captures below. Corpus metadata contains private paths and is not copied into docs. Evidence files with group/world permission bits: 0.

- `results/raw/dctx/cold_index.r0.err` (65 bytes)
- `results/raw/dctx/cold_index.r0.out` (0 bytes)
- `results/raw/dctx/cold_index.r0.time` (814 bytes)
- `results/raw/dctx/cold_index.r1.err` (65 bytes)
- `results/raw/dctx/cold_index.r1.out` (0 bytes)
- `results/raw/dctx/cold_index.r1.time` (813 bytes)
- `results/raw/dctx/cold_index.r2.err` (65 bytes)
- `results/raw/dctx/cold_index.r2.out` (0 bytes)
- `results/raw/dctx/cold_index.r2.time` (809 bytes)
- `results/raw/dctx/coverage_trace.r0.err` (65 bytes)
- `results/raw/dctx/coverage_trace.r0.out` (0 bytes)
- `results/raw/dctx/coverage_trace.r0.time` (991 bytes)
- `results/raw/dctx/edges_calls.r0.err` (65 bytes)
- `results/raw/dctx/edges_calls.r0.out` (0 bytes)
- `results/raw/dctx/edges_calls.r0.time` (853 bytes)
- `results/raw/dctx/explain.r0.err` (65 bytes)
- `results/raw/dctx/explain.r0.out` (0 bytes)
- `results/raw/dctx/explain.r0.time` (836 bytes)
- `results/raw/dctx/find.r0.err` (65 bytes)
- `results/raw/dctx/find.r0.out` (0 bytes)
- `results/raw/dctx/find.r0.time` (814 bytes)
- `results/raw/dctx/map.r0.err` (65 bytes)
- `results/raw/dctx/map.r0.out` (0 bytes)
- `results/raw/dctx/map.r0.time` (844 bytes)
- `results/raw/dctx/pack.r0.err` (65 bytes)
- `results/raw/dctx/pack.r0.out` (0 bytes)
- `results/raw/dctx/pack.r0.time` (795 bytes)
- `results/raw/dctx/privacy_find.r0.err` (0 bytes)
- `results/raw/dctx/privacy_find.r0.out` (97 bytes)
- `results/raw/dctx/privacy_find.r0.time` (758 bytes)
- `results/raw/dctx/privacy_pack.r0.err` (0 bytes)
- `results/raw/dctx/privacy_pack.r0.out` (1,346 bytes)
- `results/raw/dctx/privacy_pack.r0.time` (742 bytes)
- `results/raw/trail/cold_index.r0.err` (1,230 bytes)
- `results/raw/trail/cold_index.r0.out` (4,708 bytes)
- `results/raw/trail/cold_index.r0.time` (776 bytes)
- `results/raw/trail/edges_calls.r0.err` (0 bytes)
- `results/raw/trail/edges_calls.r0.out` (161,200 bytes)
- `results/raw/trail/edges_calls.r0.time` (761 bytes)
- `results/raw/trail/explain_source.r0.err` (0 bytes)
- `results/raw/trail/explain_source.r0.out` (3,220 bytes)
- `results/raw/trail/explain_source.r0.time` (782 bytes)
- `results/raw/trail/find.r0.err` (0 bytes)
- `results/raw/trail/find.r0.out` (1,281 bytes)
- `results/raw/trail/find.r0.time` (761 bytes)
- `results/raw/trail/incremental_add_query.r0.err` (117 bytes)
- `results/raw/trail/incremental_add_query.r0.out` (12,482 bytes)
- `results/raw/trail/incremental_add_query.r0.time` (806 bytes)
- `results/raw/trail/incremental_added_noop.r0.err` (0 bytes)
- `results/raw/trail/incremental_added_noop.r0.out` (12,482 bytes)
- `results/raw/trail/incremental_added_noop.r0.time` (801 bytes)
- `results/raw/trail/incremental_baseline.r0.err` (0 bytes)
- `results/raw/trail/incremental_baseline.r0.out` (12,482 bytes)
- `results/raw/trail/incremental_baseline.r0.time` (806 bytes)
- `results/raw/trail/incremental_remove_query.r0.err` (117 bytes)
- `results/raw/trail/incremental_remove_query.r0.out` (12,482 bytes)
- `results/raw/trail/incremental_remove_query.r0.time` (803 bytes)
- `results/raw/trail/map.r0.err` (0 bytes)
- `results/raw/trail/map.r0.out` (136,375 bytes)
- `results/raw/trail/map.r0.time` (753 bytes)
- `results/raw/trail/outline.r0.err` (0 bytes)
- `results/raw/trail/outline.r0.out` (97 bytes)
- `results/raw/trail/outline.r0.time` (785 bytes)
- `results/raw/trail/privacy_build.r0.err` (65 bytes)
- `results/raw/trail/privacy_build.r0.out` (308 bytes)
- `results/raw/trail/privacy_build.r0.time` (758 bytes)
- `results/raw/trail/privacy_grep.r0.err` (315 bytes)
- `results/raw/trail/privacy_grep.r0.out` (0 bytes)
- `results/raw/trail/privacy_grep.r0.time` (761 bytes)

## Verdict and exact remaining work

At the pinned defaults, **Trail completed the root build and provided find/source/map/caller outputs; dctx did not provide full-snapshot answers under its fixed source limit**. Trail therefore has an observed functional advantage on these root-level probes. Its graph coverage still needs qualification; successful CLI exit codes do not prove correct answers or that every eligible file was scanned. dctx's small failure RSS and zero stdout do not make it faster or more token-efficient at answering the tasks. The env fixture favors Trail for the tested non-disclosure behavior and exposes dctx pack's default disclosure of all three artificial values; it is not a claim about all private files. Neither system has a completed paired correctness/provenance evaluation here. **No overall answer-quality ranking, production readiness, public release, or security-completeness claim is made.**

Remaining for a complete part-a comparison:

1. Resolve the dctx full-corpus source ceiling through an owner/orchestrator decision outside this read-only engine card; preserve the same immutable full input or explicitly recut the benchmark scope.
2. Complete Trail cold-index repetitions (3 total successful baselines) and warm-query/no-op medians (5), including any runtime/resource blocker recorded above; one scale attempt is not the original protocol's median. Verify required native/optional parsers and resolve any installation-script restriction in isolated scratch before treating language coverage as equivalent.
3. Adapt and validate the independent oracle for this non-Git workspace root, covering all eligible files across nested repositories; generate the shared 20-target / 10-question set and score definition/call-edge correctness and source provenance without an external model.
4. Complete matched find, explanation/source, map/call-edge and task-output measurements after successful baselines; retain Trail pack as unsupported unless a real equivalent is defined. Never label output estimates as measured answer tokens or quality.
5. Complete controlled existing-file change and repeated incremental measurements. The single Trail untracked addition/removal probe above is not a median or a full freshness matrix; dctx still needs a successful baseline before it can be tested. Do not stage files with Git.
6. Produce exact per-tool selected/scanned, omitted, excluded, unsupported, failed and unvisited counts. Reconcile Trail's 60 scope graphs / 9,674 file records with all 1,975,840 input regular files, including nested repositories and files outside selected scopes. Current dctx early-abort open observations and Trail stored-graph/cache counts do not classify all exclusions or establish equivalent eligible-file coverage.


