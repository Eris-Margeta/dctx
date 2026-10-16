# Segment and manifest format contract v1

DX-A0b · binary sketches 1.0 · not a claim of a shipped format.
Defines the immutable pure-Rust index chosen in decision 077 and DX-ARCH §3.
Depends on identity v1 and cache v1; used by A2/C1/C3/C4/C5.
The codec is pure, safe Rust; filesystem publication belongs to adapters.

## Encoding envelope

All integers are unsigned little-endian; offsets are from byte zero. No native
layout, pointers, alignment assumptions or unchecked casts. Each format owns
its own major/minor version. Readers refuse unknown majors and required
capabilities; an unknown optional section can be skipped only after structural
and checksum validation. Minor evolution cannot change a known section schema.
No compression in v1; a later compression capability must bound decoded sizes.

Segment header, exactly 80 bytes:

| Offset | Width | Field |
|---:|---:|---|
| 0 | 8 | Magic ASCII `DCTXSEG` followed by NUL |
| 8 | 2 | Segment major = 1 |
| 10 | 2 | Segment minor = 0 |
| 12 | 4 | Header length = 80 |
| 16 | 8 | Exact total file length |
| 24 | 4 | Section count, ≤64 |
| 28 | 4 | Directory entry width = 64 |
| 32 | 8 | Directory offset = 80 |
| 40 | 8 | Required feature bitset, 0 in v1 |
| 48 | 32 | Envelope SHA-256 over whole file with bytes 48..80 zeroed |

Directory entries follow immediately, each exactly 64 bytes:

```text
kind:u16, schema:u16, flags:u32, offset:u64, length:u64,
record_count:u64, payload_sha256:[u8;32]
```

Only flag bit 0 (`required`) is defined. Unknown flag bits are refused in v1.
Sections are unique by kind, sorted by kind, nonoverlapping, and occur after
the directory. Offsets/lengths must fit the file and checked arithmetic. V1
encoders pack payloads in directory order with no gaps/trailing bytes. Empty
sections have zero length/count and still carry SHA-256(empty). A manifest's
SegmentObject digest covers the final envelope bytes using identity v1's
structured kind 9, distinct from the internal envelope checksum. Checksums
detect corruption; they are not authentication against a hostile same-user writer.

## Section schemas and bounds

| Kind | Section | Schema 1 requirements |
|---:|---|---|
| 1 | String dictionary | Unique UTF-8 bytewise-sorted strings; u32 count, then u32 length + bytes per entry; limits checked before allocation |
| 2 | Sparse name prefix table | Sorted prefix entries into dictionary; prefix length/bytes + first/last dictionary index; exact lookup via sorted range, no FST |
| 3 | File-local fact blocks | Sorted FactKeys, each with length-delimited local declarations, spans, summaries, raw refs and provenance; no root/path-resolved targets |
| 4 | Postings | Field-tagged identifier/path/doc term rows, sorted term IDs, sorted document IDs and delta-varint positions; include document lengths |
| 5 | View nodes | ViewKey and manifest-local dense IDs mapping permitted path/public ID to FactKey/local fact ID; never used as portable public IDs |
| 6 | Forward adjacency | Sorted source dense ID + edge kind + target/reference + confidence/provenance |
| 7 | Reverse adjacency | Reciprocal index for applicable resolved edges and unresolved-name dependency keys |

Fact segments require sections 1 and 3, optionally 2/4 for local-name lookup;
view segments require 1/5/6/7, with 2/4 when serving ranked lookup. Manifest role
declares which section set is required. Path postings live only in view segments.
Mixed roles are forbidden in v1 so parse facts cannot accidentally carry roots.
Dense IDs are segment-local on disk; references include segment reference plus
local ID when crossing segments. The pinned manifest supplies their translation.

Every fact/node/edge record is `record_length:u32 || payload`, with bounded
field counts and schema-defined fields in fixed order. C1 must freeze detailed
fact/posting payload layouts and binary goldens before format promotion; these
sections specify the required data/validation, not permission to serialize
Rust structs. Fact schema evolves independently of the envelope. Unknown fact
schema refuses use/rebuilds only authorized map data.

Default decoder ceilings: segment ≤64 MiB, ≤64 sections, dictionary string
≤1 MiB, ≤1,000,000 records per section, record ≤1 MiB. All are additional to
caller operation memory/record caps; stricter cap wins. V1 indexes partition
larger corpora into segments, never silently raise a cap. Maximum-length fields
are validated against both remaining bytes and configured limits before reserve.

Varints are canonical unsigned base-128, at most 10 bytes for u64; reject overflow,
overlong encodings and zero deltas where IDs require strict increase. Validate
UTF-8, enum tags, spans against ObjectKey.byte_length, dictionary/dense IDs,
postings order, cardinalities and forward/reverse consistency before exposing
queryable data. Reject duplicate FactKeys with conflicting payloads. Section
checksums and envelope checksum must both pass before generation publication.

