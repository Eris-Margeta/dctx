# Scan / segment performance

The ignored `pipeline_perf::seeded_pipeline_benchmark` integration test generates
1,000, 10,000 and 50,000 source slots in a temporary repository, plus one
`.gitignore`. Seed: `0xd1113a5e20261113`; fixed LCG, 64 package directories,
Rust/JavaScript/TypeScript/Python/Go/Markdown, unique content, excluded `.secret`
files, binary-classified files, built-in rules and two project globs. It does not
read global configuration. Drafts represent synthetic declarations, not parser
performance. Python and Go use the codec's Opaque language tag with their extension
as dialect.

Scan includes rule loading, traversal, classification and hashing. Segment time
includes building both fact (declarations and postings) and view (nodes) envelopes;
draft construction and decoding are outside the measured interval. An independent
counting allocator in the integration-test binary tracks allocation/reallocation
events and **peak simultaneously live allocation blocks**, not RSS or allocated
bytes. Counters are thread-local so harness activity cannot contaminate them.
Inputs are borrowed, and measured outputs stay alive until counters are disabled.
The instrumentation is identical before and after; timings include its overhead.

## Reproduce

From the worktree root, with Rust/Cargo on PATH:

```sh
umask 022
export CARGO_TARGET_DIR="$PWD/target/mapperf-build"
export CARGO_HOME="$PWD/target/mapperf-cargo"
export TMPDIR="$PWD/target/mapperf-tmp"
mkdir -p "$TMPDIR" target/mapperf-baseline
export DCTX_PERF_BASELINE="$PWD/target/mapperf-baseline"
cargo test -p dctx-map-scan --release --test pipeline_perf -- --ignored --nocapture
```

On the baseline revision, this creates full scan/fact/view byte artifacts. Subsequent
runs compare every byte and never overwrite an existing artifact. The same test
without `DCTX_PERF_BASELINE` still checks three repeated outputs and independent
1/2/4-thread caller groups. The crates expose serial APIs (`IgnoreEngine` is not
`Sync`); these groups exercise actual concurrent complete pipelines, not internal
parallel scanning. `DCTX_PERF_SIZE=10000` optionally selects a single fixture for
controlled ablations; the default always runs all three sizes.

The normal (not ignored) test compares the 1k fixture to
`tests/golden/pipeline-1000.bin`, captured from the unmodified implementation,
checking it twice serially and at 1/2/4 caller counts. The golden is the complete
portable scan framing plus both segment envelopes, not only a digest. The ignored
test also decodes both segments and performs determinism checks at every size.
No timing threshold or sleep is a test assertion.

## Baseline

Measured 2026-11-13, Linux x86_64, Rust 1.100.0, release profile, default features.
Baseline production revision: `8b3d12ff8159b4600b37e3f89921f3c58815d622`.
Three serial measured passes per size, each with a fresh ignore engine; generation
is excluded. The first scan follows file creation; subsequent scans are warm.
These are shared-runner wall times, not CPU time or a cold-cache claim. Median is
reported; raw passes are retained below to expose IO/scheduling variance.

| Files | Scan ms (passes 0/1/2) | Segment ms (passes 0/1/2) | Scan events / peak live | Segment events / peak live |
| ---: | --- | --- | ---: | ---: |
| 1,000 | 105.070 / 62.842 / 59.588 | 44.628 / 43.350 / 43.700 | 16,226 / 5,551 | 107,315 / 2,853 |
| 10,000 | 1745.892 / 594.727 / 605.951 | 611.666 / 663.473 / 679.435 | 142,487 / 29,903 | 1,334,903 / 28,305 |
| 50,000 | 10536.681 / 2477.190 / 2637.350 | 3549.663 / 3474.711 / 3808.062 | 702,339 / 69,258 | 7,569,147 / 141,405 |

Allocation counts were identical across all three passes at each size.

## Before / after

The final verification run uses the final source and the same instrumentation.
All three full byte artifacts matched; allocation counts also matched the first
optimized run exactly. Times below are medians of three passes, in milliseconds.

| Files | Scan ms: before → final | Segment ms: before → final | Scan events: before → final | Segment events: before → final |
| ---: | ---: | ---: | ---: | ---: |
| 1,000 | 62.842 → 101.889 | 43.700 → 45.108 | 16,226 → 13,966 | 107,315 → 20,038 |
| 10,000 | 605.951 → 757.332 | 663.473 → 402.495 | 142,487 → 122,227 | 1,334,903 → 199,410 |
| 50,000 | 2637.350 → 2725.204 | 3549.663 → 2209.116 | 702,339 → 602,079 | 7,569,147 → 996,446 |

At 50k, segment construction is 37.8% faster in the final comparison. Scan is
3.3% slower despite fewer allocations; **these measurements do not establish a
scan wall-time speedup**. File creation/IO and shared-runner scheduling vary
substantially. Peak live counts are unchanged at every size; allocation churn
reductions are stable and independently isolated below.

