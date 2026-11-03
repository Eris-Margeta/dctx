// SPDX-License-Identifier: Apache-2.0
//! SHA-256 envelopes and the identity-v1 `SegmentObject` digest.
//! Implements SEGMENTS-v1 checksums and IDENTITY-v1 structured kind 9.
//! Depends on `dctx-hash` and [`super::error::SegmentError`]. The directory stores payload digests.
//! The envelope hash covers the file with bytes 48..80 forced to zero. It is not authentication.

use super::error::SegmentError;

/// SHA-256 of `bytes`. A segment under the size cap cannot hit the hasher length limit.
pub(crate) fn sha256(bytes: &[u8]) -> Result<[u8; 32], SegmentError> {
    let digest =
        dctx_hash::Sha256::digest(bytes).map_err(|_| SegmentError::Limit("checksum input"))?;
    Ok(*digest.as_bytes())
}

fn absorb(hasher: &mut dctx_hash::Sha256, data: &[u8]) -> Result<(), SegmentError> {
    hasher
        .update(data)
        .map_err(|_| SegmentError::Limit("checksum input"))
}

/// Envelope checksum: SHA-256 of `bytes` with the stored checksum field treated as zeros.
///
/// # Errors
/// Refuses an envelope shorter than 80 bytes or an input beyond the SHA-256 length limit.
pub fn envelope_checksum(bytes: &[u8]) -> Result<[u8; 32], SegmentError> {
    if bytes.len() < 80 {
        return Err(SegmentError::Truncated {
            at: super::limits::offset_of(bytes.len()),
            need: 80,
        });
    }
    let head = bytes.get(..48).ok_or(SegmentError::Truncated { at: 0, need: 48 })?;
    let tail = bytes.get(80..).ok_or(SegmentError::Truncated { at: 80, need: 0 })?;
    let mut hasher = dctx_hash::Sha256::new();
    absorb(&mut hasher, head)?;
    absorb(&mut hasher, &[0u8; 32])?;
    absorb(&mut hasher, tail)?;
    Ok(*hasher.finalize().as_bytes())
}

/// Identity v1 kind 9 over the final envelope bytes. Distinct from [`envelope_checksum`].
///
/// # Errors
/// Refuses an unrepresentable input length or a structured input beyond the SHA-256 length limit.
pub fn segment_object_digest(envelope: &[u8]) -> Result<[u8; 32], SegmentError> {
    let mut hasher = dctx_hash::Sha256::new();
    absorb(&mut hasher, b"DCTX\0")?;
    absorb(&mut hasher, &9u16.to_le_bytes())?;
    absorb(&mut hasher, &1u16.to_le_bytes())?;
    let len = u64::try_from(envelope.len()).map_err(|_| SegmentError::OffsetOverflow)?;
    absorb(&mut hasher, &len.to_le_bytes())?;
    absorb(&mut hasher, envelope)?;
    Ok(*hasher.finalize().as_bytes())
}

pub(crate) fn check_envelope(bytes: &[u8], stored: &[u8; 32]) -> Result<(), SegmentError> {
    let actual = envelope_checksum(bytes)?;
    if actual != *stored {
        return Err(SegmentError::BadEnvelopeChecksum);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_payload_is_the_nist_empty_digest() {
        let digest = sha256(&[]).unwrap();
        let mut expected = [0u8; 32];
        for (index, byte) in b"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            .chunks(2)
            .enumerate()
        {
            expected[index] = u8::from_str_radix(std::str::from_utf8(byte).unwrap(), 16).unwrap();
        }
        assert_eq!(digest, expected);
        assert_eq!(dctx_hash::Sha256::digest(b"").unwrap().as_bytes(), &digest);
    }

    #[test]
    fn segment_object_digest_is_stable_and_distinct_from_the_raw_hash() {
        let bytes = b"DCTXSEG segment";
        let once = segment_object_digest(bytes).unwrap();
        assert_eq!(segment_object_digest(bytes).unwrap(), once);
        assert_ne!(sha256(bytes).unwrap(), once);
        assert_ne!(segment_object_digest(b"other").unwrap(), once);
    }
}
