// SPDX-License-Identifier: Apache-2.0
//! Bounded byte reader and writer for plugin section values.
//! Implements contracts/DTS-v2.md plugin section payloads.
//! Depends on template size limits and typed corruption errors; used by the binary plugin section codec.

use crate::error::{TemplateError, MAX_STRING_BYTES};

pub(super) fn count(len: usize) -> Result<u32, TemplateError> {
    u32::try_from(len).map_err(|_| TemplateError::Limit { what: "count", limit: 64 })
}

#[derive(Default)]
pub(super) struct Buf(pub(super) Vec<u8>);

impl Buf {
    pub(super) fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    pub(super) fn u16(&mut self, value: u16) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    pub(super) fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    pub(super) fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    pub(super) fn raw(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    pub(super) fn str(&mut self, text: &str) -> Result<(), TemplateError> {
        if text.len() > MAX_STRING_BYTES {
            return Err(TemplateError::Limit { what: "string", limit: MAX_STRING_BYTES as u64 });
        }
        self.u32(u32::try_from(text.len()).map_err(|_| TemplateError::Limit {
            what: "string",
            limit: MAX_STRING_BYTES as u64,
        })?);
        self.raw(text.as_bytes());
        Ok(())
    }

    pub(super) fn strs(&mut self, items: &[String]) -> Result<(), TemplateError> {
        self.u32(count(items.len())?);
        for item in items {
            self.str(item)?;
        }
        Ok(())
    }
}

pub(super) struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    pub(super) fn take(&mut self, len: usize) -> Result<&'a [u8], TemplateError> {
        let end = self.at.checked_add(len).ok_or(TemplateError::Corrupt { reason: "truncated" })?;
        if end > self.data.len() {
            return Err(TemplateError::Corrupt { reason: "truncated" });
        }
        let out = &self.data[self.at..end];
        self.at = end;
        Ok(out)
    }

    pub(super) fn u8(&mut self) -> Result<u8, TemplateError> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn u16(&mut self) -> Result<u16, TemplateError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub(super) fn u32(&mut self) -> Result<u32, TemplateError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub(super) fn u64(&mut self) -> Result<u64, TemplateError> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]]))
    }

    pub(super) fn string(&mut self) -> Result<String, TemplateError> {
        let len = usize::try_from(self.u32()?).unwrap_or(usize::MAX);
        if len > MAX_STRING_BYTES {
            return Err(TemplateError::Limit { what: "string", limit: MAX_STRING_BYTES as u64 });
        }
        let bytes = self.take(len)?;
        std::str::from_utf8(bytes).map(str::to_string).map_err(|_| TemplateError::Corrupt { reason: "utf8" })
    }

    pub(super) fn list_len(&mut self) -> Result<usize, TemplateError> {
        let len = usize::try_from(self.u32()?).unwrap_or(usize::MAX);
        if len > 64 {
            return Err(TemplateError::Limit { what: "list", limit: 64 });
        }
        Ok(len)
    }

    pub(super) fn string_list(&mut self) -> Result<Vec<String>, TemplateError> {
        let len = self.list_len()?;
        let mut items = Vec::with_capacity(len);
        for _ in 0..len {
            items.push(self.string()?);
        }
        Ok(items)
    }

    pub(super) fn finish(&self) -> Result<(), TemplateError> {
        if self.at == self.data.len() {
            Ok(())
        } else {
            Err(TemplateError::Corrupt { reason: "payload" })
        }
    }
}

