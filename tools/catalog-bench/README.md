<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->

# Indexed metadata catalog experiment

Standalone Linux/Unix Cargo project for DCX-CATALOG-02, with its own workspace
and lockfile. Production code, manifests, and caches are outside this experiment.
Direct dependencies remain exactly pinned `ignore = 0.4.24` and `memmap2 = 0.9.9`.

## Run

From the worktree root, keep every writable cache and temporary path local:

```sh
umask 022
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTC_WRAPPER=
export CARGO_HOME="$PWD/tools/catalog-bench/.cargo-home"
export CARGO_TARGET_DIR="$PWD/tools/catalog-bench/target"
mkdir -p tools/catalog-bench/scratch/tmp
export TMPDIR="$PWD/tools/catalog-bench/scratch/tmp"
CARGO_NET_OFFLINE=false cargo fetch --locked --manifest-path tools/catalog-bench/Cargo.toml
cargo build --release --locked --manifest-path tools/catalog-bench/Cargo.toml
python3 tools/catalog-bench/run.py --fresh-tree-files 1000000
```

The runner measures this worktree, read-only `/srv/work`, accessible `/srv/data`,
and 1M synthetic metadata records. The optional fresh-tree case creates 1M empty
files under a locally ignored `.research-tmp/` directory, measures the first
traversal and repeat traversal, then deletes the tree. It refuses more than 1M
files or less than 2 GiB free space. An intentionally conservative allocation
estimate includes 4,096 bytes per empty-file inode and must stay below 5 GB.
Creation and directory fsync warm metadata: **first-touch is not verified cold**.
No source content is read, and no input-tree cache eviction is performed.

`--report` defaults to `docs/results/DCX-CATALOG-02.json`; `--threads` controls
walk parallelism. Positional roots replace defaults. `--synthetic-only` and
`--skip-synthetic` select cases; `--fresh-tree-files` independently adds the disk
case; `--fresh-only --fresh-tree-files 1000000` runs only that case. All catalogs are deleted, including on ordinary exceptions. Partial
snapshots report their metrics and exit unsuccessfully; in particular inaccessible
entries under `/srv/data` are not silently counted as coverage. The runner uses
an 8 GiB address-space ceiling, distinct from the 512 MiB RSS target.

```sh
tools/catalog-bench/target/release/catalog-bench build . tools/catalog-bench/scratch/local.cat 36
tools/catalog-bench/target/release/catalog-bench inspect tools/catalog-bench/scratch/local.cat
tools/catalog-bench/target/release/catalog-bench query tools/catalog-bench/scratch/local.cat parser exact 100
tools/catalog-bench/target/release/catalog-bench query tools/catalog-bench/scratch/local.cat pars prefix 100
tools/catalog-bench/target/release/catalog-bench query tools/catalog-bench/scratch/local.cat arse substring 100
rm tools/catalog-bench/scratch/local.cat
```

`query` emits NUL-delimited raw relative path bytes. Mode defaults to `exact`;
omitting the limit returns every match. Tokens are maximal ASCII-alphanumeric or
non-ASCII-byte runs in a basename; punctuation separates them. Matching folds
ASCII case only, without Unicode normalization or CamelCase splitting. Prefix
and substring operate **within a token**, so `test-parser` is an invalid query.
Empty queries return nothing. Output IDs are distinct and ordered by token key,
then file ID; limited output is a deterministic first page for that catalog.
There is no continuation cursor or total-count guarantee for a limited query.
The library `find` preserves exact-token, unlimited, sorted-ID behavior.

## Traversal and construction

The ignore-aware parallel walker collects metadata **and lowercase token
occurrences in the same thread-local visitor**. Merging adjusts token file IDs;
no second filesystem walk occurs. `write_snapshot` consumes this collected state,
preserves merged traversal file order, sorts directories, and encodes the metadata
and index. File order can differ between builds. The generic `write_catalog`
API sorts supplied files and prepares their tokens in a parallel in-memory pass;
the synthetic case uses that API. Index sorting/compression occurs after merge.

Hidden files and root/nested `.gitignore` files are included/respected.
`.git`, `node_modules`, `target`, and the output directory are pruned.
Ancestor/global Git ignores, `.ignore`, and Git info/exclude are disabled.
Symlink files/directories are excluded; symlink roots and implicit symlinked
`.gitignore`/`.jj` controls are refused. This path-based walker assumes a
quiescent tree and does not defend against adversarial directory replacement.
Mount boundaries are allowed; ordinary concurrent changes can yield partial data.
Only ignore controls and filesystem metadata are read, never source contents.

Regular `.git` marker files/directories start shards without reading Git
administrative data. Nested Git roots override their parents. Top-level directories
outside Git and root files use fallback shards; bare repos without markers are
not detected. All hard-linked paths remain separate entries. Extension hints route
files to code, Markdown, binary/media/archive, or other; they are not content
classification. Unix `(device, inode)` and signed seconds/nanoseconds are retained.

## Format v2

All paths are raw Unix bytes; integers are little endian or canonical unsigned
LEB128. The read-only mmap contains every persisted index byte. There is no
checksum, incremental update, recovery protocol, or backward compatibility with v1.
The backing file must remain immutable while mapped. Output creation refuses
replacement, and build timing includes `sync_all` and dropping builder state.
Failed writes can leave partial outputs, which the runner removes.

