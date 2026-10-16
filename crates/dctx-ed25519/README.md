# dctx-ed25519

First-party, safe Rust **verification only** for pure Ed25519 signatures. Implements
decision 081 option B for `docs/design/PLUGINS.md` §5.2; decision 081 supersedes that design's
earlier proposed signature dependency. No production dependencies, allocation, IO,
signing, key generation, secret-key types, Git behavior or platform effects.
Production builds are `no_std` and forbid unsafe code. Synthetic signing appears
only in tests, through the approved dev-only `ed25519-dalek` dependency.

```rust
use dctx_ed25519::{verify, VerificationError};

fn authenticate(key: &[u8], exact_manifest_envelope: &[u8], signature: &[u8])
    -> Result<(), VerificationError>
{
    verify(key, exact_manifest_envelope, signature)
}
```

The caller supplies trusted publisher keys and constructs the exact signed bytes,
including the `dctx-plugin-manifest-v1` domain separator required by the plugin
design. This crate adds no prefix, reserialization or prehash. Trust, revocation,
rollback, manifest parsing and OS confinement remain host responsibilities.
It does not enable plugins or relax decision 080's confinement requirements.

## Verification policy

1. Public keys are exactly 32 bytes; signatures are exactly 64 bytes (`R || S`).
   No suffixes, truncation, overlapping fields or implicit zero padding.
2. Both `A` and `R` must decode as Edwards25519 points. The encoded `y` must be
   less than `p = 2^255 - 19`; it is never reduced on input. A square root must
   exist, and `x = 0` with sign bit one is rejected (RFC 8032 §5.1.3).
3. Require `0 <= S < L`, where
   `L = 2^252 + 27742317777372353535851937790883648493`. Never reduce input `S`.
4. Reject **all eight small-order points** as either `A` or `R`, including the
   identity: `[8]P = identity`. Rejecting small-order `R` is an intentional
   additional strict policy, matching dalek's `verify_strict`.
5. Compute `k = little_endian(SHA-512(R || A || message)) mod L`, following
   RFC 8032's reference implementation and dalek. Use the original canonical
   encoding bytes in the hash. Verify the **cofactorless** equation
   `[S]B = R + [k]A`, permitted by RFC 8032 §5.1.7. Do not multiply the equation
   by eight. The small-order check uses eight only to classify points.
6. Mixed-order points are not rejected solely for having a torsion component.
   They must pass the unmultiplied equation with the reduced challenge. This
   matches dalek strict verification rather than imposing an extra `[L]P = 0`
   check. It is not ZIP-215's permissive/cofactored acceptance policy.

There is no Ed25519ctx, Ed25519ph, batch verification or secret arithmetic API.
The exported `Sha512` streaming/digest API is ordinary SHA-512, not Ed25519ph.
Arithmetic and comparisons can branch on public inputs; **no constant-time
claim** is made. Signature validation does not prove publisher trust.

## Arithmetic invariants

| ID | Invariant | Evidence |
|---|---|---|
| I1 | Field values are canonical five-limb radix-2^51 integers modulo p. Each product sum is below 2^109, fitting u128. Three carry passes and one canonical subtraction suffice. | Field boundary/constants and randomized algebra tests; signature reference tests. |
| I2 | Extended points maintain `XY = ZT` and `-X² + Y² = Z² + dT²`, with nonzero Z. Complete addition/doubling implement RFC 8032 §5.1.4. | Randomized coordinate-invariant tests, base/order/inverse tests, all torsion fixtures, RFC/Wycheproof and dalek differentials. |
| I3 | Scalar reduction keeps a remainder below L; doubling plus one stays below 2L < 2^254. Signature S is checked before arithmetic. | Order/wide-reduction boundaries and malleability fixtures. |
| I4 | SHA-512 is chunk-invariant and encodes the 128-bit bit length in big endian. Messages must be shorter than 2^128 bits, as in FIPS 180-4. | NIST known answers including a million `a` bytes, padding splits, length carry, random sha2 differential tests. |
| I5 | Acceptance requires canonical inputs, non-small-order A/R and the cofactorless equation. | Exact corpus results, adversarial fixtures, signed-message and mutation properties. |

Private modules separate field, scalar, point, compression and streaming-hash
rules; `lib.rs` exposes only verification, typed errors and SHA-512.
Storage is constant-size; verification work is linear in message length plus
two fixed-width scalar multiplications. These are straightforward variable-time
algorithms intended for reviewability, without precomputed tables or assembly.

## Wycheproof edge-class policy

The **complete, unmodified** pinned `testvectors_v1/ed25519_test.json` is embedded
under `tests/data`. All 151 cases run: **88 valid, 63 invalid, zero skipped**.
Tests assert its SHA-256, IDs, counts, known flags, published result and independent
dalek strict result. Unknown result labels (including a future `acceptable`) or
new edge classes fail until explicitly reviewed; they are never silently mapped
to acceptance or omitted.

| Edge class | Cases | Policy |
|---|---:|---|
| `Valid` | 18 | Accept each ordinary valid signature. |
| `Ktv` | 38 | Accept each published known valid signature. |
| `TinkOverflow` | 32 | Accept all published valid arithmetic-boundary signatures using bounded u128 field arithmetic. |
| `InvalidKtv` | 1 | Reject the known invalid signature. |
| `InvalidSignature` | 20 | Reject invalid equations and edge-case scalars/points; zero scalars alone are not automatically invalid. |
| `InvalidEncoding` | 22 | Reject non-canonical point/scalar encodings, impossible roots and negative-zero encodings; no lenient decoding. |
| `SignatureMalleability` | 8 | Reject S >= L, including equivalent scalars formed by adding L. |
| `SignatureWithGarbage` | 5 | Reject extra signature bytes even when the first 64 bytes verify. |
| `CompressedSignature` | 4 | Reject shortened/overlapping R and S representations. |
| `TruncatedSignature` | 3 | Reject every signature shorter than 64 bytes. |

Small-order keys/R and mixed-order/cofactor behavior are also covered explicitly
in `tests/policy.rs`, since the ten corpus flags do not independently promise
complete coverage of those policies. Canonical key decoding is explicitly
stricter than dalek's permissive key decoder; this cannot accept a dalek-rejected
signature. No reference mismatch in the test corpus is waived.

## Release gate and validation

**Any disagreement with a published expected result or reference implementation
fails the suite and blocks release.** Tests never change expectations to agree
with this implementation, skip invalid cases or quarantine mismatches.
`ed25519-dalek = 2.2.0` (strict verification) and `sha2 = 0.10.9` are test oracles,
not production dependencies. Their transitive dependencies are dev-only too.

The suite includes all five RFC 8032 §7.1 vectors (including the 1023-byte message
and pure Ed25519 signing of SHA-512(`abc`)), all Wycheproof cases, small-order and
cofactorless fixtures, 128-case proptest checks of random signed messages/keys,
random single-bit mutations of each input, arbitrary encodings/lengths and SHA-512
chunking. A deterministic sample additionally checks **every** bit of its key,
signature and message. Proptest writes no regression files; reported failing
seeds/cases must become reviewed fixtures before release.

Authorized validation runs only through the development-machine runner:

```text
cargo test -p dctx-ed25519
cargo clippy -p dctx-ed25519 -- -D warnings
```

Passing these checks is reproducible test evidence, not an independent
cryptographic audit. Release review remains responsible for the first-party
implementation and the plugin trust boundary.
