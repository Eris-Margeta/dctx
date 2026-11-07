<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# dctx plugins: architecture and API

Status: current design reference. See the [user guide](../guides/PLUGINS.md) for
available commands and platform limits, and [ADR-017](../ADR-017-Plugin-System.md)
for the architectural decision. The [original proposal](../archive/design/PLUGINS.md)
is retained as history.

## Protocol and lifecycle

Plugins run out of process and speak bounded, length-framed UTF-8 JSON over stdio.
The [API v1 contract](plugin-api/README.md) defines negotiation, invocation,
results, capabilities and receipts. A process boundary alone does not provide
security isolation. Independently installed plugins require tested native
confinement; unsupported operations fail closed.

The host starts and reaps processes, applies cancellation and progress limits,
and brokers effects. Plugins receive only selected, authorized facts. Granting
capabilities does not grant arbitrary filesystem, network or process access.

## Authority and audit

Effective authority intersects the manifest, explicit user consent, organization
policy, host limits and current selection. Check it at admission and commit.
Configuration cannot create consent. Installation, enablement, verification and
grants are separate states; a disabled plugin remains disabled across upgrades.

Record effect intent before mutation and completion afterward. If completion
cannot be established, report an unknown outcome and reconcile it before retrying.
Audit records contain bounded metadata, without source contents or credentials.
See [properties](plugin-api/properties.md) and the
[formal models](../formal/dctx-plugins/README.md).

## Distribution and trust

Signed manifests identify exact platform artifacts and digests. Verify signatures,
bounds and identity before activation; activate atomically and retain offline
operation. Ed25519 verification is provided through `dctx-ed25519`, which wraps
`ed25519-dalek`. Signing keys belong to release tooling.

The [distribution contract](plugin-api/distribution.md) defines signing inputs,
index updates and installation transactions. The [release formats](plugin-release/CONTRACT.md)
define supporting metadata. Contracts describe requirements; release evidence
and the user guide determine which platform operations are qualified today.

## Bundled Git plugin

`plugins/dctx-plugin-git` handles explicit `.gitignore` protection and brokered
staging. The core does not spawn Git or use Git object identities. The
[Git contract](plugin-api/git.md) specifies consent, safe configuration and effects.
A bundled exception must be reported explicitly; it does not authorize an
independently installed override or imply native confinement.

All first-party plugins use Apache-2.0, copyright Eris Margeta, like the host.
