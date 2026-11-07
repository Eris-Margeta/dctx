<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Semantic validation v1

Status: normative supplement to the schemas.
The protocol, template and host components implement these rules after structural
decoding. Predicate names below are stable references for the corpus and matrix.

## Records and declarations

**S-BOUNDS.** All integers have decimal lexical form `0|[1-9][0-9]*`, range
0 through 9007199254740991, and no fraction, exponent, sign or implicit cast.
All schema string lengths also bound UTF-8 **bytes**, not just Unicode scalar
counts. Reject embedded NUL in all strings, unpaired Unicode surrogates and
control characters in identifiers. Human text may contain newline/tab, but is
escaped on display. Apply the frame/manifest/index size bound before parsing.
Identifier, path, filename and digest patterns must match the entire byte
string; a regular-expression end anchor must not admit a final newline.
Reject duplicate keys at every depth before constructing a map. Depth counts
the root object as 1; member count sums object members throughout the document.
Use checked arithmetic for every offset, byte total, size and sequence.

**S-MANIFEST.** Manifest maximum is 256 KiB, depth 32, 4096 total object members.
IDs, aliases, command verbs, config keys, reason codes and target entries are
unique by semantic key, not just object equality. Plugin IDs are lowercase
`publisher/name`; each component must not be `.` or `..`, a reserved Windows
device name, or end in a dot. Alias claims may not shadow built-ins, internal
worker switches, another enabled plugin or path-routing rules. Version strings
obey SemVer 2.0, including no leading zero in numeric prerelease identifiers.
Protocol ranges have min <= max, one range per major. Select the highest
supported common major, then highest common minor; frozen v1 supports only 1.0.
Targets in the schema are candidates, not claims of confinement support.
Linux artifacts require `libc`; other targets forbid it. Filenames/runtime
resources are unique safe components: no `.`, `..`, trailing dot, Windows
device basename, separators, alternate stream or case-fold collision. License
is a bounded SPDX expression checked by release tooling, not a shell command.

**S-CONFIG.** Config keys and command record fields must be declared. Apply
defaults, then precedence; reject wrong types or unknown keys. Integer min <=
max and default in range; enums are homogeneous scalar values with default
in values; string defaults/values obey max_bytes; list defaults/values obey
max_items and the scalar item schema. Lists cannot nest. No environment
interpolation, executable paths, expressions or arbitrary schema evaluation.
Command records must have exactly the declared field set and approved types;
text requires `allow_text=true`. Ordinary hooks return `command_output=[]`.
Reason/diagnostic codes returned by plugins must occur in their manifest's
dictionaries; host errors use the closed error code set and safe reason names.

**S-CAPABILITY.** A capability describes a requested ceiling, never consent.
Required and optional requests cannot duplicate/conflict for one resource;
`outputs` are additional optional content requests. Every path is a literal
named project-relative resource, never a glob/directory traversal interface.
Reject empty components, trailing slash, platform-reserved names, case-fold
collisions and equivalent spellings that would bind another object. Host opens
resources against root identity; no prefix-string containment. `file.write`
does not imply read, create, delete or execution. `create` is separately
consented; Git v1 fixes it false. Version 1's only external program recipe is
`git.add-ignore`, with the explicit administrative resource. Unknown operations
require a future negotiated extension. `network=[]` is mandatory. Source-bearing
state needs `source_content=true`, separate provenance permission and current
selection on every read; Git state never stores source content.

**S-ARTIFACT.** Decode each fixture's bytes and use `dctx_hash::Sha256::digest`;
compare all 32 bytes and the exact byte length. Wire/plan digests have `sha256:`;
manifest/index hexadecimal fields do not. These spellings are not
`ContentDigest::Display` (`v1:sha256:…:length`); conversion is explicit and
preserves the separately carried length. A digest is integrity, never identity
trust or signature proof. Check manifest identity against the install record
and handshake, and authenticated filename/format/target/size against payload.

## Authority and effects

**S-GRANT.** At admission and at commit, require
`action ∈ M ∩ U ∩ O ∩ H ∩ S(t) ∩ A`, current epoch, remaining budget,
verified artifact and durable audit intent. Intersection is semantic scope
containment with minima for numeric ceilings; explicit denies win. Required
unsupported/denied capabilities block invocation; optional denials are omitted
and produce truthful degraded results. Installation, enablement, consent,
verification and platform support are distinct. Consent binds publisher,
plugin/artifact/manifest identity, scope, capability ceilings and revision.
Only an explicit approval or a pre-consented equal/narrower update policy can
create a derived record. Project DTS, an index and generic `--yes` cannot.

