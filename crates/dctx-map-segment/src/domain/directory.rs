// SPDX-License-Identifier: Apache-2.0
//! Bounded section directory. Entries are 64 bytes, unique, sorted, and packed without gaps.
//! Implements SEGMENTS-v1 directory rules and the fact/view role section sets.
//! Depends on the header constants, checksums, and [`super::kinds::SegmentRole`]. The assembler writes files.
//! Payload checksums are compared by the decoder after this structural pass.

use super::checksum::sha256;
use super::cursor::{put, read_array, read_u16, read_u32, read_u64};
use super::error::SegmentError;
use super::header::{ENTRY_LEN, HEADER_LEN};
use super::kinds::{SectionKind, SegmentRole};
use super::limits::{DecoderLimits, offset_of};

pub(crate) const REQUIRED: u32 = 1;

#[derive(Debug)]
pub(crate) struct Entry {
    pub kind: u16,
    pub schema: u16,
    pub flags: u32,
    pub offset: u64,
    pub length: u64,
    pub record_count: u64,
    pub checksum: [u8; 32],
}

pub(crate) struct BuiltSection {
    pub kind: u16,
    pub schema: u16,
    pub flags: u32,
    pub record_count: u64,
    pub payload: Vec<u8>,
}

pub(crate) fn parse(
    bytes: &[u8],
    file_len: u64,
    section_count: u32,
    limits: &DecoderLimits,
) -> Result<Vec<Entry>, SegmentError> {
    let dir_bytes = u64::from(section_count)
        .checked_mul(u64::from(ENTRY_LEN))
        .ok_or(SegmentError::Arithmetic)?;
    let dir_end = u64::from(HEADER_LEN)
        .checked_add(dir_bytes)
        .ok_or(SegmentError::Arithmetic)?;
    if dir_end > file_len {
        return Err(SegmentError::Truncated {
            at: file_len,
            need: dir_end,
        });
    }
    let mut entries = Vec::new();
    let mut previous_kind = 0u16;
    let mut previous_end = dir_end;
    for index in 0..section_count {
        let at = 80 + usize::try_from(index).map_err(|_| SegmentError::Arithmetic)? * 64;
        let kind = read_u16(bytes, at)?;
        let schema = read_u16(bytes, at + 2)?;
        let flags = read_u32(bytes, at + 4)?;
        let offset = read_u64(bytes, at + 8)?;
        let length = read_u64(bytes, at + 16)?;
        let record_count = read_u64(bytes, at + 24)?;
        let checksum = read_array::<32>(bytes, at + 32)?;
        if kind == 0 || kind <= previous_kind {
            return Err(SegmentError::Directory(if kind == 0 {
                "kind"
            } else {
                "order"
            }));
        }
        if flags & !REQUIRED != 0 {
            return Err(SegmentError::Directory("flag"));
        }
        if record_count > limits.max_records {
            return Err(SegmentError::Limit("records"));
        }
        if length == 0 && record_count != 0 {
            return Err(SegmentError::Cardinality("count"));
        }
        let end = offset
            .checked_add(length)
            .ok_or(SegmentError::OffsetOverflow)?;
        if offset < previous_end || end > file_len {
            return Err(SegmentError::OffsetOverflow);
        }
        if offset != previous_end {
            return Err(SegmentError::Directory("gap"));
        }
        previous_kind = kind;
        previous_end = end;
        entries.push(Entry {
            kind,
            schema,
            flags,
            offset,
            length,
            record_count,
            checksum,
        });
    }
    if previous_end != file_len {
        return Err(SegmentError::Directory("gap"));
    }
    Ok(entries)
}

pub(crate) fn check_role(entries: &[Entry], role: SegmentRole) -> Result<(), SegmentError> {
    for entry in entries {
        if let Some(kind) = SectionKind::from_code(entry.kind) {
            if entry.schema != 1 {
                return Err(SegmentError::UnknownSchema {
                    kind: entry.kind,
                    schema: entry.schema,
                });
            }
            if role.forbids(kind) {
                return Err(SegmentError::Role("forbidden"));
            }
            let required = role.requires(kind);
            if required != (entry.flags & REQUIRED == REQUIRED) {
                return Err(SegmentError::Role("flag"));
            }
            if !required && !role.optional(kind) {
                return Err(SegmentError::Role("forbidden"));
            }
        } else if entry.flags & REQUIRED == REQUIRED {
            return Err(SegmentError::UnknownSection { kind: entry.kind });
        }
    }
    for kind in SectionKind::ALL {
        if role.requires(kind) && !entries.iter().any(|entry| entry.kind == kind.code()) {
            return Err(SegmentError::Role("missing"));
        }
    }
    Ok(())
}

