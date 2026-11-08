# DCX-STAMP-01

Base: `da25f2c346fb6555fc44c3f85c011733bd653739`. No commit created.

**Result: stamp refresh and equivalence pass; the 50 ms warm-query p50 requirement is NOT met.** The final quiet-run p50 is 285.83 ms on this checkout and 88.91 ms on the synthetic 4k tree. This card is incomplete on latency acceptance.

Implementation stays in `crates/dctx/src/session/**`, `crates/dctx/src/index/**`, and tests. This report is the requested documentation exception. Filesystem adapters, rule loading, and website files are untouched.

Inventory comparison uses path, decision, and size, with duplicate paths rejected. Changed or missing stamps trigger digest verification. Matching content retains the parsed graph, refreshes live modification times and stamps, recomputes the canonical snapshot digest when modification times change, and persists the refreshed generation. No source parsing occurs during refresh. A content mismatch rebuilds. Failed earlier reads cannot certify equality. Racy files retain no trusted stamp and are digested on every invocation.

Decoded generations share immutable graph storage; metadata changes use copy-on-write. The decoder interns path and edge-label spans without temporary per-reference strings. Reuse inventory walks retain file rows without constructing module nodes; all source, policy, cancellation, deadline, and resource checks remain in the existing walker. Hash inventories retain duplicate detection. Refresh publication checks that its input view is still current, and lease acquisition cannot pin a different view. Existing handles retain their original metadata and snapshot identity.

Opt-in profiling adds `reuse load`, `reuse decode`, `reuse check`, and `reuse pin` stages alongside walk and query. Tests retain exact profile-stage assertions, expanded for these additional stages.

## Validation

Final gate commands ran with `umask 022`, the Rust toolchain on `PATH`, `CARGO_TARGET_DIR="$PWD/target"`, and temporary state under `target/stamp-bench`.

```sh
cargo fmt --all --check
cargo clippy --workspace --exclude dctx-desktop --exclude dctx-desktop-api --all-targets --locked -- -D warnings
cargo test --workspace --exclude dctx-desktop --exclude dctx-desktop-api --locked
cargo build --release --locked -p dctx-cli && python3 tools/engine-contract/warm_read.py target/release/dctx-cli && python3 tools/engine-contract/parity.py target/release/dctx-cli
```

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | PASS, exit 0 |
| Workspace clippy with `-D warnings` | PASS, exit 0 |
| Workspace tests | PASS, exit 0; 2,427 passed, 0 failed, 0 ignored across 262 suite summaries |
| Release build | PASS, exit 0 |
| `warm_read.py` | PASS; 0 cache opens, 3 target opens, zero unrelated source opens/reads across 32 branches |
| `parity.py` | PASS; 2 cases, 3 surfaces, 6 byte comparisons |

`stamp_equiv.rs` now checks one-file and multi-file touches, exact source-read paths independently of session counters, zero rebuild reads, unchanged generation identity across invocations, persisted settled stamps, repeated racy-window digestion, and equality with fresh strict-build output for every verb. The 200-edit test now explicitly requires touch-only steps to reuse. Additional regressions cover omitted-content metadata, existing handle immutability, stale-view pin/refresh rejection, recovery after failed reads, and inventory record limits. Existing same-size/same-mtime and strict-live tests passed.

During development, a focused profile test first rejected the new stage list; its exact expected list was extended from nine to thirteen stages. An early workspace run failed the new existing-handle regression while implementation and compilation overlapped. Superseded validation runs were stopped; the final complete workspace command above ran to exit 0. No assertion, threshold, or security check was weakened, and no skips were added.

## Performance

The reproducible harness is `crates/dctx/tests/stamp_bench.py`. It queries this checkout and a synthetic tree of 4,000 settled Rust files, arranged in 40 directories, with two functions per file. Each measured query is a separate release process using a persistent isolated cache, `DCTX_AUTO_CLEAN=0`, and `DCTX_REUSE_REPORT=1`. It runs two preparation queries before 20 measured invocations of `map find main --limit 5`. p50 is the median; p95 is the nearest-rank 19th ordered sample. Profiles are separate from the timed samples. Every timed run must reuse with zero rebuilt or verified content reads. The harness exits 1 when either p50 exceeds 50 ms.

```sh
python3 crates/dctx/tests/stamp_bench.py target/stamp-bench/dctx-before before
python3 crates/dctx/tests/stamp_bench.py target/release/dctx-cli after
```

`dctx-before` is the release executable built from the base revision, saved before the implementation changed. Both final comparisons use the same checkout contents. Raw samples, profiles, and gate logs are in `target/stamp-bench`.

