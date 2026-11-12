# One-shot map find cold start — 2026-11-12

## Measurement

Linux x86_64, Rust 1.99.0, default-feature release CLI. The reported Windows
2,700 ms p50 on a 1,551-file checkout is the motivating tester observation, not a
measurement reproduced here. This checkout has about 2,700 inventory file rows,
89,000 nodes and a 9.2 MiB retained index. The generated tree has exactly 1,500 Rust
files, 31,500 nodes (30,000 functions plus modules), and a 4.8 MiB index.

The supported command is `dctx map find <name> --place .`; the CLI rejects a second
positional argument. The repository query was `snapshot_reusable`; the generated
query was `latency_probe_0750_10`. Both queries return indexed definitions.

Each binary first warmed the index. Then 31 normal subprocess calls per binary
were interleaved in alternating old/new order, with stdout/stderr captured. Each
normal call was followed by a separate profiled call. Both versions read the same
index format and source tree. Diagnostic calls had to report generation reuse,
and diagnostic stdout had to equal normal stdout. Auto-clean was disabled, config
was isolated, and build/index/temp caches stayed under the worktree. Local test
compilation was paused during measurement. This measures a warm index and OS page
cache in a new process, rather than a warm process or a cold disk.

All values below are milliseconds. Wall latency includes spawn, dynamic loading,
argument handling, host/cache setup, index open/validation, query, output and exit.
CPU is child user plus system CPU measured separately from wall time. Percentiles
use nearest rank (p50 sample 16 and p95 sample 30 of 31).

| Metric | Repository before | Repository after | 1,500 files before | 1,500 files after |
| --- | ---: | ---: | ---: | ---: |
| Normal one-shot wall p50 | 304.35 | 307.93 | 135.35 | 133.76 |
| Normal one-shot wall p95 | 430.21 | 747.76 | 284.55 | 394.88 |
| Child CPU p50 | 286.27 | 276.65 | 113.74 | 106.02 |
| Profiled one-shot wall p50 | 314.65 | 305.43 | 144.73 | 132.14 |

| Profile stage (p50) | Repository before | Repository after | 1,500 files before | 1,500 files after |
| --- | ---: | ---: | ---: | ---: |
| Cache directory resolve | 0.019 | 0.019 | 0.018 | 0.020 |
| Host open (includes cache resolve, rule discovery and initial inventory stats) | 72.718 | 88.860 | 24.993 | 26.637 |
| Index object load | 16.669 | 17.901 | 11.424 | 10.502 |
| Index decode | 72.881 | 63.902 | 42.941 | 35.223 |
| Inventory replay / validation walk | 34.200 | 28.369 | 2.851 | 2.880 |
| Inventory equality / duplicate checks | 1.958 | 1.937 | 0.718 | 0.756 |
| Source reads | 0.000 | 0.000 | 0.000 | 0.000 |
| Source hashing | 0.000 | 0.000 | 0.000 | 0.000 |
| Persisted lease / generation pin | 7.469 | 8.265 | 8.322 | 8.897 |
| Remaining snapshot bookkeeping | 3.055 | 3.045 | 1.793 | 1.730 |
| Primary query and page encoding | 27.619 | 29.259 | 8.703 | 9.399 |
| Final stdout write + flush | 0.053 | 0.047 | 0.047 | 0.045 |

A separate 31-call `--version` sample gave **4.51 ms p50** as a
process-start/runtime/dispatch proxy. It includes printing one version line and
exit; it is not a measurement of the OS loader alone. Cache resolve is a subset
of host open. The other stages are separate samples; sums of medians need not
match the wall median. Additional wall time includes the CLI's indexed-name
follow-up query, session/lease destruction, argument handling and scheduling.

The dominant indexed stage was decode: it falls from 72.88 to 63.90 ms for the
repository and 42.94 to 35.22 ms for the synthetic tree. Child CPU falls by 3.4%
and 6.8%, respectively. End-to-end wall p50 is essentially flat (repository 304.35
to 307.93 ms; synthetic 135.35 to 133.76 ms); this run establishes a decode saving,
not a substantial overall latency improvement. The after p95 values show that
storage/scheduling tails remain. No Windows end-to-end speedup is claimed.

All profiled warm calls read and hash **zero source files**. The 1,500-file fixture
reports `stamp_skipped_reads 1500`. A separate `strace -c` synthetic warm call
recorded 1,515 `newfstatat`, 190 `statx`, 81 `openat`, 98 `read` and six `fsync`
calls: a full metadata inventory and durable ledger work still occur even when
source hashing is skipped. The syscall sample is instrumented and is not used as
a normal latency sample. Host setup and durable cache operations remain material
costs after the decoder change.

