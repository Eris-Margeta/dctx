Historical design record; superseded by [the current documentation](../../../guides/PLUGINS.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
Current licensing: all first-party dctx code and documentation, including the desktop app and planned GPUI rebuild, are Apache-2.0, copyright Eris Margeta. Licence and pricing discussions below describe historical proposals.

# Trust dependency and audit inventory

PL-R · 2026-10-16 · decision 081 and DX-PLUGIN §7.1 dependency research.
Repository manifests/lock plus labelled existing-knowledge candidates only.
PL-I/PL-REL use this as an approval checklist, not approval or an audit report.

## Production verifier and content hashing

| Exact component | Licence evidence | Unsafe/native closure |
|---|---|---|
| `dctx-ed25519 0.1.0` | Apache-2.0 in its checked-in Cargo.toml | First-party safe Rust, forbid unsafe, no_std; **empty production dependency graph**. Variable-time public verification only; no signing/key generation. |
| `dctx-hash 0.1.0` | MIT in its checked-in Cargo.toml | First-party SHA-256, empty production dependency graph; do not infer project-wide Apache-2.0 from design prose over the manifest. |

Both need their dedicated vector/property/differential evidence before use at
the release boundary. A dossier does not upgrade code review into a crypto audit.

## Dev oracles and release-only signer candidate

The verifier's dev declaration is `ed25519-dalek = "=2.2.0"`, defaults disabled,
feature `std`; SHA-512 oracle is `sha2 = "=0.10.9"`. Known upstream licences are
respectively BSD-3-Clause and MIT OR Apache-2.0. Those exact sources/licence files
were not independently inspected here. A release-only dalek signer would use a
separate reviewed feature set (including key-zeroization policy) in key manager
tooling; dev approval is not release-signing approval.

Observed lock graph anchors, with versions fixed in the current repository:

| Package | Relevant recorded edges / risk |
|---|---|
| ed25519-dalek 2.2.0 | curve25519-dalek 4.1.3, ed25519 2.2.3, serde 1.0.228, sha2 0.10.9, subtle 2.6.1, zeroize 1.9.1 |
| curve25519-dalek 4.1.3 | cfg-if 1.0.5, cpufeatures 0.2.17, curve25519-dalek-derive 0.1.1, digest 0.10.7, fiat-crypto 0.2.9, rustc_version 0.4.1, subtle 2.6.1, zeroize 1.9.1 |
| sha2 0.10.9 | cfg-if 1.0.5, cpufeatures 0.2.17, digest 0.10.7 |
| digest 0.10.7 | block-buffer 0.10.4, crypto-common 0.1.7; both reach generic-array 0.14.7; generic-array reaches typenum 1.20.1 and version_check 0.9.5 |
| cpufeatures 0.2.17 | libc 0.2.190; target-dependent CPU probing/unsafe |
| ed25519 2.2.3 | signature 2.2.0, pkcs8 0.10.2 in the workspace lock feature union; minimal runtime feature reachability not established |
| curve25519-dalek-derive 0.1.1 | proc-macro2, quote and syn 2.0.119; build-host proc macro closure must be inventoried separately |

Known licence families: curve25519-dalek is BSD-3-Clause; its derive crate,
ed25519/signature/pkcs8, sha2/digest/crypto-common/block-buffer, zeroize, cfg-if,
cpufeatures and libc generally use MIT OR Apache-2.0; subtle uses BSD-3-Clause;
fiat-crypto uses MIT OR Apache-2.0 OR BSD-1-Clause. Treat these as candidates for
exact-pin legal verification, not a complete SPDX report. Every remaining graph
edge, licence text and feature must be recorded before signer approval.

The closure is **not unsafe-free**: optimized curve/SHA CPU backends may use
unsafe intrinsics/assembly; cpufeatures and libc reach native APIs; zeroize
uses low-level operations; build proc macros and their own dependencies matter.
No complete target/feature-specific unsafe census was performed. Do not promise
that disabling defaults makes the selected backend free of unsafe code.
First-party tooling must still expose only safe calls and forbid its own unsafe.

There is no audit report for the chosen signer/version/feature closure in the
named repository inputs. Known project audit history is not evidence for this
exact build. Real signing remains blocked until the report, audited revision,
version delta and remediation review are attached. The private key remains in
the key manager, and all selected signing dependencies remain outside runtime dctx.

## HTTPS candidates: exact direct pins, unresolved full closure

| Exact candidate | Known upstream licence; evidence | Unsafe/native boundary and gate |
|---|---|---|
| rustls 0.23.23 | Apache-2.0 OR ISC OR MIT; knowledge only | Safe TLS API; crypto provider/webpki/pki types and feature graph must be frozen. Does not itself establish the provider's safety or an HTTP fetch policy. |
| ring 0.17.14 | ISC, MIT and OpenSSL-family licences across source components; knowledge only | Unsafe Rust, C and assembly, native build tooling and target entropy APIs. Preserve component licence notices; not a single Apache-only dependency. |
| webpki-roots 0.26.8 | MPL-2.0; knowledge only | Embedded trust-anchor data with pki-types dependency; root provenance/date and update policy matter. No OS trust-store lookup intended. |
| reqwest 0.13.5 | MIT OR Apache-2.0; version observed in Cargo.lock | Alternative HTTP client candidate; lock entry lists hyper/tokio/url/tower etc., but no qualified plugin TLS feature closure. Defaults/provider and connection binding must be reviewed. |

Rustls/ring/root versions above are **research pins, not security endorsements**.
No registry sources, current advisory databases, audit reports or transitive
resolution were obtained under this offline packet. In particular, rustls-webpki,
rustls-pki-types, ring build deps (`cc` and target support), entropy deps and
HTTP client's full graph have no accepted exact resolution here. Do not fill
those slots by guessing versions or treating a transitive range as an exact pin.
The unknown full closure blocks TLS implementation approval, while this dossier
provides the concrete candidate and behavioral tests for that approval packet.

Selecting OS TLS instead would introduce a different closure: Security.framework/
platform trust on macOS, Schannel/Windows trust on Windows, usually OpenSSL or
another native backend on Linux. Native-tls is not “std-only” or automatically
unsafe-free. No OS TLS wrapper/version is selected here, and no system curl
fallback is authorized. An audited Rust TLS stack still has native/assembly
and OS dependencies when its provider does.

## Closure record needed for promotion

For each target, freeze package version/checksum, features, SPDX expression and
actual licence notices; normal/build/dev graph; Rust/SDK/compiler/linker and
native versions; unsafe/FFI/assembly inventory; source/audit/advisory date; runtime
DLL/loader/root data inventory; and the matching artifact digest. Reject
unapproved graphs, don't silently broaden features to make a build work.
Use authorized remote validation in the owning packet only. PL-R neither
resolves dependencies nor edits Cargo.toml/Cargo.lock nor performs signing.
