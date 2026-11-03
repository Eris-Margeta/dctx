// SPDX-License-Identifier: Apache-2.0
//! Current-generation pointer: `DCTXPTR` plus a manifest digest.
//! Implements SEGMENTS-v1 "Current pointer sketch". The rename of this file is the visibility point.
//! Depends on [`super::bytes::sha256`]. Recovery trusts a pointer only together with its closure.
//! A newer staged manifest is not a pointer. Checksums are raw SHA-256 of the first 60 bytes.

use super::bytes::{self, sha256};
use super::error::StoreError;

/// Encoded pointer length, in bytes.
pub(crate) const POINTER_LEN: u64 = 92;

pub(crate) const MAGIC: [u8; 8] = *b"DCTXPTR\0";

/// One committed pointer. The digest is a `ManifestObject`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pointer {
    pub generation: u64,
    pub digest: [u8; 32],
    pub length: u64,
}

pub(crate) fn encode_pointer(pointer: &Pointer) -> Result<Vec<u8>, StoreError> {
    let mut body = Vec::with_capacity(92);
    body.extend_from_slice(&MAGIC);
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&pointer.generation.to_le_bytes());
    body.extend_from_slice(&pointer.digest);
    body.extend_from_slice(&pointer.length.to_le_bytes());
    let sum = sha256(&body)?;
    body.extend_from_slice(&sum);
    Ok(body)
}

pub(crate) fn decode_pointer(bytes: &[u8]) -> Result<Pointer, StoreError> {
    if bytes.len() != 92 {
        return Err(StoreError::CorruptIndex { reason: "pointer" });
    }
    if bytes[..8] != MAGIC {
        return Err(StoreError::CorruptIndex { reason: "pointer" });
    }
    let major = bytes::read_u16(bytes, 8)?;
    if major != 1 {
        return Err(StoreError::UnsupportedVersion { found: major });
    }
    if bytes::read_u16(bytes, 10)? != 0 {
        return Err(StoreError::UnsupportedVersion { found: major });
    }
    let generation = bytes::read_u64(bytes, 12)?;
    let digest = bytes::read_array(bytes, 20)?;
    let length = bytes::read_u64(bytes, 52)?;
    let stored = bytes::read_array(bytes, 60)?;
    if sha256(&bytes[..60])? != stored || generation == 0 {
        return Err(StoreError::CorruptIndex { reason: "pointer" });
    }
    Ok(Pointer {
        generation,
        digest,
        length,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_round_trips_and_a_flipped_bit_is_corrupt() {
        let pointer = Pointer {
            generation: 3,
            digest: [9u8; 32],
            length: 80,
        };
        let bytes = encode_pointer(&pointer).unwrap();
        assert_eq!(bytes.len(), 92);
        assert_eq!(decode_pointer(&bytes).unwrap(), pointer);
        let mut bad = bytes.clone();
        bad[20] ^= 0xff;
        assert!(decode_pointer(&bad).is_err());
        assert!(decode_pointer(&bytes[..60]).is_err());
    }
}
