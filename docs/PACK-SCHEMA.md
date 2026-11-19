<!-- SPDX-License-Identifier: Apache-2.0 -->
# Pack JSON contract and offline builds

`dctx pack --for 'fix alpha_body' --budget 3000 --json` emits exactly one
LF-terminated JSON document. Its contract is **`dctx.pack/1`**, initially
`schema_version: "1.0"`. This contract has its own compatibility rules even while
other dctx interfaces remain experimental. It applies to task packs; ordinary
`pack --json` remains a summary and `context` has a separate interface.

Run the CLI in a project with an `alpha_body` declaration:

```sh runnable
dctx pack --schema > pack.schema.json
dctx pack --for 'fix alpha_body' --json > pack.json
```

From the dctx source checkout, validate that output, replacing `PROJECT` with
the path to your project:

```sh
python3 scripts/validate_pack_schema.py < PROJECT/pack.json
```

The schema is shipped at [schemas/dctx.pack-1.schema.json](../schemas/dctx.pack-1.schema.json).
The CLI embeds an identical copy so schema printing also works in a standalone
binary without a project or cache. Tests enforce equality between both copies.
The schema's HTTPS `$id` is an identifier; printing and validation need no HTTP
request. All schema references are local. The validator requires Python 3 and
`jsonschema >= 4.18` (tested with 4.19.2); provision that test dependency before
running offline. It is not a runtime or release-build dependency.

## Fields and exact spans

| Field | Meaning |
| --- | --- |
| `schema`, `schema_version` | Major contract ID and major.minor revision. |
| `v`, `verb` | Legacy envelope markers, `2` and `context`. They do not version this contract. |
| `text` | Complete human-readable selection page, including its end line. |
| `hits`, `terms` | Matches before budget selection, and number of extracted search terms. |
| `budget` | Requested selection budget in estimated tokens. |
| `snapshot` | Pinned source generation identity, `v1:sha256:<hex>:<length>`. |
| `commit` | Verified source commit, or `null` when unavailable. |
| `fragments` | Exact source prefixes included in read sections, in selection order. |

Every fragment has `target`, `path`, `byte_start`, `byte_end`, `line_start`,
`line_end`, `sha256`, and `content`:

- Paths use `/`, relative to the selected source. With `--also`, paths carry
  directory labels; resolve each label against the corresponding supplied root.
- Byte offsets count original file bytes: zero-based, **start inclusive, end
  exclusive**. After JSON decoding, UTF-8 encoding `content` reproduces exactly
  `file_bytes[byte_start:byte_end]`. It is always nonempty.
- Lines are one-based and inclusive. LF advances the line counter; CR remains
  part of a CRLF span. `line_end` is the line containing the last included byte,
  so a terminating LF does not add an extra empty line.
- `sha256` is the 64-digit lowercase SHA-256 of exactly those span bytes, without
  a domain prefix, newline insertion, encoding conversion or normalization.
- Each fragment inherits the envelope's `snapshot` and `commit`. The snapshot
  covers the engine's canonical generation (paths, decisions, file metadata,
  content digests and graph), not just the fragment. Touching a file can change
  that identity even when its fragment hash stays the same.

The engine re-reads and verifies the file against the pinned content digest
before producing fragment content. A changed source refuses the transaction.
The current filesystem source supplies a snapshot identity but no verified VCS
identity, so `commit` is `null`, including inside a Git checkout. A binary's build
commit is never presented as a source commit; arbitrary working files need not
match it.

Find and neighbor records in `text` are generated index metadata, not literal
source fragments. Read section titles and record lines are also metadata. A
source line that lacks a final LF may receive one in `text` for display; its
fragment retains the original bytes. JSON read sections contain raw source
prefixes without synthetic read-truncation markers. The end line's `dropped`
count describes omitted display lines. The selection keeps whole source lines
except where a node itself begins or ends inside a line.

As with the earlier JSON text envelope, `--budget` bounds the selected **text
page**, using `ceil(2 * UTF-8 bytes / 7)`. JSON escaping, fragment copies and
provenance metadata add bytes outside that selection budget. Runtimes should
use `fragments` for source verification and account for the whole JSON document
when imposing a transport-size limit. Exit 1 still emits a valid document with
no matches; exit 2/3 failures do not promise a pack document.

## Compatibility and determinism

Additive fields or optional metadata increment `schema_version`'s minor number
while retaining `schema: "dctx.pack/1"`. Consumers must accept unknown fields and
must not require the minor number to equal `1.0`. Removing or renaming fields,
changing types, meanings, span/hash conventions, or required-field semantics is
a breaking change and requires a new major ID such as `dctx.pack/2`, a new
schema, and an explicit migration. Historical major contracts must remain
available to pinned consumers. Do not silently assign major-2 data a major-1 ID.
Schema revisions are independent of the CLI package version.

For the same binary, roots, source bytes and metadata, ignore/policy settings,
request and budget, output is byte-identical. Output includes no current clock,
cache age, absolute source path or randomly assigned generation number. A warm
cache and cold cache use the same serialization. Different binary/parser
versions can select different records; pin and hash-verify the binary as well
as checking the schema ID. Text formatting and record selection are not a
cross-version golden promise.

`cargo test --offline --locked -p dctx-cli --test pack_schema` validates success
and empty goldens using a Draft 2020-12 validator, verifies actual source bytes
and line bounds, checks changed-source identities and sensitive-file omission,
and compares repeat output bytes. It includes Unicode, CRLF, missing final LF,
nonzero offsets, budget cuts, negative schema cases and additive-field acceptance.

## Offline release recipe

