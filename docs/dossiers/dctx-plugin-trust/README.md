# Plugin trust and decision 081 dossier

PL-R · researched 2026-10-16 · repository and existing knowledge; no network.
Implements DX-PLUGIN §7.1 trust research under decision 081 option B and PL-C.
PL-I/PL-X/PL-REL consume this with tls.md and dependencies.md; no signing occurs.

## Decision and existing implementation

**Use `dctx-ed25519` for verification in dctx; never add runtime signing.**
decision 081 supersedes PLUGINS.md §5.2's earlier proposed external verifier and
decision 8. `ed25519-dalek` remains a dev-only differential oracle in dctx.
Release/key manager tooling has a separate audited-signing boundary; its signing
dependency is not allowed to leak into dctx's production graph.

| Component | Repository evidence | What it does not establish |
|---|---|---|
| `dctx-ed25519` 0.1.0, Apache-2.0 | [manifest](../../../crates/dctx-ed25519/Cargo.toml), [API](../../../crates/dctx-ed25519/src/lib.rs), [policy](../../../crates/dctx-ed25519/README.md): no production dependencies, no_std, forbid unsafe; `verify(key, message, signature)` | An independent cryptographic audit or a test pass in this packet |
| `dctx-hash` 0.1.0 | [manifest](../../../crates/dctx-hash/Cargo.toml): no production dependencies; first-party SHA-256 for authenticated content/fingerprints | Publisher authenticity by itself; manifest currently says MIT although the plugin design's project licensing direction is Apache-2.0; PL-REL must reconcile inventory |
| `ed25519-dalek = 2.2.0`, `sha2 = 0.10.9` | Exact dev-only declarations in the verifier manifest | Permission to use them in runtime dctx; test coverage is not signing-library audit evidence |

Verifier inputs are public data. Its variable-time implementation is not a
secret-key/constant-time signer and must not be repurposed as one. Keep the
zero-external-production rule separate from std, OS and adapter native code.

## Exact authenticated bytes

[PL-C distribution](../../design/plugin-api/distribution.md) is normative:

```text
manifest_message = ASCII("dctx-plugin-manifest-v1") || exact_bytes(plugin.json)
index_message    = ASCII("dctx-plugin-index-v1")    || exact_bytes(index.json)
signature       = exactly 64 raw bytes (R || S)
public_key      = exactly 32 raw bytes
fingerprint     = lowercase_hex(SHA256(public_key))
```

No NUL, separator newline, length prefix, JSON reserialization, prehash,
Ed25519ph or Ed25519ctx. Preserve the downloaded bytes and pass the constructed
bounded message directly to `dctx_ed25519::verify`. The verifier itself adds no
domain. A signature over an index cannot substitute for the manifest signature;
SHA256SUMS must agree with the signed target table and has no separate authority.

Resolve the publisher-key identifier through an existing pinned/approved local
trust record, not a key downloaded alongside the signature. Perform bounds and
strict decoding, signature verification and schema/semantic checks before
activation; valid cryptography does not excuse duplicate JSON keys, unknown
fields, wrong ID/target, capability expansion or invalid protocol versions.

Official keys ship pinned with the verified containing release. Custom
publisher approval requires an independently authenticated fingerprint or an
organization trust bundle. A GitHub repository/owner, HTTPS certificate, signed
index listing or a matching content digest is not publisher-key approval.

## Verifier acceptance policy and required gates

The checked-in implementation checks canonical key/R point encodings,
`0 <= S < L`, all small-order A/R points, and the cofactorless equation
`[S]B = R + [SHA512(R || A || M) mod L]A`. Mixed-order points are governed by
that equation; do not describe the implementation as requiring prime-subgroup
membership or ZIP-215 acceptance. All lengths must be exact. See the crate
README for negative-zero encoding and reduced-challenge details.

Checked-in [fixture provenance](../../../crates/dctx-ed25519/tests/data/README.md):
all five RFC 8032 §7.1 vectors and the full Wycheproof corpus at revision
`5722833ca004983abd1a91bcb6c24596d50ac0f9`, SHA-256
`752d2ea7d7c6cf4736381b6cbacb61f8182b126ab7cd9b058f00c50084975536`.
The corpus declares 151 cases: 88 valid, 63 invalid; separate fixtures cover
small-order, mixed-order/cofactor and mutation cases. This packet reads the
policy and provenance; it does not rerun or certify their correctness.