**S-ADMISSION.** Verified executable identity and no check/exec substitution
are prerequisites. Confinement must be established before third-party enablement
and execution, including hello. The sole temporary alternative is the host's
verified bundled-Git disposition in README.md; a plugin cannot request it.
Do not initialize with ambient secrets, root cwd, inherited capabilities,
filesystem mounts, network or descendant execution. Any platform inability
outside that exception returns `UnsupportedPlatform`. Brokered Git still needs
a separately proven constrained recipe, even under the bundled exception.

**S-PRIVACY.** `Exclude`, `OmitContent`, `DebugVisible` authorize zero source
reads, hashes or deliveries. Check selection before open/hash/cache lookup
that exposes content, and before metadata disclosure. Bind output/state handles
to plugin, invocation, root, epoch, generation and provenance. Denial of any
constituent refuses the whole derived output or regenerates an authorized
projection. No raw root inventory, cache/store path or historical index escapes.
Administrative Git metadata is internal to the recipe; never disclose `.git`
bytes, stored paths or excluded source/history as metadata. Excluded `.gitignore`
cannot be read, hashed, rewritten or staged; host rule loading is not disclosure
authority. Unknown provenance refuses rather than trusting old handles.

**S-FILE.** Bind resource handles to race-safe opened identities. Compare
preconditions at read continuation, upload and atomic replace. Offset zero
starts a read; all later file/state chunks require the first token. A changed
file returns Conflict without new bytes. New uploads start at zero with no
upload ID. Later chunks are contiguous, immutable and bound to resource,
invocation and precondition. Identical offset/bytes/final replay returns the
same upload result; overlap or altered replay conflicts. Empty final chunk is
permitted. Final seals the upload. Replace requires sealed upload, checked size
and growth, current permissions/identity, preserved metadata where supported,
and no ungranted creation. Reject symlink/hardlink/reparse races. Reclaim uploads
on replace, cancellation or revocation; no plugin sees temporary paths.
Separately consented creation uses a host-issued absence precondition bound
to the named resource and parent identity; commit atomically checks absence.
Git's existing-file-only profile never receives that creation authority.

**S-REPLAY.** Effect key is (artifact digest, project identity, invocation,
action_id); argument digest is SHA-256 of the canonical method/params bytes
defined in wire.md, excluding action_id. Identical key and arguments returns
the ledger receipt without executing twice. Different arguments return Conflict.
Partial/unknown effects never automatically retry, even if the plugin exits
successfully. An unreconciled pending intent yields EffectUnknown. Only verified
reconciliation or separately authorized retry can resolve it. New invocation
does not license retrying uncertain staging; content idempotency still applies.
Errors with partial/unknown effect_state must set retryable=false. A retryable
error is still not permission to repeat a mutation without ledger reconciliation.

**S-AUDIT.** The host durably logs intent before any effect or byte delivery,
then result/uncertainty linked to the intent. Audit unavailable => no new effect;
failure to persist completion => EffectUnknown and a pinned unresolved intent.
Host receipts determine committed effects; plugin receipt lists are references
checked for ownership, never proof. Safe metadata only: no source/replacement
bytes, raw argv/env, credential URLs, stderr/stdout or sensitive paths. Drain
stderr into a bounded transient ring. Audit grants, refusals, management,
progress transitions, cancellation and cleanup with host sequence/time and
policy/plan identities. Protect private logs and separate organizations. Rotate
only resolved segments; no telemetry or tamper-proof claim.
Receipt status refused requires effect_state=none; committed requires committed;
unknown requires partial or unknown and a retained intent. An earlier committed
effect remains a separate linked receipt when a later action is refused.

**S-STATE.** State handles are preallocated by initialize in the plugin's
versioned namespace. Writes replace at most one chunk with a precondition,
action ID and provenance; larger storage requires multiple bounded objects.
No automatic source persistence. Reclaim only own unpinned, unleased state;
GC never waits for unrelated core data. Check namespace, quota and current
provenance on every operation and log it. No migration/install scripts.

**S-RESULT.** Required pre-hook failure stops a still-reversible operation;
advisory failure warns and permits it. Post-hook failure preserves committed
output and returns `output_committed=true` in the host failure details. Earlier
effects remain in receipts; cross-plugin hooks are not a transaction. Failed
engine work emits no success hook. Explicit command failure returns nonzero.
Revalidate selection after preparatory writes before pack/index content reads.
Map format, ranking, budgets, confidence and authoritative cache remain core
contracts. Read-only map/lazy indexing cannot acquire mutation by hook choice.
