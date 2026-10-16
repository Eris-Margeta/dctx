# Release and installation contract v1

PL-C · normative · PLUGINS.md §5, decision 080 and decision 081.
PL-I/PL-X/PL-REL implement this contract after PL-R research and approval.
No locator in the fixture corpus is a published service or a trust anchor.

## Authenticated bytes

`plugin.json` is bounded UTF-8 JSON conforming to the manifest schema and
semantics. Preserve its exact bytes. `plugin.json.sig` is exactly 64 raw
Ed25519 signature bytes, no text armor or JSON envelope. Verification message:

```text
ASCII("dctx-plugin-manifest-v1") || exact_bytes(plugin.json)
```

There is **no NUL, newline, length prefix or prehash** between domain and
document. Use ordinary Ed25519, not Ed25519ph or Ed25519ctx. Index signatures
likewise sign `ASCII("dctx-plugin-index-v1") || exact_bytes(index.json)` with
the pinned index key. Different domains are mandatory; neither checksum lists
nor index signatures substitute for the publisher's manifest signature.
SHA256SUMS mirrors each authenticated target filename/hash; any disagreement
refuses the release. Manifest fields authenticate identity, version, publisher,
capabilities, protocol and all platform/runtime payload lengths/digests.

Public keys are exactly 32 bytes; fingerprints are lowercase hex SHA-256 of
those exact bytes using dctx-hash. `publisher_key` is a local trust-record ID,
never a filesystem path or a key downloaded next to its own signature. The
verification adapter resolves that ID from pinned/explicitly approved trust
records. Unknown keys require independent fingerprint or organization approval.
First-party safe-Rust Ed25519 verification must reject noncanonical encodings,
out-of-range scalars, invalid/small-order points and equation failures; its
dedicated conformance packet pins strict acceptance vectors and differential
checks. Do not implement signing in runtime dctx. Release tooling uses an
audited signer library with key manager-held keys and a separate signing grant.
Do not treat this document or a public test key as that grant.

Reject invalid schema and duplicate keys even when a signature verifies.
Revocation, signer changes, downgrade and consent expansion checks are separate
from the cryptographic equation. Maintain a monotonic publisher/plugin release
sequence; reject unrequested rollback. Exact-digest explicit rollback records
a reason, cannot revive revoked artifacts, and still meets consent/platform
policy. Rotation requires an authenticated old/new transition or reapproval.
The structure of key manager/organization trust bundles belongs to those existing
services; v1 invents no competing trust-bundle format or secret discovery.
Offline status is last-known revocation state plus age, never a claim of fresh
network verification. A policy freshness ceiling may refuse offline activation.

## Discovery, fetch and offline bundle

Index schema has a 4 MiB default cap, depth 32 and 4096 member limit per bounded
page; a large registry uses separately signed index files/pages, not an
unbounded JSON tree. Require unique IDs, per-plugin version/sequence entries,
repository syntax and canonical publisher fingerprints. Display summaries with
control escaping. Index inclusion grants nothing. A GitHub repository is only
a locator; its transfer cannot change an installed plugin's pinned identity.
Search uses cached verified JSON; refresh is explicit and fetches no executable.

Installer HTTPS belongs to a reviewed adapter, not a plugin network capability.
Default per-artifact cap is 256 MiB; explicit policy may adjust it. Check public
origin and all redirects, reject URL userinfo/fragments, non-HTTPS and private,
loopback/link-local targets unless installation policy explicitly permits the
destination. Bind DNS/redirect checks to the actual connection. No ambient
browser/Git credentials, shell curl/git-clone or package/interpreter downloads.
Artifact URLs are safe filenames resolved beside the selected manifest URL.
Show newly requested origins; API errors/rate limits produce typed failures.

The optional `.dctx-plugin` file has this exact uncompressed v1 layout:

| Bytes | Meaning |
|---|---|
| 8 | ASCII `DCTXPLG1` |
| 4 | little-endian u32 table byte length, 1..262144 |
| table length | UTF-8 JSON table below |
| remainder | payloads concatenated in table order, with no padding/trailing bytes |

Table is a closed object `{schema:1, entries:[{name,bytes,sha256},…]}` with
1..128 entries. `name` is a unique safe single filename (common file schema
plus platform-name checks), `bytes` is a nonnegative safe integer, `sha256` is
64 lowercase hex digits. Payload offset is the checked sum of prior lengths;
zero bytes are permitted for ancillary text only. Require plugin.json,
plugin.json.sig, exactly one selected target artifact and its declared runtime
resources; optional LICENSE, NOTICE, provenance.json and approved signed trust
metadata are data, never executable hooks. Reject undeclared executable
payloads. Verify the table's hashes against actual bytes, then manifest/signature
and signed target digests independently. The table itself is not a trust root.
Container/extracted totals obey explicit installer storage limits; never
allocate from an unchecked length/sum. No archive compression, paths, duplicate
names/case aliases, symlinks, devices or extraction scripts. Copy regular data
only to an owned staging area. This envelope does not bypass any trust check.

## Registry, activation and visible states

Resolve → downloaded → verified → staged → installed-inactive → activated.
Install/update holds a shared CLI/app operation lock. Stream size/hash checks,
verify signed manifest/payloads/target/host API, publish an immutable version
directory, then atomically switch the host-owned activation record. Never run
code during install or partial recovery. Concurrent/crashed updates expose old
or new complete activation only. Launch revalidates and binds verified bytes
to the executed identity through a protected namespace or safe platform primitive;
inability to close substitution races is a refusal. Workers lease their exact
version; removal/GC cannot unlink it before leases end. Version state explicitly;
no post-install/migration scripts. Preserve unresolved effect intents.

Third-party installs are inactive until explicit enablement, consent and tested
OS confinement. `install --enable` may combine these with a concrete consent
display, never an unattended generic yes. Bundled Git is enabled by distribution
preference but needs its recorded install/migration consent; a copied binary
shows enabled/needs-consent and pack still works. A sticky disable/removal mask
survives all updates. Release signatures cannot grant runtime authority.

List/info/help inspect verified metadata only. List reports ID/version/origin,
alias, enabled/disabled, consent, verification, compatibility and platform state.
Info adds provenance, digest, effective grants/reasons, lock and latest receipts.
Disable revokes admission and cancels workers; remove waits for leases/reaping,
retains audit and user DTS, and masks bundled bytes. Verify rechecks size/hash,
signature/trust/revocation/target/file identity without executing; failures
quarantine. Update is explicit (including --check), leaves old activation until
complete, and cannot expand authority without consent. Run is explicit routed
plugin invocation; management errors have typed JSON and nonzero exit status.
None of search/list/info/verify/help creates project `.dctx` or starts Git.

Locations are exactly PLUGINS.md §5.4: Linux XDG data/config dctx roots, macOS
Application Support dctx plugins/plugin-config, Windows LOCALAPPDATA Plugins
and APPDATA registry. Validate/encode ID components; private permissions/ACLs.
Staging shares the data filesystem for atomic publish. Consent, trust, versions
and state are not disposable map cache. Explicit portable roots are allowed;
libraries receive roots/host explicitly. No project auto-discovery, global PATH
scan, system installation or silent second registry. Existing paths import only
through a recorded migration.

Bundled Git's inventory is authenticated by its containing release manifest,
including the containing executable digest; do not create a circular embedded
manifest hash or pretend to hash an independently stored code slice. Same-binary
worker entry runs before ordinary CLI/UI/config initialization. An override is
explicit, independently verified and identified as origin=override. Reverting
respects the previous enabled preference and still-valid consent; sticky masks
win. Bundled update reports the containing release's update path. No startup
fetch, auto-enable, scheduled update or daemon. Installed default distributions
work offline; strict builds omit host/worker/distribution adapters and Git while
pack/map remain usable.
