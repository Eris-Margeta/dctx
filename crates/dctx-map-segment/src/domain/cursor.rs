// SPDX-License-Identifier: Apache-2.0
//! Little-endian cursor and writer. Every read checks the remaining length first.
//! Implements SEGMENTS-v1: explicit little-endian integers, no native layout and no unchecked casts.
//! Depends on [`super::error::SegmentError`] and [`super::limits::offset_of`]. Section codecs use it.
//! A short read is [`SegmentError::Truncated`]. Indexing below is only used after `get` succeeds.

use super::error::SegmentError;
use super::limits::offset_of;

/// Borrowed reader. `base` is the file offset of `data[0]`, used in truncation errors.
pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
    base: u64,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(data: &'a [u8], base: u64) -> Self {
        Self { data, at: 0, base }
    }

    pub(crate) fn rest(&self) -> &'a [u8] {
        self.data.get(self.at..).unwrap_or(&[])
    }

    fn here(&self) -> u64 {
        self.base.saturating_add(offset_of(self.at))
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], SegmentError> {
        let end = self.at.checked_add(n).ok_or(SegmentError::OffsetOverflow)?;
        let out = self.data.get(self.at..end).ok_or(SegmentError::Truncated {
            at: self.here(),
            need: offset_of(n),
        })?;
        self.at = end;
        Ok(out)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, SegmentError> {
        match self.take(1)?.first().copied() {
            Some(value) => Ok(value),
            None => Err(SegmentError::Arithmetic),
        }
    }

    pub(crate) fn u16(&mut self) -> Result<u16, SegmentError> {
        let bytes = self.take(2)?;
        let array: [u8; 2] = bytes.try_into().map_err(|_| SegmentError::Arithmetic)?;
        Ok(u16::from_le_bytes(array))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, SegmentError> {
        let bytes = self.take(4)?;
        let array: [u8; 4] = bytes.try_into().map_err(|_| SegmentError::Arithmetic)?;
        Ok(u32::from_le_bytes(array))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, SegmentError> {
        let bytes = self.take(8)?;
        let array: [u8; 8] = bytes.try_into().map_err(|_| SegmentError::Arithmetic)?;
        Ok(u64::from_le_bytes(array))
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], SegmentError> {
        let bytes = self.take(N)?;
        bytes.try_into().map_err(|_| SegmentError::Arithmetic)
    }

    pub(crate) fn flag(&mut self) -> Result<bool, SegmentError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(SegmentError::Cardinality("flag")),
        }
    }
}

pub(crate) fn read_u16(bytes: &[u8], at: usize) -> Result<u16, SegmentError> {
    let slot = slice_at(bytes, at, 2)?;
    let array: [u8; 2] = slot.try_into().map_err(|_| SegmentError::Arithmetic)?;
    Ok(u16::from_le_bytes(array))
}

pub(crate) fn read_u32(bytes: &[u8], at: usize) -> Result<u32, SegmentError> {
    let slot = slice_at(bytes, at, 4)?;
    let array: [u8; 4] = slot.try_into().map_err(|_| SegmentError::Arithmetic)?;
    Ok(u32::from_le_bytes(array))
}

pub(crate) fn read_u64(bytes: &[u8], at: usize) -> Result<u64, SegmentError> {
    let slot = slice_at(bytes, at, 8)?;
    let array: [u8; 8] = slot.try_into().map_err(|_| SegmentError::Arithmetic)?;
    Ok(u64::from_le_bytes(array))
}

pub(crate) fn read_array<const N: usize>(bytes: &[u8], at: usize) -> Result<[u8; N], SegmentError> {
    let slot = slice_at(bytes, at, N)?;
    slot.try_into().map_err(|_| SegmentError::Arithmetic)
}

fn slice_at(bytes: &[u8], at: usize, len: usize) -> Result<&[u8], SegmentError> {
    let end = at.checked_add(len).ok_or(SegmentError::OffsetOverflow)?;
    bytes.get(at..end).ok_or(SegmentError::Truncated {
        at: offset_of(at),
        need: offset_of(len),
    })
}

pub(crate) fn put(out: &mut [u8], at: usize, bytes: &[u8]) -> Result<(), SegmentError> {
    let end = at.checked_add(bytes.len()).ok_or(SegmentError::OffsetOverflow)?;
    let slot = out.get_mut(at..end).ok_or(SegmentError::OffsetOverflow)?;
    slot.copy_from_slice(bytes);
    Ok(())
}

pub(crate) fn push_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

pub(crate) fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_bytes(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(value);
}

pub(crate) fn push_bool(out: &mut Vec<u8>, value: bool) {
    out.push(u8::from(value));
}

pub(crate) fn push_narrow(out: &mut Vec<u8>, code: u16) -> Result<(), SegmentError> {
    let byte = u8::try_from(code).map_err(|_| SegmentError::Arithmetic)?;
    out.push(byte);
    Ok(())
}

pub(crate) fn push_record(out: &mut Vec<u8>, payload: &[u8], max_record: u32) -> Result<(), SegmentError> {
    let len = u32::try_from(payload.len()).map_err(|_| SegmentError::Limit("record"))?;
    if len > max_record {
        return Err(SegmentError::Limit("record"));
    }
    push_u32(out, len);
    out.extend_from_slice(payload);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_reads_are_truncated_and_integers_are_little_endian() {
        let mut cursor = Cursor::new(&[1, 2, 3, 4], 10);
        assert_eq!(cursor.u16().unwrap(), 0x0201);
        assert_eq!(
            cursor.take(3).unwrap_err(),
            SegmentError::Truncated { at: 12, need: 3 }
        );
        assert_eq!(read_u32(&[0, 0, 0, 1], 0).unwrap(), 0x0100_0000);
        let mut out = Vec::new();
        push_u64(&mut out, 0x0102);
        assert_eq!(out, [0x02, 0x01, 0, 0, 0, 0, 0, 0]);
    }
}
