<!-- SPDX-License-Identifier: Apache-2.0 -->
# DCX-WARM-02

Base revision: `263efad9fc1b8dc1b297f7a3d4b962a7a924ac3c`. No commit created.

**Result: the 4k target passes; the checkout target remains unmet.** Warm p50 is 94.43 ms on the checkout and 31.63 ms on the synthetic tree. All required gates pass; latency acceptance for this card is incomplete.

## Implementation

The source port offers an optional invocation-scoped inventory cursor. The Unix
cursor retains a DFS stack of no-follow directory streams. Each child directory
is opened once relative to its pinned parent; each entry is inspected with
`fstatat(AT_SYMLINK_NOFOLLOW)` against that stream's descriptor. The same stat
supplies kind, metadata and the settled stamp. Ordinary targeted stats and reads
still resolve confined handles afresh. Root boundary canonicalization, no-follow
flags, regular-file checks, racy-window handling and resource limits are retained.

CLI rule discovery records this inventory while loading nested rules in the
original sorted DFS order. The first snapshot consumes those records, without a
second filesystem walk. Subsequent snapshots use fresh cursors; a before-index
plugin invalidates prepared records. Explicit scopes share their applicable rule
walk too. Rule parsing is reused until accumulated text changes. Missing nested
rule leaves are discovered from inventory, while existing leaves still use the
strict bounded policy reader. Root `.gitignore`, nested rewrites, final
`.dctxignore`, global configuration, pruning and depth limits keep their ordering.

Selection uses indexes of exact names, exact paths and suffix candidates. The
highest matching original rule index determines the decision, preserving layered
last-match semantics. Glob matching avoids allocating for literal runs and
rejects impossible literal suffixes before backtracking, including the existing
zero-length `**/` behavior.

Decoded node fields share one owned, validated string-table buffer and ranges.
Each field stores a shared table reference and an index, rather than allocating
its own string. Answer records materialize owned strings at the output boundary.
The persisted format, complete decoder validation and all query fields remain
unchanged. The profile now also reports host opening, including rule discovery,
inventory acquisition and session/cache setup.

## Validation

Final validation commands use `umask 022`, the installed Rust toolchain on `PATH`, and
`CARGO_TARGET_DIR="$PWD/target"`, `CARGO_HOME="$PWD/target/cargo-home"`.
Temporary validation state stays under `target/DCX-WARM-02`. The ordinary test run uses its
real `private-tmp` directory (mode 0700); the second uses `linked-tmp`, a symlink
to that directory.

```sh
cargo fmt --all --check
cargo clippy --workspace --exclude dctx-desktop --exclude dctx-desktop-api --all-targets --locked -- -D warnings
cargo test --workspace --exclude dctx-desktop --exclude dctx-desktop-api --locked
TMPDIR="$PWD/target/DCX-WARM-02/linked-tmp" cargo test --workspace --exclude dctx-desktop --exclude dctx-desktop-api --locked
cargo build --release --locked -p dctx-cli && python3 tools/engine-contract/warm_read.py target/release/dctx-cli && python3 tools/engine-contract/parity.py target/release/dctx-cli
python3 tools/engine-contract/warm_inventory.py target/release/dctx-cli
```

| Gate | Result | Duration (s) |
|---|---|---:|
| Formatting | PASS, exit 0 | 3.75 |
| Workspace clippy, warnings denied | PASS, exit 0 | 0.52 |
| Workspace tests, real TMPDIR | PASS, exit 0 | 792.20 |
| Workspace tests, symlinked TMPDIR | PASS, exit 0 | 728.37 |
| Release build | PASS, exit 0 | 6.67 |
| Warm read syscall contract | PASS, exit 0 | 0.24 |
| Transport byte parity | PASS, exit 0 | 0.10 |
| Inventory syscall contract | PASS, exit 0 | 1.29 |

Each workspace run reports 2,439 passed, zero failed and zero ignored across 264 suite summaries.

```text
Warm-read syscall contract PASS: expected cache set is empty; 0 cache opens; 3 target opens; zero unrelated source opens/reads across 32 branches
```

