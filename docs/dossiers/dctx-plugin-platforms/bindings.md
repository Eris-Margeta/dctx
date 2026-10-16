# Resource and executable binding

PL-R · 2026-10-16 · PL-C S-FILE, S-ADMISSION, S-PRIVACY and PP2/PP4/PP10.
Platform design for PL-A and file/program brokers; no implementation evidence.
Depends on authorized resource handles, policy epochs and immutable activations.

## Threat boundary

Prevent an untrusted plugin or hostile repository layout from substituting a
resource between permission check and use. Symlinks, hardlinks, junctions,
mount changes, parent renames, writable executables and concurrent editors are
distinct cases. Root/admin/kernel compromise is outside the native boundary.
Do not silently exclude ordinary same-user filesystem races from S-FILE.

An OS-confined plugin cannot write the protected namespace; an unrelated
same-user process may still be able to. Owner-only mode `0700` is not immutable
against the owner. A user-supplied protected namespace must specify the principals
excluded from writes, how exclusivity is enforced and how it is lost/revoked.
Host mutexes serialize dctx clients only. POSIX advisory locks, Git's index.lock
convention and Windows share modes have different coverage, not interchangeable
guarantees. A provider must refuse a guarantee it cannot implement.

## Authorization then handle-relative access

1. Intersect the manifest, consent, organization ceiling, session selection and
   current policy epoch before opening or hashing source bytes. Bind the opaque
   resource to plugin, invocation, root identity and authorized operation.
2. Open the root directory under the caller's binding; resolve path components
   relative to retained directory handles. Never `canonicalize` then separately
   open an ordinary pathname. Reject links/reparse points, unexpected mount
   transitions, nonregular files and hardlinks for the Git profile.
3. Inspect identity and metadata through the opened handle, then perform bounded
   reads from that handle. A handle prevents path replacement from changing the
   object read, but does not by itself prevent in-place writes to the object.
   Stable-content reads need an immutable snapshot or enforced writer exclusion.
4. A precondition binds root/parent/file identity, content generation/digest,
   permissions, selection epoch and operation scope. Size/mtime alone are not
   a content generation. Revalidate at each continuation and commit; return
   `Conflict` without reading or disclosing newly substituted source content.

| OS | Candidate read primitive | Important limit |
|---|---|---|
| Linux | Safe wrapper for `openat2` with `RESOLVE_BENEATH`, `RESOLVE_NO_SYMLINKS`, `RESOLVE_NO_MAGICLINKS`, and `RESOLVE_NO_XDEV` where policy forbids mounts | Requires kernel/API support; magic links and bind mounts need explicit handling. Descriptor access does not freeze file contents. |
| macOS | Handle-relative component walk using `openat`, no-follow and directory/type/identity checks | No automatic `openat2` equivalent; directory moves and mount ancestry must be prevented or detected under a protected namespace. |
| Windows | Safe handle-relative opens, reparse-point checks, file ID/volume checks and controlled sharing | String canonicalization/GetFinalPathName alone cannot close reparse races. Public wrapper coverage is unresolved. |

No path component, rejected path or administrative index name is sent to a
plugin merely because the host inspected it. Redact external program errors.
Close all nonprotocol descriptors before launching workers; FD inheritance can
bypass pathname policy and disclose capabilities the policy never granted.

## Existing-file replacement is not ordinary atomic rename

Stage bounded bytes in a private sibling file with restrictive permissions;
preserve required metadata, flush as needed, and revalidate authority. Publishing
with `renameat`/`renameat2`/`ReplaceFileW` can make replacement atomic for readers.
It does **not** atomically mean “replace only if the current content and identity
still equal the precondition.” Another writer can act between the last check
and rename. `RENAME_NOREPLACE` tests absence only; `RENAME_EXCHANGE` swaps names
without validating old content. Neither is general compare-and-replace.

The existing-file-only Git contract also forbids silently recreating a removed
file. Preserve this under concurrent unlink, not merely in an earlier `exists`
check. An after-the-fact hash cannot undo a wrong overwrite or confidentiality
violation, and blindly restoring the old bytes can overwrite another edit.

Accept a mutation only when the platform adapter supplies enforced exclusion
covering the file **and parent namespace** for the entire check/commit, a
genuinely conditional filesystem primitive with the required semantics, or an
explicit caller-managed protected namespace with those guarantees. Otherwise
return `Conflict`/`UnsupportedPlatform` as appropriate before mutation. A future
proposal for cooperative-editor-only behavior requires a contract decision;
PL-R does not weaken PL-C to make ordinary desktop files appear race-safe.

Git staging needs the same guarantee across `.gitignore`, config, attribute
absence, repository identity and the index transaction. Git's own index lock
does not stop an editor changing `.gitignore` or `.git/config`. A snapshot/shadow
repository could isolate reads, but requires an independently approved index/
object merge transaction and must not silently change PL-C's actual-root recipe.

## Bind verified bytes to launch

| OS | Candidate binding | Refuse when |
|---|---|---|
| Linux | Verify copied bytes in sealed executable memfd; execute that descriptor; pin loader/libraries in protected mount view | Safe exec/seal wrapper, executable policy, one-shot exec gate or runtime immutability is absent |
| macOS | Protected executable/runtime namespace for the full check/exec interval; no source rewrite or path replacement | Only hash-then-path-exec or a code-signature check is available; no protected namespace |
| Windows | Protected namespace, file/parent handles denying write/delete where applicable, exact image identity through suspended creation | Pre-existing writable handles, DLL substitution, reparse/parent replacement or API lifetime proof remain possible |

Do not execute scripts or search interpreters in the initial native profile.
Executing the same path as the current dctx image does not establish that the
path still names its verified containing release. Bind it again. Revocation
closes admission; active invocations lease the exact version until reaped.
Activation selects complete immutable versions atomically, with no mutation of
an active image. Network download, verification, activation and execution are
separate states; install never runs an artifact as a validation step.

## Required evidence

PL-A fixtures race each path component, target content and executable/runtime
byte source through open/read/commit/launch, including ABA identity changes,
hardlinks, mount/junction swaps, removal of the existing file and simultaneous
CLI/app updates. Pair every refusal with an unchanged valid operation. Tests
must observe actual accessed/written objects, not just the returned path string.
Record crash points and audit durability: before intent means no effect; after
mutation with missing result means `EffectUnknown`, never automatic retry.
