<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# DCX-PERFCI-01 — performance CI and flaky-test audit

Started: 7 November 2026; validation continued on 8 November. Engine revision:
`bb923103f85e7a3d9ac97db696fe23182b78f55c`. No commit created.

## Delivered guard

The dedicated [performance workflow](../../.github/workflows/performance.yml)
builds the locked CLI and catalog benchmark with Rust 1.97.1, then checks the
[committed baseline](../../tools/bench/perf-baseline.json). The stdlib-only
[runner](../../tools/bench/perf_ci.py) generates 256 Rust files, performs five
native and three Cachegrind measurements per case, validates coverage and warm
state, and fails on any threshold violation. CI retains the raw JSON report.
`refresh` is an explicit local subcommand and never runs in CI.

The [method and refresh instructions](../../tools/bench/PERF-CI.md) document the
cache states, compiler and corpus controls, limitations and thresholds:
instructions above 1.25 times baseline; native wall time above the larger of
3 times baseline and baseline plus 150 ms. The existing offline benchmark
protocol/harness and catalog CLI remain the experiment boundaries; there are no
model calls or new production dependencies.

Baseline capture used Linux x86-64, glibc 2.41, Valgrind 3.22.0 and
`rustc 1.97.1 (8bab26f4f 2026-07-14)`, with
`RUSTFLAGS='-C target-cpu=x86-64'`. All raw samples, binary hashes, source revision,
runner/corpus hashes and flags are in the baseline. Wall samples reflect a shared
host with substantial storage contention; instruction counts provide the stable
gate. GitHub's Ubuntu job itself was not executed from this worktree.

| Case | Baseline instructions | Baseline wall median | Independent check instructions | Independent wall median |
| --- | ---: | ---: | ---: | ---: |
| Cold build | 1,145,416,061 | 1.515139 s | 1,145,377,907 | 1.065296 s |
| Warm targeted read | 84,392,695 | 0.021140 s | 84,390,870 | 0.023014 s |
| Warm find | 64,961,738 | 0.192826 s | 64,960,077 | 0.071845 s |
| Pack directory | 202,842,641 | 0.061141 s | 203,322,216 | 0.077524 s |
| Catalog build/queries | 746,480,309 | 0.258609 s | 746,532,109 | 0.147726 s |

All ten independent checks passed. Targeted reads intentionally use ephemeral
scoped generations: warming repeats the same read and verifies at most one
indexed source-content read, rather than requiring persisted generation reuse.
Warm find does require generation reuse. The catalog output lives in a sibling
scratch directory because its walker prunes the output directory.

## Commands and results

All writable Cargo caches, targets, temporary directories and benchmark scratch
were placed inside this worktree. Common local gate environment:

```sh
umask 022
export CARGO_HOME="$PWD/target/perfci/cargo-home"
export CARGO_TARGET_DIR="$PWD/target"
export TMPDIR="$PWD/target/perfci/tmp"
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
```

Debug symbols and incremental caches were disabled to fit available disk space;
test assertions and concurrency were retained. An initial cold compilation was
stopped before any Rust test ran to remove its large debug artifacts. Initial
offline dependency resolution lacked `schemars`; `CARGO_NET_OFFLINE=false cargo
fetch --locked` populated the local cache successfully before test execution.

| Command | Result |
| --- | --- |
| `cargo build --release --locked -p dctx-cli` (Rust 1.97.1, CPU flags above) | PASS |
| `cargo build --release --locked --manifest-path tools/catalog-bench/Cargo.toml` (same flags; standalone target directory) | PASS |
| `python3 tools/bench/perf_ci.py refresh --report target/perfci/final-capture.json` | PASS; measured baseline written |
| `python3 tools/bench/perf_ci.py check --report target/perfci/final-check.json` | PASS; all ten metrics |
| `python3 tools/bench/perf_ci.py check --report target/perfci/ubuntu-report.json` (Ubuntu Valgrind 3.22.0 package on this host) | PASS; all ten metrics; confirms the counter version used by Ubuntu 24.04 supports the guard |
| `python3 tools/bench/perf_ci.py check --metric wall --report target/perfci/final-wall.json` | PASS; all five native metrics |
| `python3 -m unittest discover -s tools/bench/tests -v` | PASS; 25 tests, including every-case regression injection, invalid/missing measurements, wall fallback, reproducible corpus failing CLI exit without baseline mutation, and stale-report removal on failed measurement |
| `bash tools/release/lint-workflows.sh` | PASS; actionlint 1.7.7 |
| `bash scripts/spdx_check.sh` | PASS |
| `cargo fmt --all --check` (stable Rust) | PASS |
| `cargo clippy --workspace --exclude dctx-desktop --all-targets --locked -- -D warnings` (stable Rust 1.99.0) | PASS, including both test fixes |