decision 081 requires **any** vector or differential-fuzz disagreement to block
release, including disagreement on malformed inputs. No skipped cases, changed
expectations or “strictness exception” waivers. The oracle should be dalek's
strict policy, with explicit extra canonical-decoding checks where necessary;
record the comparison definition rather than silently relaxing the dctx policy.
An unresolved discrepancy goes to the verifier owner. PL-I must also test domain
confusion, bytes changed by JSON whitespace/order, substituted keys/targets,
truncation and a well-formed manifest with an invalid signature.

## Signing only in release/key manager tooling

1. PL-REL builds reproducible per-target artifacts and inventories code,
   capabilities, versions, licences, runtime resources and digests. Freeze the
   exact bounded manifest/index message bytes before requesting a signature.
2. An authorized release operation gives the key manager an opaque key binding,
   message/domain, artifact identity, intended publisher/release sequence and
   scoped approval. The key manager validates that scope; arbitrary agent-provided
   bytes must not become an unrestricted signing oracle.
3. The audited signing library runs in the authorized release/key manager process
   that holds the private key. Tooling outside that trust boundary receives only
   public key identity, signature and receipt. Do not export key bytes into agent
   prompts, repository files, build logs, environment variables or CLI arguments.
   The precise existing key manager API is not invented by this dossier.
4. Verify the returned signature with dctx-ed25519 over those exact bytes, then
   bind release provenance and approval receipts. Publication and real signing
   remain separately authorized effects; PL-R and test fixtures grant neither.

`ed25519-dalek = 2.2.0` is a concrete **signer candidate**, not a finding that
this version/feature closure has been independently audited. A known audit of
the dalek project or another version is insufficient. PL-REL must attach the
audit/report revision, reviewed scope, selected backend/features, delta review,
remediation status and current security review. If those are unavailable, real
signing is blocked. Do not substitute the first-party verifier as a signer.
Deterministic Ed25519 signing does not remove the need for secure key generation,
storage and access control. Test keys are conspicuously nonproduction and cannot
authorize installations under production publisher identities.

## Install, freshness and lifecycle

TLS protects transport; publisher signatures bind release authority; OS
confinement limits execution. All three are independent. HTTPS and offline
bundles use the same manifest/digest/trust rules. See [tls.md](tls.md).

Resolve → download → verify → stage → install inactive → activate under the
shared CLI/app lock. Verify streamed size/digest and all declared runtime
resources. Use PL-C's bounded uncompressed DCTXPLG1 container, regular entries
only, no duplicate/case-alias names, paths, links, devices or hooks. Publish
complete immutable version directories and an atomic activation record; active
workers lease exact versions. [Launch binding](../dctx-plugin-platforms/bindings.md)
must still prevent replacement between verification and execution.

Maintain monotonically accepted publisher/plugin release sequences and explicit
trust-state generations. Refuse unrequested downgrades, revoked keys/artifacts,
unknown publishers and unapproved authority expansion. Rotation needs an
authenticated old/new transition or explicit reapproval. Exact-digest rollback
records a reason and cannot revive revocation. Filesystem rollback of the whole
trust store cannot be solved by a signed sequence alone: state the protected
storage/external trust assumption rather than claiming a trusted clock/counter
against a malicious same-user administrator.

Offline status is last-known revocation plus age. A freshness ceiling can deny
activation; do not perform an implicit network lookup or claim current absence
of revocation. Verified install is not enabled state. Third parties additionally
need a qualified OS provider before enablement. Sticky disable, consent and
organization ceilings win over release signatures and DTS preferences.

Bundled Git is authenticated by the containing executable/package's release
manifest and inventory. Avoid circular self-hashes or invented separately
hashed embedded code slices. An override has its own external artifact and
must meet the ordinary confinement gate. Strict builds keep plugin/trust IO
adapters and all Git behavior outside the core closure.

## Unrun release evidence

PL-I/PL-REL must provide signed fixture positive/negative controls for manifest
and index domains; unknown/rotated/revoked keys; corrupt payloads and runtime
resources; duplicate JSON keys; stale offline trust; replay/rollback; target and
capability changes; concurrent activation; crash recovery and launch substitution.
PL-ED owns arithmetic/vector/differential proof. PL-V connects those results to
the final remote suite. This dossier's directory check proves none of them.
