<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->

# Metadata-only catalog experiment

Standalone Linux/Unix Cargo project for DCX-CATALOG-01; it has its own workspace
and lockfile. It does not change the dctx production cache or workspace.
Only `ignore = 0.4.24` and `memmap2 = 0.9.9` are direct dependencies, both exactly
pinned. The lockfile also pins their transitive dependencies.

## Run

Run from the repository root. Redirect inherited Cargo settings and temporary
files into this worktree before fetching/building:

```sh
umask 022
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTC_WRAPPER=
export CARGO_HOME="$PWD/tools/catalog-bench/.cargo-home"
export CARGO_TARGET_DIR="$PWD/tools/catalog-bench/target"
mkdir -p tools/catalog-bench/scratch/tmp
export TMPDIR="$PWD/tools/catalog-bench/scratch/tmp"
# First fetch needs network if this local Cargo cache is empty.
CARGO_NET_OFFLINE=false cargo fetch --locked --manifest-path tools/catalog-bench/Cargo.toml
cargo build --release --locked --manifest-path tools/catalog-bench/Cargo.toml
python3 tools/catalog-bench/run.py
```

The runner defaults to this repository checkout (pass `--roots` to measure other trees),
then a one-million-record synthetic metadata case. It deletes each catalog after
measurement, including on ordinary exceptions. Its report defaults to `docs/results/DCX-CATALOG-01.json` (not committed).
Use `--report` to choose another path inside the worktree and `--threads` to
control parallelism. Positional roots replace the default roots.
`--synthetic-only` runs only the generated one-million-record case;
`--skip-synthetic` runs only the filesystem roots. The runner
limits address space to 8 GiB to bound unexpected input growth; this is separate
from the 512 MiB RSS acceptance target. Only aggregate measurements are saved.

Manual commands (the output parent must already exist):

```sh
tools/catalog-bench/target/release/catalog-bench build . tools/catalog-bench/scratch/local.cat 36
tools/catalog-bench/target/release/catalog-bench inspect tools/catalog-bench/scratch/local.cat
tools/catalog-bench/target/release/catalog-bench query tools/catalog-bench/scratch/local.cat parser
rm tools/catalog-bench/scratch/local.cat
```

`query` writes **NUL-delimited raw relative path bytes**, including non-UTF-8
names. Tokens are maximal runs of ASCII alphanumerics or non-ASCII bytes in a
basename. ASCII matching ignores case; punctuation separates tokens. For example,
`test` matches `test_parser.rs` and `Parser-test.rs`; `parse` does not match
`parser.rs`. There is no Unicode normalization/case folding or CamelCase split.
Every match is returned; there is no result cap.

## Traversal and shards

The `ignore` work-stealing parallel walker collects regular-file `lstat` metadata
into per-thread vectors and merges them after traversal. It includes hidden
files, honors root and nested `.gitignore` files even outside a Git repo, and
prunes `.git`, `node_modules`, and `target` at every depth. Ancestor/global Git
ignores, `.ignore`, and Git `info/exclude` are disabled for reproducibility and
to avoid following Git worktree administrative paths outside the measured root.
The benchmark's output directory is also pruned to avoid measuring its own files.
The project-local Cargo/build caches are excluded by this project's `.gitignore`.

Symlink files and directories are excluded. The root itself must be a real
directory. A pre-descent check refuses directories with symlinked `.gitignore`
or `.jj` entries because `ignore` otherwise follows these implicitly. Such
refusals and per-entry I/O/ignore-parse errors mark the snapshot incomplete and
make the command exit unsuccessfully; the partial metrics remain available.
The traversal assumes a quiescent tree, rather than providing protection against
concurrent adversarial directory replacement. Mount points are allowed.

A directory containing a regular `.git` marker file or `.git` directory starts
a Git shard. Marker contents are never read; worktrees count as separate roots,
and bare repositories without `.git` markers are not detected. Nested Git roots
start new shards. Outside Git roots, each top-level directory is a fallback
shard; root-level files use a root fallback shard. Git and top-level fallback
shards count even when empty; an unused root fallback does not count. Directory
parent pointers and marker flags determine shard membership after reopen.
There is no symbol index, source-content read, or symbol parsing.

Tier flags are **extension routing hints**, not content classification:
code, Markdown, known binary/media/archive, or other. Unknown binaries remain
`other`; no bytes are read to identify them. `(device, inode)` identifies a Unix
file, and each hard-linked path remains its own catalog entry.

## Format v1

