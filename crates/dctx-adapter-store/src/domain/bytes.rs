// SPDX-License-Identifier: Apache-2.0
//! Little-endian reads and SHA-256 helpers for manifests and pointers.
//! Implements IDENTITY-v1 structured hashing (kind 10 is the manifest object).
//! Depends on `dctx-hash` and [`super::error::StoreError`]. Codecs in this crate call it.
//! A short buffer is [`StoreError::CorruptIndex`], never a panic.

use super::error::StoreError;

pub(crate) fn sha256(bytes: &[u8]) -> Result<[u8; 32], StoreError> {
    let digest = dctx_hash::Sha256::digest(bytes)
        .map_err(|_| StoreError::CorruptIndex { reason: "checksum" })?;
    Ok(*digest.as_bytes())
}

pub(crate) fn structured(kind: u16, payload: &[u8]) -> Result<[u8; 32], StoreError> {
    let mut hasher = dctx_hash::Sha256::new();
    let len = u64::try_from(payload.len())
        .map_err(|_| StoreError::CorruptIndex { reason: "checksum" })?;
    hasher
        .update(b"DCTX\0")
        .map_err(|_| StoreError::CorruptIndex { reason: "checksum" })?;
    hasher
        .update(&kind.to_le_bytes())
        .and_then(|()| hasher.update(&1u16.to_le_bytes()))
        .and_then(|()| hasher.update(&len.to_le_bytes()))
        .and_then(|()| hasher.update(payload))
        .map_err(|_| StoreError::CorruptIndex { reason: "checksum" })?;
    Ok(*hasher.finalize().as_bytes())
}

pub(crate) fn read_u16(bytes: &[u8], at: usize) -> Result<u16, StoreError> {
    let slot = bytes
        .get(at..at + 2)
        .ok_or(StoreError::CorruptIndex { reason: "short" })?;
    Ok(u16::from_le_bytes([slot[0], slot[1]]))
}

pub(crate) fn read_u32(bytes: &[u8], at: usize) -> Result<u32, StoreError> {
    let slot = bytes
        .get(at..at + 4)
        .ok_or(StoreError::CorruptIndex { reason: "short" })?;
    Ok(u32::from_le_bytes([slot[0], slot[1], slot[2], slot[3]]))
}

pub(crate) fn read_u64(bytes: &[u8], at: usize) -> Result<u64, StoreError> {
    let slot = bytes
        .get(at..at + 8)
        .ok_or(StoreError::CorruptIndex { reason: "short" })?;
    let mut raw = [0u8; 8];
    raw.copy_from_slice(slot);
    Ok(u64::from_le_bytes(raw))
}

pub(crate) fn read_array(bytes: &[u8], at: usize) -> Result<[u8; 32], StoreError> {
    let slot = bytes
        .get(at..at + 32)
        .ok_or(StoreError::CorruptIndex { reason: "short" })?;
    let mut raw = [0u8; 32];
    raw.copy_from_slice(slot);
    Ok(raw)
}

pub(crate) struct Cur<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cur<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub(crate) fn u8(&mut self) -> Result<u8, StoreError> {
        let byte = *self
            .bytes
            .get(self.at)
            .ok_or(StoreError::CorruptIndex { reason: "manifest" })?;
        self.at += 1;
        Ok(byte)
    }

    pub(crate) fn u16(&mut self) -> Result<u16, StoreError> {
        let value = read_u16(self.bytes, self.at)?;
        self.at += 2;
        Ok(value)
    }

    pub(crate) fn u32(&mut self) -> Result<u32, StoreError> {
        let value = read_u32(self.bytes, self.at)?;
        self.at += 4;
        Ok(value)
    }

    pub(crate) fn u64(&mut self) -> Result<u64, StoreError> {
        let value = read_u64(self.bytes, self.at)?;
        self.at += 8;
        Ok(value)
    }

    pub(crate) fn digest(&mut self) -> Result<[u8; 32], StoreError> {
        let value = read_array(self.bytes, self.at)?;
        self.at += 32;
        Ok(value)
    }

    pub(crate) fn text(&mut self) -> Result<String, StoreError> {
        let len = self.u32()?;
        let len =
            usize::try_from(len).map_err(|_| StoreError::CorruptIndex { reason: "manifest" })?;
        if len > 4096 {
            return Err(StoreError::CorruptIndex { reason: "manifest" });
        }
        let end = self
            .at
            .checked_add(len)
            .ok_or(StoreError::CorruptIndex { reason: "manifest" })?;
        let raw = self
            .bytes
            .get(self.at..end)
            .ok_or(StoreError::CorruptIndex { reason: "manifest" })?;
        if raw.contains(&0) {
            return Err(StoreError::CorruptIndex { reason: "manifest" });
        }
        let text = std::str::from_utf8(raw)
            .map_err(|_| StoreError::CorruptIndex { reason: "manifest" })?;
        self.at = end;
        Ok(text.to_string())
    }

    pub(crate) fn finish(self) -> Result<(), StoreError> {
        if self.at != self.bytes.len() {
            return Err(StoreError::CorruptIndex { reason: "manifest" });
        }
        Ok(())
    }
}

pub(crate) fn push_str(out: &mut Vec<u8>, text: &str) -> Result<(), StoreError> {
    if text.len() > 4096 || text.as_bytes().contains(&0) {
        return Err(StoreError::InvalidRequest { reason: "text" });
    }
    let len =
        u32::try_from(text.len()).map_err(|_| StoreError::InvalidRequest { reason: "text" })?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(text.as_bytes());
    Ok(())
}
