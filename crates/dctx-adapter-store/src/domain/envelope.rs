//! 80-byte header and 64-byte directory shared by manifest bytes.
//! Implements SEGMENTS-v1 envelope rules for magic `DCTXMAN`. Segment files stay in `dctx-map-segment`.
//! Depends on [`super::bytes`] and `dctx_map_segment::envelope_checksum`. Manifest encode calls [`assemble`].
//! Payload checksums and the envelope checksum both have to match before a generation is visible.

use super::bytes::{self, sha256};
use super::error::StoreError;

pub(crate) const REQUIRED: u32 = 1;
const MAX_FILE: usize = 64 * 1024 * 1024;

pub(crate) struct Section {
    pub kind: u16,
    pub flags: u32,
    pub record_count: u64,
    pub payload: Vec<u8>,
}

pub(crate) fn assemble(magic: &[u8; 8], sections: &[Section]) -> Result<Vec<u8>, StoreError> {
    if sections.len() > 64 {
        return Err(StoreError::InvalidRequest { reason: "sections" });
    }
    for pair in sections.windows(2) {
        if pair[0].kind >= pair[1].kind {
            return Err(StoreError::InvalidRequest { reason: "sections" });
        }
    }
    let count = u32::try_from(sections.len()).map_err(|_| StoreError::InvalidRequest { reason: "sections" })?;
    let dir = usize::try_from(count).map_err(|_| StoreError::InvalidRequest { reason: "sections" })? * 64;
    let mut total = 80usize.checked_add(dir).ok_or(StoreError::InvalidRequest { reason: "size" })?;
    for section in sections {
        total = total
            .checked_add(section.payload.len())
            .ok_or(StoreError::InvalidRequest { reason: "size" })?;
    }
    if total > MAX_FILE {
        return Err(StoreError::InvalidRequest { reason: "size" });
    }
    let mut out = vec![0u8; total];
    out[..8].copy_from_slice(magic);
    out[8..10].copy_from_slice(&1u16.to_le_bytes());
    out[12..16].copy_from_slice(&80u32.to_le_bytes());
    let file_len = u64::try_from(total).map_err(|_| StoreError::InvalidRequest { reason: "size" })?;
    out[16..24].copy_from_slice(&file_len.to_le_bytes());
    out[24..28].copy_from_slice(&count.to_le_bytes());
    out[28..32].copy_from_slice(&64u32.to_le_bytes());
    out[32..40].copy_from_slice(&80u64.to_le_bytes());
    let mut offset = 80u64 + u64::from(count) * 64;
    for (index, section) in sections.iter().enumerate() {
        let at = 80 + index * 64;
        let hash = sha256(&section.payload)?;
        let length = u64::try_from(section.payload.len()).map_err(|_| StoreError::InvalidRequest {
            reason: "size",
        })?;
        out[at..at + 2].copy_from_slice(&section.kind.to_le_bytes());
        out[at + 2..at + 4].copy_from_slice(&1u16.to_le_bytes());
        out[at + 4..at + 8].copy_from_slice(&section.flags.to_le_bytes());
        out[at + 8..at + 16].copy_from_slice(&offset.to_le_bytes());
        out[at + 16..at + 24].copy_from_slice(&length.to_le_bytes());
        out[at + 24..at + 32].copy_from_slice(&section.record_count.to_le_bytes());
        out[at + 32..at + 64].copy_from_slice(&hash);
        let start = usize::try_from(offset).map_err(|_| StoreError::InvalidRequest { reason: "size" })?;
        out[start..start + section.payload.len()].copy_from_slice(&section.payload);
        offset += length;
    }
    let sum = dctx_map_segment::envelope_checksum(&out).map_err(|_| StoreError::CorruptIndex {
        reason: "checksum",
    })?;
    out[48..80].copy_from_slice(&sum);
    Ok(out)
}

pub(crate) fn parse(magic: &[u8; 8], bytes: &[u8]) -> Result<Vec<Section>, StoreError> {
    if bytes.len() > MAX_FILE {
        return Err(StoreError::CorruptIndex { reason: "limit" });
    }
    if bytes.len() < 80 {
        return Err(StoreError::CorruptIndex { reason: "short" });
    }
    if bytes[..8] != magic[..] {
        return Err(StoreError::CorruptIndex { reason: "magic" });
    }
    let major = bytes::read_u16(bytes, 8)?;
    if major != 1 {
        return Err(StoreError::UnsupportedVersion { found: major });
    }
    if bytes::read_u32(bytes, 12)? != 80 || bytes::read_u32(bytes, 28)? != 64 {
        return Err(StoreError::CorruptIndex { reason: "header" });
    }
    let file_len = bytes::read_u64(bytes, 16)?;
    if file_len != u64::try_from(bytes.len()).unwrap_or(u64::MAX) {
        return Err(StoreError::CorruptIndex { reason: "length" });
    }
    let count = bytes::read_u32(bytes, 24)?;
    if count > 64 || bytes::read_u64(bytes, 32)? != 80 || bytes::read_u64(bytes, 40)? != 0 {
        return Err(StoreError::CorruptIndex { reason: "header" });
    }
    let stored = bytes::read_array(bytes, 48)?;
    let actual = dctx_map_segment::envelope_checksum(bytes).map_err(|_| StoreError::CorruptIndex {
        reason: "checksum",
    })?;
    if stored != actual {
        return Err(StoreError::CorruptIndex { reason: "checksum" });
    }
    let dir_end = 80u64 + u64::from(count) * 64;
    if dir_end > file_len {
        return Err(StoreError::CorruptIndex { reason: "directory" });
    }
    let mut sections = Vec::new();
    let mut previous_kind = 0u16;
    let mut previous_end = dir_end;
    for index in 0..count {
        let at = 80 + usize::try_from(index).map_err(|_| StoreError::CorruptIndex { reason: "directory" })? * 64;
        let kind = bytes::read_u16(bytes, at)?;
        let schema = bytes::read_u16(bytes, at + 2)?;
        let flags = bytes::read_u32(bytes, at + 4)?;
        let offset = bytes::read_u64(bytes, at + 8)?;
        let length = bytes::read_u64(bytes, at + 16)?;
        let record_count = bytes::read_u64(bytes, at + 24)?;
        let checksum = bytes::read_array(bytes, at + 32)?;
        if kind == 0 || kind <= previous_kind || schema != 1 || flags & !REQUIRED != 0 {
            return Err(StoreError::CorruptIndex { reason: "directory" });
        }
        let end = offset.checked_add(length).ok_or(StoreError::CorruptIndex { reason: "directory" })?;
        if offset != previous_end || end > file_len {
            return Err(StoreError::CorruptIndex { reason: "directory" });
        }
        let start = usize::try_from(offset).map_err(|_| StoreError::CorruptIndex { reason: "directory" })?;
        let len = usize::try_from(length).map_err(|_| StoreError::CorruptIndex { reason: "directory" })?;
        let payload = bytes
            .get(start..start + len)
            .ok_or(StoreError::CorruptIndex { reason: "directory" })?
            .to_vec();
        if sha256(&payload)? != checksum {
            return Err(StoreError::CorruptIndex { reason: "checksum" });
        }
        previous_kind = kind;
        previous_end = end;
        sections.push(Section {
            kind,
            flags,
            record_count,
            payload,
        });
    }
    if previous_end != file_len {
        return Err(StoreError::CorruptIndex { reason: "directory" });
    }
    Ok(sections)
}