## What changed

A one-shot process cannot benefit from the existing thread-local decoded-generation
cache. The decoder previously materialized a `Vec<NodeText>` for every interned
string (including strings used only by paths and edges), cloned those owners into
five fields per node, then destroyed the temporary vector. Each owner increments
and later decrements the same atomic reference count.

The decoder now keeps one table owner and creates field owners on demand. Common
immutable graph vocabulary (`fn`, `sig`, `module`, etc.) and empty strings use
static storage, so they need no reference counting or individual allocation. Other
fields continue to own the validated shared buffer. `NodeText` remains two machine
words and remains `Send + Sync`. This also reduces destruction work when the CLI
process closes its session. The serialized index format and answer bytes stay the
same; existing indexes remain reusable. An old/new release comparison against the
same retained synthetic index produced byte-identical stdout (68 bytes).

All source inventory, policy, duplicate-path, resource-limit, digest, stamp and
lease checks remain in place. No time-to-live is used as evidence that files have
not changed. The complete inventory is still checked on every call. Settled
unchanged files avoid content reads under the existing stamp rule; changed stamps
and recent files still require digest verification, and `strict_live` still hashes
all files. This is important because a fresh index timestamp alone cannot detect
an immediate edit, addition, deletion or nested ignore-rule change.

## Regression coverage and reproduction

`crates/dctx-cli/tests/map_coldstart.rs` generates its own temporary sources and
uses a new CLI process for every query. The normal correctness test builds an
index, reuses it, makes a same-length edit, and checks that the next process finds
the new symbol and loses the old one. It also checks file addition and deletion.
A separate unit test checks decoded text ownership after its table is dropped,
and another checks every static spelling against owned and decoded strings.
Existing warm-reuse tests retain digest verification and same-stamp edit coverage.

The Linux release benchmark builds 1,500 files containing 20 documented functions
each, spread over 15 directories. It sets old mtimes directly, without sleeps,
builds the index once, then measures five complete subprocess calls. Every answer
must match the initial output and report generation reuse. The median must be
below 300 ms. Debug and non-Linux builds mark this latency test ignored; the
correctness test runs in both debug and release builds. The median keeps one
scheduler or storage outlier from deciding the performance test.

Run it with a worktree-local target, cache and temporary directory:

```bash
umask 022
export CARGO_TARGET_DIR="$PWD/target/coldstart-build"
export CARGO_HOME="$PWD/target/coldstart-cargo"
export TMPDIR="$PWD/target/coldstart-tmp"
mkdir -p "$CARGO_TARGET_DIR" "$CARGO_HOME" "$TMPDIR"
cargo test --locked --release -p dctx-cli --test map_coldstart
```

The phase measurements used the existing `--profile` output. Two temporary
`Instant` probes additionally timed `cache_root_dir()` from entry through path
resolution, and the final stdout `write_all` plus `flush`. The probes were identical
in both measured binaries and were removed from the delivered sources. Normal
latency samples disabled profiling and probe output; both measured binaries still
contained the same inactive timer/environment-check overhead. To reproduce a phase run,
wrap these two operations with `Instant::now()` / `elapsed().as_nanos()` and emit
the values only for the diagnostic run; `--profile` already reports host open,
walk, read/hash, reuse load/decode/check/pin, and query.

The initial unoptimized baseline also showed 494.1 ms repository p50 and 146.9 ms
synthetic p50 over 21 normal calls. Subsequent alternating runs exposed large
shared-storage stalls, which is why phase measurements, child CPU time and the
full distribution are distinguished from the wall-clock result. Native Windows
startup, antivirus and filesystem costs still need measurement on Windows; these
Linux numbers do not establish a replacement for the reported Windows 2,700 ms.

## Validation

All required CHECK commands exited 0:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --locked --exclude dctx-desktop --exclude dctx-desktop-api --all-targets -- -D warnings`
- `cargo test --workspace --locked --exclude dctx-desktop --exclude dctx-desktop-api`:
  2,537 passed, zero failed, one ignored (the debug latency test), including doctests.

The separate release run
`cargo test --locked --release -p dctx-cli --test map_coldstart` passed both tests,
including the 300 ms one-shot p50 guard.

The only widened implementation file is
`crates/dctx/src/session/reuse/reader.rs`, which consumes the new decode-time table.
