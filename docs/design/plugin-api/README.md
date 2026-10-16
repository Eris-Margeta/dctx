# Plugin API v1 contract

PL-C · frozen 2026-10-16 · DX-PLUGIN §7.1.
This directory is the normative contract and input corpus for PL-P, PL-D,
PL-M and the later host, distribution and Git packets. It changes no product
behavior and authorizes no installation, execution, signing or publication.

## Authority and versions

[PLUGINS.md](../PLUGINS.md) §§1–6 is the design source. The packet's decision 080
disposition accepts decisions 1–7 and 9–12, with the following decision-2
adjustment. **OS confinement is mandatory before any third-party plugin is
enabled**, including its first handshake. The official bundled Git plugin may
ship with brokered permission-checked effects and host audit while confinement
is completed per OS. This exception is determined by the verified containing
distribution, never by a manifest flag, signer reputation or a claimed ID.
An independently installed Git override is not bundled and does not inherit it.
Report the exception as `bundled-git-pending-confinement`; do not claim native
isolation or completed platform-adversary evidence for that mode. All consent,
selection, broker, audit and Git recipe requirements still apply. PP4 uses this
explicit admission alternative; the confinement branch remains mandatory for
third parties. PP2's broker privacy rule is not relaxed, and an unconfined
process is not represented as an OS-enforced privacy boundary.

decision 081 supersedes decision 8 and conflicting §5.2 prose: use Ed25519, with
first-party **safe-Rust verification in dctx**. Signing belongs only to release
tooling using an audited library; the signing key stays in the key manager.
No production verifier dependency is introduced here. `ed25519-dalek` and
`sha2` are dev-only differential oracles under the supplied Rust standards.
DX-SHA's `dctx_hash::Sha256` supplies content digests. decision 078 prohibits Git in
the core; Git behavior belongs only to `plugins/dctx-plugin-git` and its
explicitly composed broker adapters, never a core compatibility shim.

| Version axis | Frozen value | Evolution |
|---|---|---|
| Bootstrap envelope | `wire: 1` | New entry protocol for incompatible bootstrap |
| Wire / host API | `1.0` | Negotiate optional minors; mandatory changes bump major |
| Manifest | `manifest_version: 1` | Independent from plugin semver |
| Hook payload | `hook_version: 1` | Explicit per declared hook |
| DTS source / plugin plan | `2` / `plugin_plan_version: 1` | Existing DTS v1 remains valid |
| Discovery / offline bundle | `schema: 1` / `DCTXPLG1` | Independently versioned |

## Files and validation layers

| Entry | Contract |
|---|---|
| [manifest.schema.json](manifest.schema.json) | Signed release metadata, capabilities, config, artifacts |
| [wire.schema.json](wire.schema.json) | Bootstrap and session message union |
| [dts.schema.json](dts.schema.json) | Compiled plugin-plan extension, not a new ContextPlan container |
| [index.schema.json](index.schema.json) | Public signed discovery data |
| [bundle.schema.json](bundle.schema.json) | Offline container's bounded payload table |
| [receipt.schema.json](receipt.schema.json) | Host-owned effect evidence, never plugin assertions |
| [semantics.md](semantics.md) | Required checks beyond JSON Schema |
| [wire.md](wire.md) | Framing, request/result matching, states and defaults |
| [dts.md](dts.md) | Source syntax, compilation, deterministic ordering |
| [distribution.md](distribution.md) | Exact signing inputs, bundles, activation and trust |
| [git.md](git.md) | Frozen default plugin outcomes and broker recipe boundary |
| [requirements.md](requirements.md) | Requirements §§1–6 to named implementation predicates |
| [properties.md](properties.md) | PP1–PP10 predicates and mutation witnesses |
| [fixtures/README.md](fixtures/README.md) | Corpus format, expected phases and positive controls |

JSON Schema dialect is Draft 2020-12. Register every `*.schema.json` by its
`$id` (`urn:dctx:plugin-api:v1:<filename without .schema.json>`) from this
directory. References are local identifiers, not URLs to fetch. `common`,
`config`, `capabilities`, `bootstrap`, `invocation`, `requests`, `results` and
`events` are supporting schemas. Object shapes are closed. Arbitrary JSON
Schema documents in manifests are forbidden; `config.schema.json` is a small
data-only type language. No schema executes code or resolves remote resources.

Apply validation in order: bounded byte decoding and duplicate-key/number
checks; structural schema; cross-field semantics; session/authority/privacy
checks; effects through audited adapters. The schema alone cannot enforce
integer spelling, UTF-8 byte limits, signatures, scope intersections, live
handles, dynamic result types or an acyclic plan. Every such rule is normative
in the linked files and assigned a predicate. Accepting one layer is not an
execution grant. Unknown fields or APIs are refused, not ignored.

The manifest design example was illustrative. This freeze fills its omissions:
release sequence, reason/diagnostic dictionaries, bounded command output types,
resource ceilings, artifact minimum OS/runtime resources and file-write size
ceilings. Limits and exact artifact locks are explicit, never inferred from
release versions. `output.list` supplies bounded continuation metadata for the
output-list pagination required by §3.1; it confers no content authority.

## Evidence boundary

The corpus contains real SHA-256 identities and supplied preimages, including
the checked-in `dctx-hash` NIST vectors. Small artifact payloads are **hash and
schema fixtures**, not native executables or trusted installable releases.
No test signature stands for release authority. PL-I/PL-REL must add signed
native fixture bundles under their own ownership and separate authorization.

This packet permits read/edit only. Its complete authorized acceptance set is
`test -d docs/design/plugin-api` and
`ls docs/design/plugin-api | grep -q .`, run once after final edits. These are
directory checks, not schema, Rust, platform, signature or formal verification.
No Cargo command, local validator or full product suite is run here. PL-P/PL-D
consume this corpus in authorized remote validation; PL-V owns the final
one-time workspace suite and promotion evidence. The matrix describes required
future predicates and never represents them as passed by this packet.