| Corpus | Inventory / nodes | Before p50 / p95 (ms) | After p50 / p95 (ms) | p50 <= 50 ms |
|---|---:|---:|---:|---|
| This checkout | 2,608 / 87,577 | 308.47 / 334.40 | 285.83 / 293.78 | FAIL |
| Synthetic Rust tree | 4,000 / 12,000 | 102.95 / 104.80 | 88.91 / 90.58 | FAIL |

These final measurements ran after the workspace tests and builds finished. Both benchmark commands exited 1 because the unchanged 50 ms p50 threshold failed. Every measured run reported `generation reused`, `content_reads 0`, and `cached_content_reads 0`; stamp-skipped reads were 2,596 and 4,000 respectively. The other 12 checkout inventory rows omit content. Encoded generations were 9,450,594 and 1,568,042 bytes.

An earlier comparison taken while the repository-scale test was active measured checkout before 290.31/298.50 ms and after 279.10/572.27 ms (p50/p95). Those samples remain in `before-loaded.json` and `after-loaded.json`; the quiet repeat above resolves that p95 measurement concern. It does not resolve the acceptance failure.

Representative opt-in profile traces, in milliseconds (separate from the 20 timed samples):

| Phase | Before checkout | After checkout | Before 4k | After 4k |
|---|---:|---:|---:|---:|
| Walk | 105.36 | 98.08 | 60.80 | 53.45 |
| Retained load | 5.46 | 5.16 | 1.56 | 1.30 |
| Decode | 50.51 | 41.90 | 13.65 | 7.23 |
| Inventory comparison | 1.69 | 0.58 | 2.86 | 0.80 |
| Pin/finalization | 1.83 | 1.63 | 1.62 | 1.34 |
| Query | 12.51 | 13.42 | 3.49 | 3.92 |

The detailed before traces came from a diagnostic release build with only the extra timers added, before the algorithm changes; the timing comparison uses the saved unmodified base binary. Final base-binary profiles reported aggregate persist/reuse time of 73.34 ms and 17.10 ms. Profile traces are individual observations, not phase percentiles. Warm read, hash, parse/lex, span conversion, graph assembly, and canonicalize/seal stages were zero in both final corpora.

The remaining walk alone exceeds 50 ms in both corpora. The current source implementation stats directory entries and then resolves ancestor handles again for each file stat (`crates/dctx/src/adapters/fs/unix.rs`). The CLI rule walker repeatedly opens the root and recompiles selection policy (`crates/dctx-cli/src/host/rules.rs`). These are in DCX-MAC-01's assigned area and were not changed here. Owned node-string decoding also remains significant on the checkout's 87k-node graph. Further optimization is needed in both areas; the latency requirement cannot be claimed complete from this patch.

The CLI enables its profiler after session opening. Consequently, the difference between process wall time and profiled snapshot/query stages includes rule loading, session/cache setup, and teardown; it is not attributed exclusively to rule loading. A `perf record`/`perf report` sample of the warm base command showed rule parsing/matching/globbing, allocation, and kernel path traversal among the active costs.

A separate release smoke check on the 4k tree changed one file's settled mtime without changing bytes. It reported `generation reused`, `content_reads 0`, `cached_content_reads 1`, and `stamp_skipped_reads 3999`; the following invocation reported 0 verified reads and 4,000 stamp skips. A plain recent touch remains subject to the existing racy window: it is digested until settled, then verified once to persist its settled stamp. The integration test uses a future mtime to prove that such files are repeatedly digested without relying on test duration.

Raw final samples in milliseconds:

```text

before repo: 329.532, 308.117, 323.677, 306.012, 334.397, 308.819, 322.487, 311.252, 332.778, 329.168, 301.333, 301.975, 311.871, 304.755, 300.651, 293.878, 296.410, 337.837, 296.491, 304.633
before synthetic: 107.167, 102.951, 101.354, 100.032, 103.499, 103.445, 103.719, 102.957, 103.767, 104.798, 104.483, 103.632, 103.219, 100.619, 99.684, 101.440, 98.947, 100.594, 99.471, 100.632
after repo: 286.021, 284.148, 278.917, 286.722, 283.696, 286.307, 288.097, 286.895, 285.765, 283.859, 285.867, 279.634, 286.697, 286.383, 304.802, 285.784, 293.778, 275.359, 280.842, 284.769
after synthetic: 88.385, 89.275, 93.361, 88.373, 89.203, 88.995, 88.314, 87.733, 87.982, 90.575, 88.442, 87.664, 89.001, 87.353, 90.567, 88.829, 89.497, 87.902, 89.602, 89.791
```