```text
Golden parity PASS: 2 cases, 3 surfaces, 6 byte comparisons
```

Regression coverage includes pinned and swapped inventory ancestors, link stamps
and refused content reads; one-use inventory replay and explicit invalidation;
exact nested rule bytes and pruning; adding, changing and deleting nested rules
across separate CLI processes compared with fresh caches; compiled rule candidate
equivalence; and exhaustive comparison of 472,626 short glob/text pairs against
the original matcher, plus Unicode cases. Existing decoder round-trip, corruption,
stamp-equivalence and symlink-boundary suites remain enabled. The profile test's
exact stage list includes the new host phase. No assertion, threshold or security
check is weakened, and no skips are added.

Additional comparison command:

```sh
python3 target/DCX-WARM-02/compare-outputs.py
python3 -m py_compile tools/engine-contract/warm_inventory.py crates/dctx/tests/stamp_bench.py
```

The comparison passes 136 byte-for-byte checks between the saved unmodified
binary and the updated binary: eight fixtures, seven map verbs plus pack, text
and JSON, global and nested rule combinations, ignore-mode configuration and
16 touch-only refresh comparisons. Its local script and log are under
`target/DCX-WARM-02`. Python compilation also passes. The inventory syscall
contract additionally passes with 4,000 unique no-follow fd-relative file stats,
40 group directories opened exactly once and zero source-file opens.

## Performance

The final measurements use `crates/dctx/tests/stamp_bench.py`: two preparation
queries, then 20 separate release processes per corpus and binary, followed by
one separate profile invocation. The query is `map find main --limit 5`.
The p50 is the median and p95 is nearest-rank sample 19 of 20. Every timed query
must reuse with zero source rebuild reads and zero verified content reads. The
50 ms threshold and sample count are unchanged. The harness additionally checks
stable stdout and records its hash and the profile process's wall time.

The before executable is the unmodified release build saved before implementation.
A disposable archive of the base revision, with only the host-opening timer added,
supplies the separate before profile. Its output must equal the timed binary's.
The baseline preparation commands were:

```sh
# Before implementation, at the base revision:
cargo build --release --locked -p dctx-cli
mkdir -p target/stamp-bench
cp target/release/dctx-cli target/stamp-bench/dctx-before
# Separate disposable timer-only profile build:
mkdir -p target/DCX-WARM-02/baseline-profile
git archive HEAD | tar -x -C target/DCX-WARM-02/baseline-profile
# Apply only the host_start/host_elapsed/host-open-print hunk from map/mod.rs.
CARGO_TARGET_DIR="$PWD/target/DCX-WARM-02/baseline-profile/target" cargo build --manifest-path target/DCX-WARM-02/baseline-profile/Cargo.toml --release --locked -p dctx-cli
```

Both baseline builds pass (100 s and 95 s respectively). The diagnostic CLI
`map/mod.rs` matches the final file exactly; all other diagnostic sources are
from the base archive. Only the saved unmodified executable supplies the 20
before timing samples.

Both binaries query the same working-tree contents and the same settled 4,000-file
fixture, using separate isolated persistent caches. Final benchmark commands run
after builds and tests finish, without concurrent validation load.

```sh
python3 crates/dctx/tests/stamp_bench.py target/stamp-bench/dctx-before before --profile-binary target/DCX-WARM-02/baseline-profile/target/release/dctx-cli
python3 crates/dctx/tests/stamp_bench.py target/release/dctx-cli after
```

Raw benchmark samples and profiles are stored in `target/stamp-bench/before.json`
and `after.json`. Gate commands, exit statuses and durations are recorded in
`target/DCX-WARM-02/gates.json`; individual logs and syscall traces are alongside.

| Corpus | Files / nodes | Before p50 / p95 (ms) | After p50 / p95 (ms) | p50 <= 50 ms |
|---|---:|---:|---:|---|
| Checkout | 2,616 / 87,679 | 287.64 / 293.10 | 94.43 / 97.22 | FAIL |
| Synthetic Rust tree | 4,000 / 12,000 | 87.51 / 91.98 | 31.63 / 33.67 | PASS |

