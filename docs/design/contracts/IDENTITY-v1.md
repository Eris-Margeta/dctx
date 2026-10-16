# Content identity contract v1

DX-A0b · sketch version 1.0 · successor contract, not implemented evidence.
Defines decision 077 own identities, optional VCS aliases and contextual sharing.
Depends on PRODUCT-CONTRACTS C03/C04/C05; used by B0/B3/B4/B5/C1/C5.
All encoded integers below are unsigned little-endian unless explicitly text.

## Canonical identity

```text
Digest = { identity_version: u16, algorithm: u16, bytes: Vec<u8> }
algorithm 1 = SHA-256, exactly 32 digest bytes; 0 is invalid
ObjectKey = { digest: Digest, byte_length: u64 }
ObjectKey text = dctx:object:v1:sha256:<64 lowercase hex>:<decimal byte_length>
ObjectKey.digest.bytes = SHA256(exact source bytes)
```

Identity version 1 means exact file bytes: no newline, Unicode, whitespace,
BOM, path or Git blob-header normalization. Byte length is independently checked.
Empty files are valid. Uppercase hex may be normalized at an input boundary;
canonical output is lowercase with no leading zeros in decimal lengths.
Unknown algorithm/version yields `UnsupportedIdentity`, never length-based
guessing. A Git SHA-256 OID hashes a Git object header plus payload and therefore
is not the above digest, even when lengths happen to match.

Binary Digest framing: `identity_version:u16, algorithm:u16, digest_len:u16,
digest_bytes[digest_len]`; then `byte_length:u64` for ObjectKey. Version 1 permits
only algorithm 1 with length 32. Future algorithms require a new advertised
capability and explicit migration; no assumption that all future hashes are
32 bytes/64 hex. Persist full values, never display prefixes as keys.

Structured identities use `SHA256("DCTX\0" || kind:u16 || schema:u16 ||
payload_len:u64 || canonical_payload)`, wrapped in Digest. Kind tags:
1 FactKey, 2 SourceSnapshot, 3 ViewKey, 4 SelectionPolicy, 5 ExtractionPlan,
6 ResolverEnvironment, 7 BodyFingerprint, 8 ContextPlan, 9 SegmentObject,
10 ManifestObject. File content uses raw SHA-256 above, not this framing.
Distinct domain newtypes prohibit comparing an ObjectKey with a plan digest.

Canonical payload encodings have fixed field order, explicit lengths for all
variable fields and sorted unique maps/sets; no native structs or JSON hashing.
Text is UTF-8, length-prefixed by u32; collections use u32 counts and item lengths.
Reject overflow, duplicate keys and noncanonical encodings before hashing keys.
New fields that change interpretation require the corresponding schema version.

## Fact, root and view keys

```text
FactKeyV1 = (ObjectKey, language_tag:u16, dialect:utf8,
             parser_id:utf8, parser_revision:utf8,
             ExtractionPlanDigest, fact_schema:u16)
SourceSnapshotV1 = sorted [(relative_path, entry_kind, ObjectKey?)]
ViewKeyV1 = (TrustNamespace, RootIdentity, SourceSnapshotDigest,
             overlay_generation:u64, SelectionPolicyDigest,
             resolver_revision:utf8, ResolverEnvironmentDigest)
```

Language tags: 1 Rust, 2 JavaScript, 3 TypeScript, 4 Markdown, 5 opaque.
Dialect distinguishes JS/JSX/TS/TSX or Rust edition; extraction digest binds
parser options/limits that affect emitted facts. Parser revisions include the
extractor revision and upstream parser/grammar versions when used. Resource
failures cannot be cached as complete facts under the successful FactKey.

`RootIdentity` is an opaque caller-provided or registry-allocated stable ID,
scoped to a trust namespace; it is not a Git path or commit. A registry binds
root identity to an opened filesystem identity and requires explicit rebind
after a move/replacement. Paths are root-relative components, never absolute,
reject `..`/NUL and collisions. Non-UTF-8 paths require a declared lossless path
encoding capability or typed unsupported-path refusal; never lossy merging.
Default portable wire IDs use UTF-8 `/`-separated paths with reserved delimiters
percent-escaped, no Unicode/case normalization. Filesystem case rules belong
to the Source capability and cannot silently collapse distinct entries.

Source snapshots contain only the authorized projection: Include has a content
key; metadata-only entries have none; excluded entries are absent unless policy
permits their metadata. A manifest may describe a captured byte snapshot without
claiming that a live filesystem was globally atomic at capture time.
Dirty/add/delete overlays also work over a previous dctx snapshot in a plain
directory. HEAD/index/worktree labels are optional adapter metadata only.

Reusable payloads contain file-local declarations, spans, docs and unresolved
references. They never contain absolute paths, root IDs, resolved target IDs or
package layout assumptions. The same facts may resolve differently in two
roots. Exports/imports, manifests, compiler options and reverse unresolved-name
indexes determine dependent invalidation; large unfinished updates are flagged.
Sharing is within one user's explicit trust namespace after current permission
checks, never an authorization bypass across roots or organizations.

