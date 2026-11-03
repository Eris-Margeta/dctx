// SPDX-License-Identifier: Apache-2.0
//! The 80-byte segment header. Magic, versions, lengths and the envelope checksum slot.
//! Implements SEGMENTS-v1 "Encoding envelope". Checksum comparison happens after the directory.
//! Depends on [`super::cursor`] and [`super::limits`]. [`super::decode`] calls [`parse`] first.
//! Major must be 1. Any minor of major 1 is accepted. Fixed fields stay at their v1 values.

use super::cursor::{read_array, read_u16, read_u32, read_u64};
use super::error::SegmentError;
use super::limits::{offset_of, DecoderLimits};

pub(crate) const MAGIC: [u8; 8] = *b"DCTXSEG\0";
pub(crate) const MAJOR: u16 = 1;
pub(crate) const HEADER_LEN: u32 = 80;
pub(crate) const ENTRY_LEN: u32 = 64;
pub(crate) const DIR_OFFSET: u64 = 80;

#[derive(Debug)]
pub(crate) struct Header {
    pub minor: u16,
    pub file_len: u64,
    pub section_count: u32,
    pub checksum: [u8; 32],
}

pub(crate) fn parse(bytes: &[u8], limits: &DecoderLimits) -> Result<Header, SegmentError> {
    let len = offset_of(bytes.len());
    if len > limits.max_segment_bytes {
        return Err(SegmentError::Limit("segment"));
    }
    if bytes.len() < 80 {
        return Err(SegmentError::Truncated { at: len, need: 80 });
    }
    if read_array::<8>(bytes, 0)? != MAGIC {
        return Err(SegmentError::BadMagic);
    }
    let major = read_u16(bytes, 8)?;
    let minor = read_u16(bytes, 10)?;
    if major != MAJOR {
        return Err(SegmentError::UnknownVersion { major, minor });
    }
    if read_u32(bytes, 12)? != HEADER_LEN {
        return Err(SegmentError::BadHeader("length"));
    }
    let file_len = read_u64(bytes, 16)?;
    if file_len > limits.max_segment_bytes {
        return Err(SegmentError::Limit("segment"));
    }
    if len < file_len {
        return Err(SegmentError::Truncated { at: len, need: file_len });
    }
    if len > file_len {
        return Err(SegmentError::TrailingBytes);
    }
    let section_count = read_u32(bytes, 24)?;
    if section_count > limits.max_sections {
        return Err(SegmentError::Limit("sections"));
    }
    if read_u32(bytes, 28)? != ENTRY_LEN {
        return Err(SegmentError::BadHeader("width"));
    }
    if read_u64(bytes, 32)? != DIR_OFFSET {
        return Err(SegmentError::BadHeader("offset"));
    }
    let features = read_u64(bytes, 40)?;
    if features != 0 {
        return Err(SegmentError::UnknownFeature(features));
    }
    let checksum = read_array::<32>(bytes, 48)?;
    Ok(Header {
        minor,
        file_len,
        section_count,
        checksum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(major: u16, file_len: u64, features: u64) -> Vec<u8> {
        let mut bytes = vec![0u8; usize::try_from(file_len).unwrap_or(80)];
        bytes[..8].copy_from_slice(&MAGIC);
        bytes[8..10].copy_from_slice(&major.to_le_bytes());
        bytes[12..16].copy_from_slice(&HEADER_LEN.to_le_bytes());
        bytes[16..24].copy_from_slice(&file_len.to_le_bytes());
        bytes[28..32].copy_from_slice(&ENTRY_LEN.to_le_bytes());
        bytes[32..40].copy_from_slice(&DIR_OFFSET.to_le_bytes());
        bytes[40..48].copy_from_slice(&features.to_le_bytes());
        bytes
    }

    #[test]
    fn major_one_parses_and_unknown_major_is_refused() {
        let limits = DecoderLimits::standard();
        let ok = header(1, 80, 0);
        assert_eq!(parse(&ok, &limits).unwrap().section_count, 0);
        assert_eq!(
            parse(&header(2, 80, 0), &limits).unwrap_err(),
            SegmentError::UnknownVersion { major: 2, minor: 0 }
        );
        assert_eq!(
            parse(&header(1, 80, 1), &limits).unwrap_err(),
            SegmentError::UnknownFeature(1)
        );
        assert_eq!(
            parse(&[0u8; 10], &limits).unwrap_err(),
            SegmentError::Truncated { at: 10, need: 80 }
        );
        let mut bad_magic = header(1, 80, 0);
        bad_magic[0] = b'X';
        assert_eq!(parse(&bad_magic, &limits).unwrap_err(), SegmentError::BadMagic);
        let mut long = header(1, 80, 0);
        long.push(0);
        assert_eq!(parse(&long, &limits).unwrap_err(), SegmentError::TrailingBytes);
        let mut short = header(1, 100, 0);
        short.truncate(80);
        assert_eq!(
            parse(&short, &limits).unwrap_err(),
            SegmentError::Truncated { at: 80, need: 100 }
        );
    }
}