`ValidatedSegment` borrows immutable `Arc<[u8]>` or uses bounded positioned
reads under a pinned storage capability. No unchecked slices or decoder panic.
Partial/short reads yield `CorruptIndex`/IO errors. Queries must not allocate
from an unvalidated record count. Fuzz arbitrary bytes and every truncation
boundary; valid encodings are the positive controls for all refusal cases.

## Manifest v1 and overlay semantics

Manifest magic is `DCTXMAN` + NUL; use the same 80-byte header and 64-byte
directory envelope with independent major/minor = 1/0 and kind registry:

| Kind | Required manifest payload |
|---:|---|
| 1 | Namespace/root ID, generation u64, optional previous generation digest, SourceSnapshotDigest, ViewKey and captured policy epoch |
| 2 | Sorted segment references: SegmentObject Digest, full length, role, schema, expected section capabilities |
| 3 | Sorted permitted path mapping to FactKey/local IDs, metadata-only entries and freshness/completion status |
| 4 | Base manifest reference plus replacements/additions/deletion tombstones; explicit overlay generation |
| 5 | Resolver/extraction/parser revisions, environment digest, pending dependency scopes and capability requirements |
| 6 | Reserved. Encoders omit it. A payload that carries a git object id or other VCS identity is refused. Decoding and indexing use kinds 1–5 only |

ManifestObject digest is identity kind 10 over final envelope bytes. Manifest
references form a bounded acyclic graph; maximum depth 32, then flatten under
a reservation. Counts obey caller/decoder caps. Metadata paths and policy
projections cannot reveal denied content. View and revision identity are the
SourceSnapshotDigest, ViewKey and dctx generation from authorized file bytes
and file metadata. The core manifest stores no HEAD, index, worktree label or
git object id. Base and overlay generations are dctx generations.

Exactly one effective entry per path: overlay replacement/addition wins;
tombstone masks base. Deleted/replaced file-owned edges are masked too, and
reverse dependents re-resolve before a generation can claim complete resolution.
Unmerged states and high-fan-out updates are explicit partial/pending scopes.
No stale path survives just because its shared fact segment remains retained.

Current pointer sketch: `DCTXPTR\0`, pointer major/minor u16, generation u64,
ManifestObject Digest, manifest length u64, followed by SHA-256 of preceding
pointer bytes. File length is derived and bounded from the Digest framing.
The store retains a prior committed pointer for recovery. It never treats a
newer staged manifest as committed merely because its timestamp is later.

## Publication, recovery and reader protocol

1. Acquire an OS-released per-view writer lock. Reserve worst-case new bytes
   globally, including pointer/manifest/lease metadata, and pin reused inputs.
2. Write unique staging objects under the reservation; validate and sync them.
   Publish immutable filenames without overwriting an existing object's bytes.
   If identical name exists, verify identity before reuse.
3. Write/validate/sync manifest and dependency closure, retain the previous
   committed pointer, then atomically replace current pointer. Sync directory
   where supported. Pointer swap is the visibility linearization point.
4. Complete accounting, release writer/input pins. Old generation remains live
   while retained/pinned; failed publication leaves reclaimable staged objects.

Reader acquisition shares a short coordinator handshake with GC: read current,
validate manifest, register lease, confirm generation still retained, then
release coordinator. Retry a bounded number of times on acquisition races,
otherwise `Busy`. Query immutable data without per-record locks. A handle sees
one generation; source read verification and current authorization remain separate.

GC marks under the same coordinator/epoch protocol, retires references and
rechecks leases before unlink. Never rename-over/truncate live segments.
Crash recovery reconciles reservations and validates current and previous
committed pointer closures, choosing newest valid committed one. An invalid
current closure falls back to previous or returns `CorruptIndex`; uncommitted
newer segments are not promoted by guess. Recovery inputs remain until validated.

OS lock release on exit, not PID age, proves crashed ownership gone. A missing
required lock/atomic-replace/sync capability returns `UnsupportedDurability` or
an explicitly selected ephemeral/weaker-durability mode. POSIX rename alone is
not a durability promise. Windows open-file replacement/unlink behavior gets
its own adapter tests; failed deletion remains counted pending storage.

## Deterministic query and evolution gates

Ranking is versioned deterministic fixed-point lexical scoring with field
weights/name/public/tier-0/centrality/focus boosts, checked/saturating documented
arithmetic and full public ID tie-break (score descending, ID ascending).
Ranking has no VCS recency term. Freshness file metadata is not a commit time
and is not read from `.git/`. C1/C5 freeze score
constants as a ranking revision and compare relevance to SQLite; binary-format
choice alone does not define relevance. Candidate truncation and pending scopes
must be disclosed. Same ranking revision/inputs produce identical bytes.

Map format mismatch can rebuild only current authorized content. Pack history
uses its own versioned importer and must not be discarded by map recovery.
Legacy SQLite remains in its own namespace. No in-place mutation of old readers'
objects; schema/parser changes create new keys. Promotion needs A2 model checks,
C1 roundtrip/fuzz/corruption tests, C3 crash/Windows tests and C5 five-verb parity.
Mmap/FST are future reviewed capabilities; v1 never requires unsafe or mmap.