| Header offset | Field (`u64`, except magic) |
| --- | --- |
| 0 | Eight-byte magic `DCTXCAT2` |
| 8 | File count |
| 16 | Directory count, including empty root ID 0 |
| 24 | Git marker count |
| 32 | Start of file-offset table |
| 40 | Start of file records |
| 48 | Exact total length |
| 56 | Start of name index |

Directories start at byte 64: parent ID (varint), Git flag (byte), basename length
(varint), raw basename. Parents precede children. Each ancestor is stored once.
The file-offset table holds `file_count + 1` relative `u32` offsets. Records hold
parent ID, basename length/bytes, size, zigzag signed mtime seconds, nanoseconds,
inode, device (all integers varints), then tier byte (0 through 3).
The record section and name index each have independent 4 GiB limits.

The name index starts with six `u32` fields: token count, gram count, dictionary
start, gram-directory start, gram-data start, exact index length. All index offsets
are relative to its own beginning, except dictionary record offsets, which are
relative to the dictionary start, and gram posting offsets, relative to gram data.
A `token_count + 1` `u32` offset table follows the 24-byte header. Each dictionary
record contains token length (varint), lowercase token bytes, posting count
(varint), then sorted unique **file-ID deltas** (varints, first relative to zero).
Tokens are strictly byte-sorted and repeated tokens within one basename are
stored once per file in the postings.

Each sorted gram-directory record has three `u32` fields: key, posting start,
posting end. Posting payloads are unique sorted **token-ID deltas**, without a
count. Three-byte keys use `b0<<16 | b1<<8 | b2`. Tokens of length one/two get
singleton keys `0x01000000 | b0` / `0x02000000 | b0<<8 | b1`; longer tokens only
emit trigrams. Repeated grams within a token are deduplicated.

Exact and prefix searches binary-search the dictionary and lazily decode file
postings. Substrings of at least three bytes choose the smallest compressed
trigram posting list and verify candidate token bytes. Short substrings scan the
small gram directory and heap-merge matching posting lists plus singleton keys,
without scanning every dictionary token. File IDs are deduplicated during result
production. Limited searches stop after the requested number of distinct IDs;
common-token unlimited queries remain proportional to output size. Adversarial
low-selectivity multi-gram queries can still require many candidate verifications.

Reopen validates all section lengths, endpoints, canonical varints, token and
posting order, lowercase/token bytes, IDs, gram membership, directory relationships,
metadata, and flags before querying. Structural validation does not authenticate
index completeness against metadata: this is an experimental non-checksummed
format. It retains a decoded directory table, with file/token/posting data in mmap.
`catalog_bytes` includes everything; `name_index_bytes` is separately reported.

## Measurements and gates

Every nonempty case samples 1,000 random files and one basename token per file
with xorshift64 seed `0xdc7ca7a101`, including common extensions. Sampling is
outside query timers. Exact probes use whole tokens; prefix probes keep the first
ceil-half; substring probes keep the middle half (floor-quarter to ceil-three-
quarters), without a minimum length. Each mode runs 1,000 queries with at most 100
distinct results, including metadata/path reconstruction and consumed path length.
A separate 1,000-query unlimited exact workload preserves the original benchmark;
its lookup-only sub-timer measures all matching IDs and its outer timer includes
all paths/metadata. No stdout formatting is timed. Nearest-rank p50/p99 use samples
500/990. Empty catalogs run no probes; punctuation-only trees run miss probes.

`walk_seconds` includes enumeration, ignores, stat, token collection and merging.
`build_seconds` also includes sorting, encoding, sync, dropping builders, but
excludes reopen and queries. Synthetic build includes in-memory record generation.
Warm means a new process following a completed first build/query run; real trees
are live and counts may change. RSS is Linux VmHWM for the entire process,
including mmap and index construction, excluding kernel caches. `wait4` counters
and source/binary hashes are retained in the JSON.

After real/metadata-synthetic cases, the runner evicts only its **own catalog**
with `posix_fadvise(DONTNEED)`, checks zero residency with `mincore`, then reopens
and fully validates in a new process. This measures cold catalog pages, not cold
source metadata or device/controller caches. Zero-residency failures are explicit.

```sh
cargo metadata --locked --manifest-path tools/catalog-bench/Cargo.toml --no-deps
cargo fmt --manifest-path tools/catalog-bench/Cargo.toml -- --check
cargo test --locked --manifest-path tools/catalog-bench/Cargo.toml
cargo test --release --locked --manifest-path tools/catalog-bench/Cargo.toml
cargo clippy --locked --manifest-path tools/catalog-bench/Cargo.toml --all-targets -- -D warnings -W clippy::pedantic
RUSTDOCFLAGS='-D warnings' cargo doc --locked --manifest-path tools/catalog-bench/Cargo.toml --no-deps
python3 -m unittest discover -s tools/catalog-bench/tests -p 'test_*.py'
```

Tests compare index modes and limited output with a scan oracle; cover duplicate
tokens, short queries, mixed ASCII case, non-UTF-8, Unicode, empty/missing keys,
parallel visitor ID merging, truncated/mutated index sections, and all prior
metadata, routing, ignore, shard, and symlink cases.
Results and acceptance decisions: [DCX-CATALOG-02](../../docs/results/DCX-CATALOG-02.md).
