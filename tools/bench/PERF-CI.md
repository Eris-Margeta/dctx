<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Synthetic performance regression guard

[performance.yml](../../.github/workflows/performance.yml) runs offline after
building the locked release binaries. This complements the
[benchmark protocol](../../docs/BENCHMARK-PROTOCOL.md) and the
[offline experiment harness](README.md): it measures local engine operations,
not model quality, agent sessions, billed tokens or real-world speedups.
It also exercises the existing [catalog benchmark](../catalog-bench/README.md).
No new production dependencies or engine implementations are introduced.

The generated corpus has 256 Rust files in 16 directories, each with 32 functions,
plus a unique `perf_target` function and approximately 434 KiB of comment padding
in its file. All names, contents and old modification times are fixed. The corpus
hash is checked against the committed `perf-baseline.json`. Generation, cache
cleanup and warm preparation are outside measured intervals. Every subprocess
has a 180-second timeout; the CI job has a 30-minute limit including compilation.

| Case | Measured operation and cache state |
| --- | --- |
| `cold_build` | Actual CLI `map find perf_target --json` with empty isolated engine/map caches, including index construction and answer encoding |
| `warm_read` | Actual CLI targeted symbol read after a completed read of the same target; body sentinel and at most one indexed source-content read are mandatory |
| `warm_find` | Actual CLI find after a completed find; generation reuse and sentinel are mandatory |
| `pack` | Actual CLI pack of the complete mid-size directory, including source rendering, after one completed pack |
| `catalog_build` | Existing catalog-bench build with one walker thread, including validation and its seeded query workload; exactly 256 files and zero walk errors required |

Cold means **empty dctx caches**, as in the protocol; generated files have warm
OS metadata/pages. No filesystem cache eviction, network, provider or agent loop
is involved. Wall measurements include process start, stdout/stderr capture and
exit. Each invocation must succeed and pass output checks; failures are never
retried or discarded. Warm preparation cost is excluded and described above.
Targeted reads use ephemeral scoped generations and bypass persisted cache
admission: their warm state is a repeated operation over warm filesystem pages,
not generation reuse. The diagnostic source-read counter is emitted after index
preparation, before the query's additional source verification. Warm find uses
the persisted generation and must report its reuse.

Five native samples per case gate their median wall time. Three additional
Cachegrind samples gate median executed **guest instructions (`Ir`)**, with cache
and branch simulation disabled. Instrumented elapsed time is never used as native
wall time. Instruction counts cover the complete process, including loader,
engine, worker threads, encoding and teardown; they avoid CPU-frequency and
scheduler noise. Kernel instructions and I/O waits are outside `Ir`; the native
wall gate covers large latency changes there. Rayon and the catalog walker use
one thread. CI uses Linux x86-64, Rust 1.97.1, locked default features, the release profile and
`RUSTFLAGS='-C target-cpu=x86-64'`. Compiler, platform, corpus, runner source or
policy changes fail closed and require an intentional refresh. Reports include the
source revision and binary hashes,
compiler, libc and Valgrind versions, raw samples and medians, without machine names
or absolute worktree paths.

Thresholds are committed in the runner and baseline:

* Instructions: fail above **1.25 × baseline median** (25% allowance for allocator,
  runtime, loader and traversal variation across Linux runners).
* Native wall: fail above **max(3 × baseline median, baseline median + 150 ms)**.
  These generous bands catch large latency regressions without making shared
  hosted runners a precision timing service.

The instruction gate is mandatory in CI. Local `--metric wall` explicitly selects
the noisy fallback when Valgrind is unavailable; it cannot refresh the baseline.
Valgrind failures or missing counters never silently downgrade to wall time.
Valgrind/libc updates can affect startup counts: inspect raw samples and provenance
before accepting a refresh. This small corpus catches broad regressions and does
not replace the separate scale benchmarks or native Windows measurements.

To check or intentionally refresh, from the repository root:

```sh
export RUSTUP_TOOLCHAIN=1.97.1
export RUSTFLAGS='-C target-cpu=x86-64'
export CARGO_INCREMENTAL=0
cargo build --release --locked -p dctx-cli
cargo build --release --locked --manifest-path tools/catalog-bench/Cargo.toml
python3 -m unittest discover -s tools/bench/tests -v
python3 tools/bench/perf_ci.py check
# Only after reviewing an intended engine, compiler, corpus or runtime change:
python3 tools/bench/perf_ci.py refresh
git diff -- tools/bench/perf-baseline.json
python3 tools/bench/perf_ci.py check
```

Install Valgrind through the host package manager first. A refresh overwrites only
the selected baseline and report, runs every case again, and never runs during a
CI check. Review the measured change and its reason with the JSON diff; do not
refresh merely to turn a regression green. Default disposable trees, caches and
reports live under `target/perfci/` inside the worktree and trees are removed even
on ordinary failures. `--baseline`, `--report`, `--scratch`, `--dctx` and `--catalog`
support explicitly selected local paths; writable paths must remain in this tree.
An old report is removed before measurement so a failure cannot upload stale data.