All paths are root-relative raw Unix bytes. All integers are little endian or
canonical unsigned LEB128. Source contents, absolute input roots, and tokens
are not persisted.

| Header byte offset | Field (`u64`, except magic) |
| --- | --- |
| 0 | Eight-byte magic `DCTXCAT1` |
| 8 | File count |
| 16 | Directory count, including the empty root at ID 0 |
| 24 | Git-marker root count |
| 32 | Start of file-offset table |
| 40 | Start of file records |
| 48 | Exact total catalog length |
| 56 | Reserved, must be zero |

The directory section begins at byte 64. Each directory stores parent ID
(varint), git-marker flag (one byte, 0 or 1), basename length (varint), and basename
bytes. Parents precede children. This is parent-relative front coding: a full
ancestor prefix is represented by its parent ID and is stored only once.
There is no sibling-name prefix compression.

The file-offset table contains `file_count + 1` relative `u32` offsets into the
record section. Each record stores parent directory ID, basename length and
bytes, size, zigzag-encoded signed mtime seconds, mtime nanoseconds, inode, device
(all integers varints), then one tier byte (`code=0`, `markdown=1`, `other=2`,
`binary=3`). Nanoseconds must be less than one billion. The record section is
limited to 4 GiB. Directory overhead, header, and all offsets are included in
reported bytes/file. The writer refuses overwrite and calls `sync_all` before
reporting build completion.

Reopen maps read-only and validates the complete directory/index/record layout,
bounds, varints, names, timestamps, and flags. It retains a small decoded
directory table, not a decoded file list. The caller must keep the backing file
immutable while mapped. There is no checksum, crash recovery, incremental
update, or compatibility promise for this experimental format. Failed writes
can leave partial outputs; the runner removes them.

Name queries scan the mmap record-offset table and basenames, then reconstruct
all matching paths through directory parents. Reopen/validation is measured
separately from query latency. This baseline intentionally measures the cost of
finding names without another persistent index.

## Measurement and verification

Each filesystem root receives two separate processes: a first-observed walk with
unknown cache state, then a repeated warm walk following the first build/query
run. The runner does not drop system caches or open source contents to warm them.
An unprivileged first walk must not be described as a verified cold walk.
The runner separately evicts **its own completed output** with
`posix_fadvise(DONTNEED)`, verifies zero resident pages with `mincore`, then times
a fresh-process mmap reopen/validation. This is a cold catalog read, not a cold
source-tree walk or construction measurement. If pages remain resident, the
report marks the reopen unverified. No input-tree cache eviction is performed.
`walk_seconds` includes traversal, stat calls, and merging thread vectors;
`build_seconds` additionally includes sorting, encoding, `sync_all`, and dropping
builder data, but excludes reopening and queries.

Every nonempty catalog gets 1,000 deterministic xorshift64 queries (seed
`0xdc7ca7a101`): choose a file uniformly, then one nonempty basename token uniformly.
Sampling is outside the timer. Common extensions are included. Each timed query
includes finding every match, reading its metadata, reconstructing its full path,
and consuming the path length; stdout formatting is excluded. Percentiles use
nearest ranks 500 and 990. An empty catalog reports no query samples. A tree with
only punctuation-only basenames receives 1,000 miss queries.

RSS is Linux `/proc/self/status` `VmHWM` in KiB, sampled after walking,
building, and queries; use the maximum of these readings. This covers the whole
build/reopen/query process, including the mapped catalog. It excludes the
kernel filesystem caches. The runner also records `wait4` resource usage; its
peak can include Python memory briefly inherited before `exec` on small inputs. Synthetic measurements generate metadata in memory, not one million
filesystem files; they cannot qualify cold filesystem construction or traversal.

```sh
cargo fmt --manifest-path tools/catalog-bench/Cargo.toml -- --check
cargo test --locked --manifest-path tools/catalog-bench/Cargo.toml
cargo test --release --locked --manifest-path tools/catalog-bench/Cargo.toml
cargo clippy --locked --manifest-path tools/catalog-bench/Cargo.toml --all-targets -- -D warnings -W clippy::pedantic
RUSTDOCFLAGS='-D warnings' cargo doc --locked --manifest-path tools/catalog-bench/Cargo.toml --no-deps
```

Tests cover metadata round trips, parent-relative front coding, Unicode and
non-UTF-8 bytes, query semantics, shards, empty catalogs, malformed/truncated
formats, ignore rules, all four routing tiers, and symlink boundaries.
Measured outcomes are summarised in the project's benchmark notes when published.
