# dctx-ed25519

Safe Rust **verification only** for pure Ed25519 signatures. Curve arithmetic is the
audited `ed25519-dalek` **2.2.0** crate (`VerifyingKey::verify_strict`), for the maintainer
decision 087 / decision 081 and `docs/design/PLUGINS.md` §5.2. The dependency is pinned with
`default-features = false` and no extra features: verification does not need `std`,
`alloc`, `fast`, `zeroize`, batching, or signing. No key generation, secret-key types,
Git behavior, or platform effects are exposed. Production builds of this crate are
`no_std` and `forbid(unsafe_code)`. The public `Sha512` type remains a first-party
digest. Tests sign with dalek's `SigningKey`; that type is not part of this crate's API.

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
2. Both `A` and `R` must be canonically encoded Edwards25519 points. dalek's
   `from_bytes` follows ZIP-215 and will reduce a non-canonical `y`. This crate
   recompresses the point and requires the original bytes back, so `y >= p` and
   `x = 0` with sign bit one are rejected (RFC 8032 §5.1.3).
3. Require `0 <= S < L`, where
   `L = 2^252 + 27742317777372353535851937790883648493`. Never reduce input `S`.
   `verify_strict` repeats this canonical-scalar check.
4. Reject **all eight small-order points** as either `A` or `R`, including the
   identity: `[8]P = identity`. `VerifyingKey::is_weak` classifies `A` and `R`.
   Rejecting small-order `R` matches dalek's `verify_strict`.
5. Acceptance is `verify_strict` returning success. It checks the **cofactorless**
   equation `[S]B = R + [k]A` with `k = little_endian(SHA-512(R || A || message)) mod L`,
   using the original canonical encoding bytes in the hash. It does not multiply
   the equation by eight.
6. Mixed-order points are not rejected solely for having a torsion component.
   They must pass the unmultiplied equation with the reduced challenge. This
   matches dalek strict verification rather than imposing an extra `[L]P = 0`
   check. It is not ZIP-215's permissive/cofactored acceptance policy.

There is no Ed25519ctx, Ed25519ph, batch verification or secret arithmetic API.
The exported `Sha512` streaming/digest API is ordinary SHA-512, not Ed25519ph.
Arithmetic and comparisons can branch on public inputs; **no constant-time
claim** is made. Signature validation does not prove publisher trust.

## What this crate checks

| ID | Invariant | Evidence |
|---|---|---|
| I1 | Acceptance is `ed25519-dalek` 2.2.0 `verify_strict`: canonical `S`, non-small-order `A` and `R`, and the cofactorless equation. | RFC 8032 §7.1, the full Wycheproof corpus, cofactorless fixtures, signed-message properties. |
| I2 | A point encoding dalek would reduce must still recompress to the same bytes. | Non-canonical coordinate and negative-zero fixtures. |
| I3 | `S` is below `L` before `verify_strict`, so malleable scalars keep a distinct error. | Order-boundary and malleability fixtures. |
| I4 | The public SHA-512 is chunk-invariant and encodes the 128-bit bit length in big endian. Messages must be shorter than 2^128 bits, as in FIPS 180-4; overflow panics before modifying the state. | NIST known answers including a million `a` bytes, padding splits, length carry, maximum/overflow counter tests, seeded sha2 differential tests. |
| I5 | Each refusal path has a distinct [`VerificationError`](src/lib.rs) and a valid control. | Crate unit test plus the policy fixtures. |

Field, scalar, and point arithmetic live in `ed25519-dalek`. This crate keeps
length checks, the canonical range and recompress checks, the public error enum,
and the public SHA-512 API. Verification work is one strict dalek verification,
linear in the message length.

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
| `TinkOverflow` | 32 | Accept all published valid arithmetic-boundary signatures. |
| `InvalidKtv` | 1 | Reject the known invalid signature. |
| `InvalidSignature` | 20 | Reject invalid equations and edge-case scalars/points; zero scalars alone are not automatically invalid. |
| `InvalidEncoding` | 22 | Reject non-canonical point/scalar encodings, impossible roots and negative-zero encodings; no lenient decoding. |
| `SignatureMalleability` | 8 | Reject S >= L, including equivalent scalars formed by adding L. |
| `SignatureWithGarbage` | 5 | Reject extra signature bytes even when the first 64 bytes verify. |
| `CompressedSignature` | 4 | Reject shortened/overlapping R and S representations. |
| `TruncatedSignature` | 3 | Reject every signature shorter than 64 bytes. |

Small-order keys/R and mixed-order/cofactor behavior are also covered explicitly
in `tests/policy.rs`, since the ten corpus flags do not independently promise
complete coverage of those policies. Canonical encoding is stricter than dalek's
ZIP-215 key decoder. A signature dalek's `verify_strict` rejects is never accepted.
No reference mismatch in the test corpus is waived.

## Release gate and validation

**Any disagreement with a published expected result or with `verify_strict`
fails the suite and blocks release.** Tests never change expectations to agree
with this wrapper, skip invalid cases or quarantine mismatches.
`sha2 = 0.10.9` is the dev-only oracle for the public SHA-512 API.

The suite includes all five RFC 8032 §7.1 vectors (including the 1023-byte message
and pure Ed25519 signing of SHA-512(`abc`)), all Wycheproof cases, small-order and
cofactorless fixtures, 128-case proptest checks of seeded signed messages/keys,
seeded single-bit mutations of each input, arbitrary encodings/lengths and SHA-512
chunking. A deterministic sample additionally checks **every** bit of its key,
signature and message. Proptest uses a fixed ChaCha seed and writes no regression
files; reported failing seeds/cases must become reviewed fixtures before release.

`tests/n1113_hostile.rs` covers empty and one-byte inputs, every small-sample
truncation, exact and overlong key/signature sizes, invalid UTF-8, duplicate and
reordered byte sequences, repeated calls, concurrent verification and cloned
hash states. Hashes are checked against `sha2`. Private counter tests exercise
the SHA-512 length limit without allocating an enormous message. Error display
messages include a next step and formatting failures propagate to the caller.

Run the combined verification/hash gate twice with the same results:

```text
cargo test -p dctx-ed25519 -p dctx-hash
cargo clippy -p dctx-ed25519 -p dctx-hash --all-targets -- -D warnings
cargo fmt -p dctx-ed25519 -p dctx-hash -- --check
```

Passing these checks is reproducible test evidence, not an independent
cryptographic audit. Release review remains responsible for the dalek pin
and the plugin trust boundary.
