Historical design record; superseded by [the current documentation](../../../guides/PLUGINS.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# HTTPS download profile

PL-R · 2026-10-16 · PLUGINS §5.2–5.4 and PL-C distribution/S-INSTALL.
Design for PL-I's explicit download adapter; no network or TLS tests performed.
Depends on pinned trust/size policy, dependencies.md and race-safe activation.

## Disposition and candidate stack

Rust std does not implement TLS. The existing lock has `reqwest 0.13.5`, but
the observed entry alone does not establish a TLS-enabled client or an approved
plugin-distribution dependency closure. No HTTPS adapter is qualified here.
An unavailable adapter must fail refresh/download explicitly; cached inspection
and verified offline install can remain available with normal freshness rules.
Do not shell out to curl, Git, a browser or a package manager as a substitute.

Concrete qualification candidate: `rustls = 0.23.23` with defaults disabled and
explicit `std`, `ring` and (if required by public release origins) `tls12`;
`ring = 0.17.14` as provider; `webpki-roots = 0.26.8` as an explicitly versioned
root snapshot. These are existing-knowledge candidate pins, not the current
recommended security versions or resolved repository dependencies. They need
source/feature/licence/security approval before use. No `aws-lc-rs`, OpenSSL or
native-tls fallback is implied. See [dependencies.md](dependencies.md).

Rustls provides a safe Rust TLS interface; ring adds unsafe/native/assembly
implementation. Neither implements the whole fetch policy. PL-I must select
and approve an HTTP/URL layer or bounded first-party framing adapter separately.
`reqwest = 0.13.5` is an observed alternative HTTP-layer candidate, not a claim
that its resolved TLS provider matches these pins. A client that cannot bind
DNS validation to the actual connection cannot implement this profile.

For all five proposed native targets, a statically linked Rust TLS/provider
stack is structurally compatible with one dctx executable; native build inputs
and dynamic system runtime libraries remain inventoried. Platform trust-store
loading is a separate choice with OS/FFI dependencies. The initial proposal
uses embedded explicit roots to avoid credential-home/keychain discovery;
roots still need reviewed updates and a maintenance policy.

## Required connection policy

| Stage | Required behavior |
|---|---|
| URL | HTTPS only; reject userinfo, fragments, unsupported ports/schemes and ambiguous host/IP syntax; normalize before comparing origins. Artifact names resolve beside the selected manifest, as PL-C requires. |
| Destination | Policy validates every A/AAAA result and the actual connected address, including IPv4-mapped IPv6, loopback, private, link-local, multicast, unspecified and reserved ranges. Reject disallowed addresses unless an explicit install policy grants them. |
| Connection | Connect to the validated address without a second unchecked DNS lookup; use the original verified hostname for SNI, certificate name and HTTP Host. Restrict address retries to the validated set. |
| TLS | TLS 1.3 preferred; TLS 1.2 only with the selected provider's approved suites. Verify chain, hostname, validity period and signature algorithms against explicit roots. No accept-invalid-cert, insecure name override or plaintext fallback. |
| Redirect | Manual bounded redirects (proposed maximum 5); recheck scheme/origin/DNS/connected address at every hop. A new unapproved origin needs the existing install-policy decision, not automatic trust. |
| Authentication | No cookies, netrc, Git/browser credentials, client certificate discovery, keychain queries, bearer tokens or ambient proxy configuration. Public downloads only. An approved proxy needs its own destination-binding threat model. |
| HTTP | Bound status/header count/bytes and body; reject conflicting framing and unsupported transfer/content encodings. Hash precisely the artifact byte representation declared by the manifest. Disable transparent compression unless explicitly specified and bounded. |
| Limits | PL-C index default 4 MiB, artifact default 256 MiB, exact signed size plus configured total/temp-storage caps. Proposed transport header cap 64 KiB; manifest maximum 256 KiB, depth 32 and 4096 total members (PL-C S-MANIFEST); signature exactly 64 bytes. Stop on overflow even if Content-Length is absent or false. |
| Errors | Bounded typed failures; no response-body/URL credential leakage. No execution, extraction hooks, activation or trust changes after an incomplete transfer. |

Proposed transport-specific limits require PL-I configuration/contract alignment;
they do not revise frozen PL-C schema fields. Use injected clocks for certificate
and trust freshness tests. For real transfers, expose bytes/progress/budgets and
cancellation; bounded connection/read policies do not become a blanket plugin
liveness rule under decision 077. TLS certificate revocation behavior must be explicit:
ordinary rustls/webpki verification does not promise fresh online OCSP/CRL checks.
Do not fetch arbitrary certificate URLs implicitly. Publisher-key revocation
remains a separate signed-trust policy even with a valid TLS chain.

## Storage and runtime separation

Stream to a private non-executable staging object under the managed install
root, enforce length while hashing with dctx-hash, and authenticate the manifest
with dctx-ed25519. Close/durably publish only after every byte, declared runtime
resource and trust/target check succeeds. Preserve the original manifest bytes.
On interruption, leave inactive/recoverable state; do not install from partial
downloads. DCTXPLG1's payload table is not a substitute for the signed manifest.

Installer network permission never enters the worker grant. Do not inherit TLS
sockets, connection-pool handles, proxy descriptors or credential providers into
a worker. No startup fetch, auto-enable, daemon, scheduled refresh or installer
script is part of this design. GitHub metadata only locates public releases;
rate limiting or unavailable APIs produce errors, not credential discovery.

## Required tests and source pointers

PL-I tests need an explicit fake/local test transport grant in their own packet;
PL-R opens no test listener. Positive controls must cover valid certificate/name,
approved redirect and exact signed payload. Negatives: expired/untrusted/wrong-
host chain, HTTP downgrade, redirect to internal/metadata address, DNS rebinding,
IPv6/mapped address, injected proxy/env, oversized/chunked/slow stream, false
length, decompression expansion, changed TLS roots and interrupted activation.
Test signature failures despite valid TLS and TLS failures despite otherwise
valid signed bytes: neither replaces the other policy.

Reference locators (not fetched): [rustls](https://docs.rs/rustls/0.23.23/rustls/),
[ring](https://docs.rs/ring/0.17.14/ring/),
[webpki-roots](https://docs.rs/webpki-roots/0.26.8/webpki_roots/),
[TLS 1.3 RFC 8446](https://www.rfc-editor.org/rfc/rfc8446).