The supported proof recipe targets **Linux x86_64 GNU**, Rust **1.100.0**, with
the default CLI features. Toolchain provisioning and dependency-cache hydration
are separate preparation steps, performed before disconnecting. Provision the
Rust 1.100.0 toolchain and target, Python 3, bubblewrap, a native C compiler and
GNU linker, plus a Cargo registry cache containing the exact locked dependency
versions (including the sparse index entries). A prepared vendor directory can
also be configured through Cargo source replacement. No vendored dependency
copies are added to this repository.

On a preparation machine, `cargo +1.100.0 fetch --locked --target x86_64-unknown-linux-gnu` can hydrate the cache. Preserve `Cargo.lock` unchanged
and transfer the prepared toolchain/cache to the offline host when needed.
Cargo verifies registry package checksums against the lockfile. Missing cached
packages make the offline build fail; the recipe never retries online.

Use an already configured external build/cache directory; do not place caches
in the checkout:

```sh
umask 022
export SOURCE_DATE_EPOCH=1794873600
python3 scripts/reproducible_build.py --output "$CARGO_TARGET_DIR/pack-repro-proof"
```

The script:

1. Starts a bubblewrap network namespace with only loopback, then runs both
   builds and their dependency build scripts inside it. `--frozen --offline`
   additionally forbids Cargo network access and lockfile changes. Rustup uses
   the already installed pinned toolchain.
2. Builds only `dctx-cli` and its dependencies twice with `--release --target
   x86_64-unknown-linux-gnu`, in two fresh temporary target directories under
   the existing `CARGO_TARGET_DIR`. It uses two compile jobs, disables
   incremental compilation, strips symbols, suppresses ELF build IDs, and
   remaps source/cache/target paths to fixed virtual paths. It neither runs
   `cargo clean` nor rebuilds the desktop application.
3. Pins `SOURCE_DATE_EPOCH` and the embedded build commit. The compiler/linker
   versions are recorded; this is a same-host proof, not a promise of equal
   binaries across different native toolchains or libc versions.
4. Hashes both complete executable files and requires equal SHA-256 values.
   It moves the first executable to the result directory as `dctx-cli` and
   removes regenerable compiler intermediates. If hashes differ, it also
   preserves `dctx-cli-second` for byte inspection and exits unsuccessfully.
5. Emits `manifest.json` (`dctx.build/1`) beside the executable, containing
   compiler version/commit/LLVM, target, profile/features, epoch, native tool
   versions, lockfile SHA-256, source commit, source-tree SHA-256, flags, network
   isolation, two artifact hashes, size and the reproducibility result. Host
   paths, personal identifiers and environment secrets are omitted.

`dctx-build-tree/1` hashes actual working-tree build inputs, including dirty and
untracked Git-visible files. It covers root `Cargo.toml`, `Cargo.lock`, `VERSION`,
`crates/`, `plugins/`, `schemas/`, `.cargo/`, `examples/library/`, the desktop
manifest needed for workspace resolution, and the build-recipe script. It
excludes docs, unrelated applications and generated build/cache outputs, so
recording this proof cannot change the source hash. Paths are sorted by their
raw bytes; SHA-256 starts with `dctx-build-tree/1\0`, then for each file hashes
big-endian u64 path length, relative path bytes, big-endian u32 permission bits
(mode & 0777), big-endian u64 content length, and content bytes. Missing files
are absent; symlink build inputs are refused. Inputs and lockfile are hashed
again after the builds to catch changes during the proof.

## Recorded proof

On 2026-11-17, two fresh default-feature release builds completed successfully
inside the isolated network namespace, with no external network access. Both
complete **9,574,096-byte** executables had SHA-256:

```text
e708477585ebebe32b796ef130909923a5c0b1bfdbed0cc5e965add65d989012
```

**Reproducible: yes.** There are no differing artifact bytes to explain. The
[recorded machine-readable manifest](PACK-BUILD-MANIFEST.json) includes both
hashes, Rust 1.100.0 / LLVM 23.1.5, target `x86_64-unknown-linux-gnu`, GCC 14.2.0,
GNU ld 2.44, glibc 2.41, epoch `1794873600`, and these input hashes:

| Input | SHA-256 |
| --- | --- |
| Cargo.lock | `946f51e83ec610eeaf18d455a83f2451d42154bc23100f2f7467f7ac6a6d25af` |
| Source tree (1,854 files) | `a5579d728d19465e65a1ac45c6a2d1bbb535c49f496eb7a6aead643df1cbf845` |

The release executable's `pack --schema` output also matched the repository
schema byte-for-byte. The release executable reproduced both checked-in
goldens, with byte-identical repeated output for each. The success and empty
golden documents validated with network access isolated as well.

This proof used the uncommitted working tree. The source-tree hash identifies
its actual inputs; the manifest's source commit is the checkout base embedded
in `--version`. Committing the changes alters that embedded commit, so the
release process must generate a fresh manifest for its final committed source.


Validation on the same working tree:

```sh
CARGO_BUILD_JOBS=2 cargo test --offline --locked --workspace --exclude dctx-desktop -- --test-threads=2
CARGO_BUILD_JOBS=2 cargo clippy --offline --locked -p dctx -p dctx-cli --all-targets -- -D warnings
```

The workspace gate passed **3,269 tests, zero failures, zero ignored**, including
all six new pack-contract tests. It includes the facade, CLI, engine crates,
plugins, desktop API and doctests; the Tauri desktop application is excluded as
in the repository's release gate. Clippy passed with warnings denied. A separate
mapped-root check verified both fragments against the two actual root files,
validated the schema and compared repeated output bytes. No version bump,
commit, tag, publication or website deployment was performed.