pub(crate) fn payload<'a>(bytes: &'a [u8], entry: &Entry) -> Result<&'a [u8], SegmentError> {
    let start = usize::try_from(entry.offset).map_err(|_| SegmentError::OffsetOverflow)?;
    let len = usize::try_from(entry.length).map_err(|_| SegmentError::OffsetOverflow)?;
    let end = start.checked_add(len).ok_or(SegmentError::OffsetOverflow)?;
    bytes.get(start..end).ok_or(SegmentError::OffsetOverflow)
}

pub(crate) fn assemble(
    sections: &[BuiltSection],
    limits: &DecoderLimits,
) -> Result<Vec<u8>, SegmentError> {
    if sections.len() > usize::try_from(limits.max_sections).unwrap_or(usize::MAX) {
        return Err(SegmentError::Limit("sections"));
    }
    for pair in sections.windows(2) {
        if pair[0].kind >= pair[1].kind {
            return Err(SegmentError::Directory("order"));
        }
    }
    let count = u32::try_from(sections.len()).map_err(|_| SegmentError::Arithmetic)?;
    let dir_bytes = usize::try_from(u64::from(count) * 64).map_err(|_| SegmentError::Arithmetic)?;
    let mut total = 80usize
        .checked_add(dir_bytes)
        .ok_or(SegmentError::Arithmetic)?;
    for section in sections {
        total = total
            .checked_add(section.payload.len())
            .ok_or(SegmentError::Arithmetic)?;
    }
    let file_len = offset_of(total);
    if file_len > limits.max_segment_bytes {
        return Err(SegmentError::Limit("segment"));
    }
    let mut out = vec![0u8; total];
    put(&mut out, 0, &super::header::MAGIC)?;
    put(&mut out, 8, &1u16.to_le_bytes())?;
    put(&mut out, 12, &80u32.to_le_bytes())?;
    put(&mut out, 16, &file_len.to_le_bytes())?;
    put(&mut out, 24, &count.to_le_bytes())?;
    put(&mut out, 28, &64u32.to_le_bytes())?;
    put(&mut out, 32, &80u64.to_le_bytes())?;
    let mut offset = 80u64 + u64::from(count) * 64;
    for (index, section) in sections.iter().enumerate() {
        let at = 80 + index * 64;
        let hash = sha256(&section.payload)?;
        let length = offset_of(section.payload.len());
        put(&mut out, at, &section.kind.to_le_bytes())?;
        put(&mut out, at + 2, &section.schema.to_le_bytes())?;
        put(&mut out, at + 4, &section.flags.to_le_bytes())?;
        put(&mut out, at + 8, &offset.to_le_bytes())?;
        put(&mut out, at + 16, &length.to_le_bytes())?;
        put(&mut out, at + 24, &section.record_count.to_le_bytes())?;
        put(&mut out, at + 32, &hash)?;
        let start = usize::try_from(offset).map_err(|_| SegmentError::Arithmetic)?;
        put(&mut out, start, &section.payload)?;
        offset = offset.saturating_add(length);
    }
    let sum = super::checksum::envelope_checksum(&out)?;
    put(&mut out, 48, &sum)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_past_the_file_is_overflow_and_a_gap_is_refused() {
        let limits = DecoderLimits::standard();
        let mut bytes = vec![0u8; 144];
        bytes[24..28].copy_from_slice(&1u32.to_le_bytes());
        bytes[80..82].copy_from_slice(&1u16.to_le_bytes());
        bytes[84..88].copy_from_slice(&1u32.to_le_bytes());
        bytes[88..96].copy_from_slice(&1000u64.to_le_bytes());
        bytes[96..104].copy_from_slice(&1u64.to_le_bytes());
        assert_eq!(
            parse(&bytes, 144, 1, &limits).unwrap_err(),
            SegmentError::OffsetOverflow
        );
        bytes[88..96].copy_from_slice(&u64::MAX.to_le_bytes());
        bytes[96..104].copy_from_slice(&2u64.to_le_bytes());
        assert_eq!(
            parse(&bytes, 144, 1, &limits).unwrap_err(),
            SegmentError::OffsetOverflow
        );
        bytes[88..96].copy_from_slice(&150u64.to_le_bytes());
        bytes[96..104].copy_from_slice(&10u64.to_le_bytes());
        assert_eq!(
            parse(&bytes, 160, 1, &limits).unwrap_err(),
            SegmentError::Directory("gap")
        );
    }
}
