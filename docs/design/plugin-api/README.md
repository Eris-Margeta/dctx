<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Plugin API v1 contract

Status: current protocol design reference. This directory contains normative
schemas and conformance inputs. See the [plugin design](../PLUGINS.md) for the
architecture and [user guide](../../guides/PLUGINS.md) for implemented behavior.

## Authority and versions

Third-party plugins require native confinement before their first handshake.
Only the verified official bundled Git implementation may report
`bundled-git-pending-confinement` while platform qualification is incomplete.
This exception does not follow a manifest flag, signer reputation, claimed ID
or independently installed override. Consent, selection, broker and audit rules
still apply; the exception does not establish OS-enforced privacy.

Ed25519 verification uses `dctx-ed25519`, backed by the audited
`ed25519-dalek` dependency. Earlier first-party-verifier proposals are superseded.
Signing belongs to release tooling; keys are never embedded in plugin code.
`dctx_hash::Sha256` supplies content digests. Git effects belong to the explicit
plugin and broker adapters, without a core Git compatibility path.

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
No test signature stands for release authority. Release validation requires signed native fixture bundles with independent
key-management and platform evidence.

Schema fixtures do not establish signature validity, native execution,
platform confinement or release approval. Run the relevant conformance, platform
and formal checks and retain their results before making those claims.