Both optimized runs are retained here rather than discarding the noisier first
run. Pass 0 includes the first scan after creation; passes 1/2 are warm scans.

| Run | Files | Scan ms (passes 0/1/2) | Segment ms (passes 0/1/2) |
| --- | ---: | --- | --- |
| First optimized | 1,000 | 510.827 / 92.450 / 92.879 | 39.724 / 31.410 / 43.463 |
| First optimized | 10,000 | 2160.292 / 772.085 / 778.522 | 502.811 / 470.918 / 504.265 |
| First optimized | 50,000 | 11041.624 / 3130.355 / 3020.423 | 2727.011 / 2390.987 / 2229.243 |
| Final verification | 1,000 | 199.970 / 101.889 / 96.691 | 45.108 / 44.800 / 48.630 |
| Final verification | 10,000 | 3033.017 / 724.789 / 757.332 | 494.757 / 386.039 / 402.495 |
| Final verification | 50,000 | 7778.017 / 2705.184 / 2725.204 | 2316.227 / 2126.877 / 2209.116 |

## Optimizations and compatibility

1. **Fact sorting:** the previous `sort_by_key` serialized a 100-byte key into a
   growing `Vec` on every comparison, giving O(n log n) heap traffic. Compare the
   digest and remaining wire fields directly and lazily. Integer comparisons use
   little-endian byte arrays, preserving canonical wire order rather than numeric
   order. Randomized tie-field comparisons against the unchanged encoder and an
   explicit 256-before-1 regression test protect this distinction.
2. **Dictionary collection:** `remember` previously cloned every string before
   inserting it into a set, including repeated parser/revision/dialect/summary
   strings. Collect borrowed strings and allocate owned values only once per
   distinct string when constructing the dictionary. Ordering and existing limits
   remain unchanged for both fact and view segments.
3. **Glob matching:** every uncached glob allocated a 64-entry heap stack and
   pushed/popped deterministic character transitions. Use a 64-entry inline stack,
   spill to a `Vec` when necessary, consume literals directly, and materialize star
   alternatives lazily. There is no imposed path/pattern depth limit. Exhaustively
   compare 1,365 short patterns against 121 texts (165,165 pairs) using a frozen copy
   of the previous matcher; long paths also exercise heap spill. Existing legacy
   wildcard behavior, including malformed double stars and slash boundaries, is
   preserved rather than changed to a different glob dialect.

The measured peak live allocation count stays unchanged: retained scan records,
ignore caches and segment structures dominate that peak. These changes reduce
allocation churn, not the amount of retained output. No filesystem reads, privacy
decisions, public APIs, thread model, segment schema or hash algorithm changed.

## Isolating the three allocation hotspots

Each 10k ablation restores only the named old implementation while retaining the
other optimizations, then runs the same three-pass benchmark and complete byte
and caller-count checks. Allocation counts are identical in every pass, unlike
wall times on this shared runner. Peaks are 29,903 scan / 28,305 segment throughout.

| 10k variant | Scan allocation events | Segment allocation events | Extra events versus optimized |
| --- | ---: | ---: | ---: |
| All optimized | 122,227 | 199,410 | 0 |
| Old fact sort only | 122,227 | 1,259,522 | 1,060,112 segment |
| Old dictionary cloning only | 122,227 | 274,791 | 75,381 segment |
| Old glob matcher only | 142,487 | 199,410 | 20,260 scan |
| All old (baseline) | 142,487 | 1,334,903 | 20,260 scan + 1,135,493 segment |

This ranks the three targeted allocation hotspots and accounts for the complete
measured allocation reduction, without inferring per-hotspot CPU savings from
separate noisy wall-clock runs. At 50k, allocation events fall from 702,339 to
602,079 for scan and from 7,569,147 to 996,446 for segment construction.

To recapture the baseline, retain the benchmark harness and its development
dependency, and use the baseline revision's versions of the four changed
production files (`glob.rs`, `compile.rs`, `dictionary.rs`, `facts.rs`). Keep the
resulting baseline directory for the optimized run. The 1k golden is checked in;
all three output digests are also fixed in `tests/support/repo.rs`, so subsequent
ignored runs detect drift even without the optional large baseline artifacts.

## Validation

The required check passed on the final production and test code:

```sh
cargo test -p dctx-map-scan -p dctx-map-segment
cargo test -p dctx-map-scan -p dctx-map-segment
cargo clippy -p dctx-map-scan -p dctx-map-segment --all-targets -- -D warnings
```

Each test run: 122 scan unit tests, 2 integration tests, 37 segment unit tests;
**161 passed, 1 ignored, 0 failed**; both crates have zero doctests. Clippy exits 0.
The integration tests include a counter self-check for reallocation events versus
peak live blocks. The full ignored benchmark and all three ablations also pass.
The only change outside the two crates is the required `Cargo.lock` dependency
edge for scan's development dependency on segment; no new external crate is added.
