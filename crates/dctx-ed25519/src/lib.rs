// SPDX-License-Identifier: Apache-2.0
//! Ed25519 verification for dctx plugin trust (decision 081, decision 087, PLUGINS §5.2).
//! Curve arithmetic is audited `ed25519-dalek` 2.2.0 (`VerifyingKey::verify_strict`).
//! Safe Rust, `forbid(unsafe_code)`, no signing or key generation in the public API.
//! See the crate README for the acceptance policy and the Wycheproof gate.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod compress;
mod sha512;

pub use sha512::Sha512;

use ed25519_dalek::{Signature, VerifyingKey};

/// Little-endian subgroup order `L`.
///
/// `ed25519-dalek` reports a non-canonical `S` and an equation failure as the
/// same `SignatureError` when built without `std`. This range check runs first
/// so [`VerificationError::NonCanonicalScalar`] stays distinct. `verify_strict`
/// repeats the check before it accepts.
const GROUP_ORDER: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
];

/// Reason an Ed25519 signature could not be verified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerificationError {
    /// A public key must contain exactly 32 bytes.
    PublicKeyLength,
    /// A detached signature must contain exactly 64 bytes.
    SignatureLength,
    /// The public key is not a canonically encoded Edwards25519 point.
    PublicKeyEncoding,
    /// The public key has order dividing eight, including the identity.
    SmallOrderPublicKey,
    /// R is not a canonically encoded Edwards25519 point.
    SignaturePointEncoding,
    /// R has order dividing eight, including the identity.
    SmallOrderSignaturePoint,
    /// S is not in the range 0 <= S < L.
    NonCanonicalScalar,
    /// The cofactorless verification equation does not hold.
    EquationMismatch,
}

impl core::fmt::Display for VerificationError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let message = match self {
            Self::PublicKeyLength => "Ed25519 public key must be 32 bytes",
            Self::SignatureLength => "Ed25519 signature must be 64 bytes",
            Self::PublicKeyEncoding => "non-canonical or invalid Ed25519 public key point",
            Self::SmallOrderPublicKey => "small-order Ed25519 public key",
            Self::SignaturePointEncoding => "non-canonical or invalid Ed25519 signature point",
            Self::SmallOrderSignaturePoint => "small-order Ed25519 signature point",
            Self::NonCanonicalScalar => "non-canonical Ed25519 signature scalar",
            Self::EquationMismatch => "Ed25519 verification equation failed",
        };
        formatter.write_str(message)
    }
}

impl core::error::Error for VerificationError {}

fn scalar_is_canonical(scalar: &[u8; 32]) -> bool {
    // Little-endian integer compare, high byte first. Equal to L is non-canonical.
    for (byte, limit) in scalar.iter().zip(GROUP_ORDER).rev() {
        match byte.cmp(&limit) {
            core::cmp::Ordering::Less => return true,
            core::cmp::Ordering::Greater => return false,
            core::cmp::Ordering::Equal => {}
        }
    }
    false
}

/// ZIP-215 `from_bytes` accepts a non-canonical `y` and a sign bit on `x = 0`.
/// RFC 8032 canonical points are exactly those that recompress to the same bytes.
fn check_point(
    bytes: &[u8; 32],
    encoding: VerificationError,
    small_order: VerificationError,
) -> Result<VerifyingKey, VerificationError> {
    let key = VerifyingKey::from_bytes(bytes).map_err(|_| encoding)?;
    if key.to_edwards().compress().to_bytes() != *bytes {
        return Err(encoding);
    }
    if key.is_weak() {
        return Err(small_order);
    }
    Ok(key)
}

/// Verifies a detached, pure Ed25519 signature over the exact message bytes.
///
/// Keys and signatures must have their exact lengths and canonical encodings.
/// Small-order public keys and R points are rejected. The equation is
/// [`VerifyingKey::verify_strict`]: `[S]B = R + [k]A` with
/// `k = SHA-512(R || A || message) mod L`. It does not multiply by eight.
/// Mixed-order points are governed by that equation, not a prime-subgroup test.
///
/// The implementation is variable-time on public inputs. It supports neither
/// Ed25519ctx nor Ed25519ph, signing, key generation or batch verification.
///
/// # Errors
/// Returns [`VerificationError`] for malformed inputs or a failed signature.
pub fn verify(
    public_key: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Result<(), VerificationError> {
    let public_key: &[u8; 32] = public_key
        .try_into()
        .map_err(|_| VerificationError::PublicKeyLength)?;
    let signature: &[u8; 64] = signature
        .try_into()
        .map_err(|_| VerificationError::SignatureLength)?;
    let mut r_bytes = [0u8; 32];
    let mut scalar = [0u8; 32];
    r_bytes.copy_from_slice(&signature[..32]);
    scalar.copy_from_slice(&signature[32..]);
    if !scalar_is_canonical(&scalar) {
        return Err(VerificationError::NonCanonicalScalar);
    }
    let key = check_point(
        public_key,
        VerificationError::PublicKeyEncoding,
        VerificationError::SmallOrderPublicKey,
    )?;
    check_point(
        &r_bytes,
        VerificationError::SignaturePointEncoding,
        VerificationError::SmallOrderSignaturePoint,
    )?;
    key.verify_strict(message, &Signature::from_bytes(signature))
        .map_err(|_| VerificationError::EquationMismatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_vector() -> ([u8; 32], [u8; 64]) {
        let key = hex::decode("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
            .unwrap()
            .try_into()
            .unwrap();
        let signature = hex::decode(concat!(
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155",
            "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
        ))
        .unwrap()
        .try_into()
        .unwrap();
        (key, signature)
    }

    #[test]
    fn every_error_has_a_valid_control() {
        let (key, signature) = first_vector();
        assert_eq!(verify(&key, b"", &signature), Ok(()));
        assert_eq!(
            verify(&key[..31], b"", &signature),
            Err(VerificationError::PublicKeyLength)
        );
        assert_eq!(
            verify(&key, b"", &signature[..63]),
            Err(VerificationError::SignatureLength)
        );
        assert_eq!(
            verify(&[0xff; 32], b"", &signature),
            Err(VerificationError::PublicKeyEncoding)
        );
        assert_eq!(
            verify(&[0; 32], b"", &signature),
            Err(VerificationError::SmallOrderPublicKey)
        );
        let mut changed = signature;
        changed[..32].fill(0xff);
        assert_eq!(
            verify(&key, b"", &changed),
            Err(VerificationError::SignaturePointEncoding)
        );
        changed[..32].fill(0);
        assert_eq!(
            verify(&key, b"", &changed),
            Err(VerificationError::SmallOrderSignaturePoint)
        );
        changed = signature;
        changed[32..].copy_from_slice(&GROUP_ORDER);
        assert_eq!(
            verify(&key, b"", &changed),
            Err(VerificationError::NonCanonicalScalar)
        );
        assert_eq!(
            verify(&key, b"x", &signature),
            Err(VerificationError::EquationMismatch)
        );
    }
}