Both final benchmark commands exit 1 because the checkout p50 exceeds 50 ms. All 80 measured invocations reuse with zero rebuild reads and zero verified content reads. Before/after stdout hashes match for both corpora.

Checkout: 2,604 stamp-skipped files, 112,443 edges, 9,467,967 retained-generation bytes. Counts describe the sampled tree; this report is completed after sampling.

4k: 4,000 stamp-skipped files, 12,000 edges, 1,568,041 retained-generation bytes. Counts describe the sampled tree; this report is completed after sampling.

Representative separate profiles, in milliseconds (individual observations, not phase percentiles):

| Phase | Before checkout | After checkout | Before 4k | After 4k |
|---|---:|---:|---:|---:|
| host open | 100.35 | 22.95 | 14.60 | 12.21 |
| walk | 99.53 | 10.16 | 54.07 | 2.97 |
| reuse load | 5.40 | 5.62 | 1.08 | 1.03 |
| reuse decode | 42.23 | 27.48 | 6.48 | 5.01 |
| reuse check | 0.58 | 0.65 | 0.75 | 0.69 |
| reuse pin | 1.69 | 1.68 | 1.28 | 1.30 |
| persist/reuse | 0.65 | 1.29 | 0.50 | 0.50 |
| query | 13.47 | 14.84 | 3.88 | 3.95 |
| Process wall remainder | 17.23 | 9.38 | 4.55 | 3.79 |
| Profile process wall | 281.13 | 94.04 | 87.18 | 31.45 |

`host open` includes rule loading, the fused filesystem inventory (after), policy construction and session/cache setup. `walk` after the change is primarily inventory replay and effective selection; the before column performs a second filesystem walk. The remainder includes process startup, teardown, lease release and work outside the recorded stages; it is not attributed exclusively to rule loading. Read, hash, parse/lex, span conversion, graph assembly and canonicalize/seal are zero in these warm traces.

The checkout still exceeds the budget. Its remaining eager retained-graph work is 27.48 ms decoding all 87,679 nodes and 112,443 edges, plus 14.84 ms for the whole-graph find. Together with 5.62 ms loading/verifying the retained object, those phases alone consume 47.94 ms. Host/inventory adds 22.95 ms, selection/replay adds 10.16 ms, and pinning and process cleanup add further time. Shared strings reduce decode allocation, but the decoder still eagerly materializes every node/edge and find still scans every node. These phases together block 50 ms on the checkout in this implementation; no claim that 50 ms is fundamentally impossible is made. The 4k graph is smaller and passes.

Initial diagnostic runs overlapped builds or source edits; one correctly refused sampling when an edit caused rebuilding. They are not the final timing comparison. The final run preserves all reuse assertions and runs after validation is quiet.

Raw final samples in milliseconds:

```text
before repo: 290.406, 294.405, 289.799, 280.765, 287.354, 279.213, 289.987, 279.960, 293.102, 289.006, 280.544, 289.085, 279.835, 287.836, 290.243, 289.819, 282.798, 286.324, 280.548, 287.446
before synthetic: 90.114, 87.893, 86.834, 87.484, 87.390, 87.632, 87.057, 88.174, 91.981, 90.749, 86.897, 89.758, 86.237, 87.201, 86.050, 87.185, 87.769, 96.984, 86.710, 87.543
after repo: 93.872, 91.616, 95.892, 94.128, 96.488, 94.482, 95.118, 94.736, 94.779, 98.642, 96.955, 97.223, 91.864, 90.243, 87.820, 94.380, 91.211, 90.790, 93.843, 96.245
after synthetic: 33.331, 33.190, 31.583, 33.668, 33.717, 31.727, 31.868, 31.107, 33.150, 31.088, 33.198, 31.106, 32.050, 31.382, 31.468, 31.204, 31.679, 30.992, 31.385, 31.121
```