The reuse promise is once per retained compatible FactKey/namespace, with a
bounded per-key reservation. GC, parser/schema/extraction changes or distinct
bytes require reparse. Concurrent losers reuse only the validated published
facts; crashed/failed reservations cannot poison the key forever.

## Optional Git/VCS evidence

```text
VcsObjectRef = { provider, object_algorithm, full_oid, object_kind }
GitAlias = { repository_identity, VcsObjectRef, ObjectKey,
             equivalence_contract, freshness_evidence, evidence_version }
```

Git OIDs retain their own algorithm (SHA-1 20 bytes, SHA-256 32 bytes today);
never truncate to seven characters for storage or reject SHA-256 repositories
because a field was sized for SHA-1. Other VCS adapters may provide hints using
their own typed metadata. Neither Git nor any VCS is installed/discovered/run
unless the caller selected that capability.

To skip rehashing, the adapter must provide all of:

1. A previously validated alias binding the candidate bytes to dctx's own
   ObjectKey, with retained valid facts or bytes. A bare OID is insufficient.
2. Evidence that this authorized source's current bytes equal that object:
   immutable source capability or a documented freshness proof sufficient for
   the requested freshness mode. Stat equality alone is never strict proof.
3. No active/unknown conversion: CRLF, working-tree encoding, LFS and filters
   invalidate simple equivalence. Do not run filters to obtain/prove content.
4. Current policy approval, namespace compatibility and integrity/provenance
   validation. Weak/collision-suspect/untrusted OIDs cannot select facts alone.

Missing proof/alias uses one authorized read for own digest and parsing.
If facts are gone and bytes are needed, read/hash again. In a metadata-validated
view an alias may be reused with that explicitly weaker freshness label;
strict-current queries require proof or authorized byte validation. Strict mode
does not promise avoiding reads just because Git exists.

No broad `git status`, recursive history walk, subprocess filter execution or
packed-object delta-base access containing denied bytes. Git index metadata is
a separately granted capability, not source permission for `.git/` contents.
Transforms, sparse/unmerged/racy files and unavailable Git must fall back or
return typed uncertainty; never change content identity based on VCS presence.

## Freshness, body fingerprints and privacy

Freshness modes: `immutable_snapshot` (caller guarantees snapshot bytes),
`metadata_validated` (observed stamps, same-stamp edits may be missed),
`strict_live` (validate authorized bytes in the relevant dependency scope).
Report scope, observation boundary and `stale`/`unknown`/partial scopes.
Strict live cannot claim an atomic whole-tree filesystem snapshot without a
snapshot capability. Race or deadline yields flagged scope or typed `Stale`.
`read` always validates exact source bytes against ObjectKey before emitting
spans, including when the index was opened with metadata freshness.

BodyFingerprint is separate from ObjectKey. Version 1 normalizes only lexical
whitespace tokens outside strings/comments/literals, preserving token boundaries
with length framing; uncertain lexing uses exact-body bytes plus a distinct
normalization tag. This prevents whitespace inside literals being ignored.
Body match is move evidence, never content identity or proof of unique rename.
Duplicate matches yield `Ambiguous`; aliases expand on collision. Public IDs
remain path/name based with ordinals for overloads; fingerprints do not replace
them. Tier-1 summaries additionally bind extractor/model provenance and body
fingerprint, otherwise fall back to tier 0. The library never calls a model.

Policy revocation blocks new emissions through existing handles immediately
after the revocation is observed; already delivered bytes cannot be recalled.
Physical pin lifetime does not grant current access to a previously allowed
path. Content hashing itself is a read and obeys Include-only authorization.

## SHA-256 candidate and acceptance

DX-ARCH-04a remains open. Candidate: first-party, safe, streaming SHA-256 with
checked message-length accounting (reject input ≥2^64 bits), fixed block state,
standard padding, big-endian message words/output and specified wrapping u32
arithmetic inside SHA-256. Identity framing remains little-endian; the SHA
algorithm's internal endian convention is unchanged. No production `sha2`,
assembly, native code, vendored relabeling or first-party unsafe.

Required implementation tests, owned by B3 with explicitly cut module ownership:

| Test | Oracle / expected evidence |
|---|---|
| Empty bytes | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| ASCII `abc` | `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad` |
| NIST multi-block `abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq` | `248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1` |
| NIST million ASCII `a` | `cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0` |
| NIST byte-oriented CAVP short/long message vectors | Pin vector source/revision and file digest in future test fixtures; no network dependency in tests |
| Streaming equivalence | 0/1/55/56/63/64/65/119/120-byte padding edges, all splits of short messages, randomized chunks, zero-length updates |
| Fuzzed differential | Mutational/random byte inputs and chunk boundaries vs exact-pinned dev-only `sha2` with asm off; retain seed, iterations and minimized failures |
| Limits/canonical framing | Length overflow typed refusal, valid near-boundary state control; cross-domain/schema changes never alias by encoding ambiguity |

NIST vectors alone do not prove correctness. Differential fuzzing is required
in addition to ordinary property tests; a fuzz runner/dependency needs its own
approved dossier/packet. CI checks that `sha2` is absent from the successor
production hashing closure. Legacy baseline dependencies are separately listed.
These tests are requirements, not results generated by this design packet.
