// SPDX-License-Identifier: Apache-2.0
//! Canonical unsigned base-128 integers, at most 10 bytes for a `u64`.
//! Implements SEGMENTS-v1: reject overflow, overlong encodings, and unfinished sequences.
//! Depends on [`super::error::SegmentError`]. Postings use it for document and position deltas.
//! Zero is a single `0x00` byte. A continued final group of zero is overlong, not a zero delta.

use super::error::SegmentError;

/// Appends the shortest base-128 encoding of `value`.
pub(crate) fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = u8::try_from(value & 0x7f).unwrap_or(0);
        value >>= 7;
        if value == 0 {
            out.push(byte);
            break;
        }
        byte |= 0x80;
        out.push(byte);
    }
}

/// Reads one canonical varint. The `usize` is the number of bytes consumed.
#[allow(
    clippy::verbose_bit_mask,
    reason = "The 0x7f mask names the seven payload bits of a varint byte and matches the encoder."
)]
pub(crate) fn read_varint(data: &[u8]) -> Result<(u64, usize), SegmentError> {
    let mut result = 0u64;
    let mut shift = 0;
    for (index, byte) in data.iter().copied().enumerate() {
        if index >= 10 {
            return Err(SegmentError::Varint("overflow"));
        }
        let piece = u64::from(byte & 0x7f);
        if shift == 63 && (piece > 1 || byte & 0x80 != 0) {
            return Err(SegmentError::Varint("overflow"));
        }
        result |= piece << shift;
        if byte & 0x80 == 0 {
            if index > 0 && byte & 0x7f == 0 {
                return Err(SegmentError::Varint("overlong"));
            }
            return Ok((result, index + 1));
        }
        shift += 7;
    }
    Err(SegmentError::Varint("truncated"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_values_round_trip_and_overlong_zeros_are_refused() {
        for value in [0u64, 1, 127, 128, 255, 300, u64::MAX] {
            let mut bytes = Vec::new();
            push_varint(&mut bytes, value);
            assert!(bytes.len() <= 10);
            assert_eq!(read_varint(&bytes).unwrap(), (value, bytes.len()));
        }
        assert_eq!(read_varint(&[0x00]).unwrap(), (0, 1));
        assert_eq!(read_varint(&[0xac, 0x02]).unwrap(), (300, 2));
        assert_eq!(
            read_varint(&[0x80, 0x00]).unwrap_err(),
            SegmentError::Varint("overlong")
        );
        assert_eq!(
            read_varint(&[0x80]).unwrap_err(),
            SegmentError::Varint("truncated")
        );
        assert_eq!(
            read_varint(&[0x80; 10]).unwrap_err(),
            SegmentError::Varint("overflow")
        );
    }
}