The pinned benchmark compiler's Clippy 1.97.1 reported the existing
`manual_option_zip` diagnostic in `dctx-lex-md/src/lexer/inline.rs:89`. The required
workspace Clippy gate passes on installed stable Rust 1.99.0; no Markdown lexer
production change or warning suppression was introduced.

## Every observed failing test

1. `dctx::stamp_equiv::stamp_equiv_two_hundred_random_edits_answer_every_verb_like_a_cold_rebuild`
   failed in the initial audit, at edit 95. Replaying the fixed RNG sequence
   identified kind 9, the two same-size edits with identical timestamps. Its
   timestamp was only 300 ms old before a durable intermediate snapshot; storage
   delay could age it beyond the two-second racy window. The test then entered
   the explicitly documented settled same-stamp blind spot. The fixture now uses
   one future timestamp for both edits, keeping it unsettled throughout the
   bounded test run without a sleep, retry, or weaker assertion.
2. `dctx_core::source_files::archive_fallback_keeps_nested_sources_and_ignores_nonregular_entries`
   failed in the initial audit. A workspace-local `TMPDIR` let Git discover the
   ancestor checkout, so the test never entered the archive fallback and returned
   no tracked files. The test-support helper now accepts a Git command per call;
   this fixture selects a definitely absent executable to exercise the actual
   fallback independently of PATH, Git availability or temp location. It avoids
   process-global environment changes while tests run in parallel.
3. `dctx_map_query::repository::dctx_repository_recall_and_latency` failed in
   validation pass 1 with SQLite `DiskFull` at `Index::refresh`, after 386.93 s.
   The shared filesystem fell to approximately 44 MiB free while other workers
   were active. This is an external storage failure, rather than an assertion
   failure. Completed lint/profiler artifacts and unused platform caches were
   removed from this worktree; additional shared capacity was requested because
   this task cannot clean other worktrees. The full repository fixture and its
   assertions remain enabled.

No per-test retries, skips or ignored attributes were added. All assertions and
the 200-edit property sequence remain enabled.

## Full-suite repetitions

Each pass runs the entire required workspace suite, including desktop API tests
and doctests, with only the requested desktop application excluded:

```sh
RUSTUP_TOOLCHAIN=1.97.1 CFLAGS=-O2 cargo test --workspace --exclude dctx-desktop --locked --no-fail-fast
```

`--no-fail-fast` records every failing test target instead of stopping at the
first one. Passes run sequentially; no individual failing test is retried.
The initial audit was stopped while `dctx_repository_recall_and_latency` was
still running after exposing the two failures above; it is not counted as a
completed repetition. A local two-second `perf record` profile identified
bundled SQLite FTS writes as the active bottleneck. The five complete validation
passes compile that native dependency with `CFLAGS=-O2`; Rust's default test
assertions, debug assertions, test selection and concurrency remain enabled.
An ignored, worktree-local process monitor suspends only the repository-index
test when shared free space is low and resumes that same process when capacity
returns. It does not restart or skip tests. Repetition durations include these
resource waits; they are not benchmark claims. The monitor never targets the
benchmark processes. Final baseline capture and
independent validation used Ubuntu Valgrind 3.22.0 on this host; this does not
claim execution of the hosted GitHub job itself.

| Complete pass | Passed | Failed | Ignored | Exit | Elapsed |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 2,332 | 1 (`DiskFull`) | 0 | 101 | 634.44 s |
| 2 | 2,333 | 0 | 0 | 0 | 1,172.68 s |
| 3 | 2,333 | 0 | 0 | 0 | 1,543.36 s |
| 4 | 2,333 | 0 | 0 | 0 | 2,102.90 s |
| 5 | 2,333 | 0 | 0 | 0 | 1,992.96 s |

Five complete suite runs executed. Runs 2–5 passed every test after disk capacity
recovered; the only failed complete run was the recorded `DiskFull` failure.
Both repaired flaky tests passed in every complete run. No tests were ignored.
The final required format, Clippy and test gates pass. Workflow validation,
licence checks, all 25 Python harness tests, the final ten benchmark comparisons
and the explicit wall-only fallback also pass. The baseline matches the final
runner hash, the diff contains no website changes, and HEAD remains unchanged.
