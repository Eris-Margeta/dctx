# Linux map find latency

The Windows tester reported a 2.7 s p50 per process on a 1,551-file repository.
This run uses a generated Linux fixture. It measures the same operation with a
fresh process and a retained index. The numbers describe the Linux fixture.

## Reproduce

Build the baseline and candidate with `cargo build --release --locked -p dctx-cli`
and keep both binaries. Run the comparison with both paths.

```sh
scripts/map-latency.sh BASELINE_BINARY CANDIDATE_BINARY
```

The script generates 1,500 mixed Rust, Python, Markdown and JSON sources in 102
directories with a maximum directory depth of four. It sets file modification
times outside the stamp freshness window and uses a separate cache for each
binary. One empty-index call and one retained-index call per binary capture
the existing map timing spans. Then 20 fresh processes per binary query the
retained index in alternating order. Reported p50 and p95 are nearest-rank wall
times and include process startup and exit. Auto-clean is disabled for these
samples. All 44 calls must write the same bytes to stdout.

The baseline was built from `b16c5f4f`. Both binaries used the same release
configuration on x86-64 Linux.

## Retained-index benchmark

| Build | p50 | p95 |
| --- | ---: | ---: |
| Before | 17.355 ms | 20.776 ms |
| After | 15.809 ms | 24.450 ms |

The p50 improved by 8.9% in this 20-run comparison and fell short of the 25%
target. The p95 increased. An earlier comparison with the same binaries while
workspace tests were running measured a p50 of 18.337 ms before and 17.607 ms
after. All 44 stdout captures were byte-identical in each comparison.

## Profile

With an empty cache, the baseline spent most of its measured scan time in
parse/lex and source reads. With a retained index, the existing stat stamps
avoided all 1,500 content reads and parse/lex dropped to zero. The remaining
walk included live file stats. Index load and decode, root setup and cache
ledger transactions took additional time. The host-open span includes the
rule-discovery walk, which supplies metadata to the engine walk, so these spans
and the CLI phase totals should be interpreted independently.

| Span | Empty index before | Empty index after | Retained index before | Retained index after |
| --- | ---: | ---: | ---: | ---: |
| Host open | 11.78 ms | 5.86 ms | 7.66 ms | 6.48 ms |
| Index load | 0.09 ms | 0.06 ms | 0.62 ms | 0.62 ms |
| Index decode | 0 ms | 0 ms | 1.72 ms | 1.72 ms |
| Engine walk | 17.87 ms | 17.49 ms | 1.35 ms | 1.36 ms |
| Source read | 53.23 ms | 47.08 ms | 0 ms | 0 ms |
| Parse/lex | 97.11 ms | 94.61 ms | 0 ms | 0 ms |

The cache already durably updates the ledger during snapshot acquisition.
Cached map sessions now record root use with that transaction, saving an
independent startup ledger write. Unused sessions that never reach a cache
transaction leave the root-use time alone. On Linux, source reads also use one
open-handle stat for both file type and size. The pre-open type check and
bounded read still apply.

A separate Linux `strace -c` run counted the relevant calls. Counts include
cache and runtime activity in addition to the source walk.

| Call | Empty index before | Empty index after | Retained index before | Retained index after |
| --- | ---: | ---: | ---: | ---: |
| `newfstatat` | 3,101 | 3,101 | 1,601 | 1,601 |
| `statx` | 3,144 | 1,627 | 172 | 154 |
| `fsync` | 10 | 8 | 6 | 4 |
| `rename` | 4 | 3 | 3 | 2 |

The Rust check used
`cargo test --workspace --locked --no-fail-fast --exclude dctx-desktop`. The
desktop app requires a separately built frontend bundle. The workspace run
completed 3,259 tests successfully. Two overlapping runs collided in the
shared plugin-platform fixture directory and aborted that target. All 11 cases
passed in isolation with both the original workspace test binary and
`cargo test --locked -p dctx-plugin-platform --test n1113_hostile`.

## Files changed

- `crates/dctx-cli/src/host.rs`
- `crates/dctx-cli/src/host/roots.rs`
- `crates/dctx/src/adapters/fs/unix.rs`
- `crates/dctx/src/session/build.rs`
- `crates/dctx/src/session/ledger.rs`
- `crates/dctx/src/session/ledger/cache_bind.rs`
- `crates/dctx/src/session/ledger/cache_bind/tests.rs`
- `scripts/map-latency.sh`
- `docs/benchmark/map-latency.md`
