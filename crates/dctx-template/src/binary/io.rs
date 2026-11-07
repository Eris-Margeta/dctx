// SPDX-License-Identifier: Apache-2.0
//! Bounded little-endian byte primitives and section framing reader.
//! Implements contracts/DTS-v2.md compiled IR and IDENTITY-v1.
//! Depends on typed template errors and binary size limits; used by the `ContextPlan` binary codec.

use crate::error::{MAX_PLAN_BYTES, MAX_STRING_BYTES, TemplateError};

pub(super) fn count(len: usize) -> Result<u32, TemplateError> {
    u32::try_from(len).map_err(|_| TemplateError::Limit {
        what: "count",
        limit: 100_000,
    })
}

pub(super) fn read_section<'a>(
    reader: &mut Reader<'a>,
) -> Result<(u16, u16, &'a [u8]), TemplateError> {
    let kind = reader.u16()?;
    let schema = reader.u16()?;
    let len = usize::try_from(reader.u32()?).unwrap_or(usize::MAX);
    if len > usize::try_from(MAX_PLAN_BYTES).unwrap_or(usize::MAX) {
        return Err(TemplateError::Limit {
            what: "section",
            limit: u64::from(MAX_PLAN_BYTES),
        });
    }
    Ok((kind, schema, reader.take(len)?))
}

pub(super) struct Buf(pub(super) Vec<u8>);

impl Buf {
    pub(super) fn new() -> Self {
        Self(Vec::new())
    }

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
            return Err(TemplateError::Limit {
                what: "string",
                limit: MAX_STRING_BYTES as u64,
            });
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

    pub(super) fn rest(&self) -> usize {
        self.data.len().saturating_sub(self.at)
    }

    pub(super) fn take(&mut self, len: usize) -> Result<&'a [u8], TemplateError> {
        let end = self.at.checked_add(len).ok_or(TemplateError::Corrupt {
            reason: "truncated",
        })?;
        if end > self.data.len() {
            return Err(TemplateError::Corrupt {
                reason: "truncated",
            });
        }
        let out = &self.data[self.at..end];
        self.at = end;
        Ok(out)
    }

    pub(super) fn u8(&mut self) -> Result<u8, TemplateError> {
        let bytes = self.take(1)?;
        Ok(bytes[0])
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
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    pub(super) fn string(&mut self) -> Result<String, TemplateError> {
        let len = usize::try_from(self.u32()?).unwrap_or(usize::MAX);
        if len > MAX_STRING_BYTES {
            return Err(TemplateError::Limit {
                what: "string",
                limit: MAX_STRING_BYTES as u64,
            });
        }
        let bytes = self.take(len)?;
        std::str::from_utf8(bytes)
            .map(str::to_string)
            .map_err(|_| TemplateError::Corrupt { reason: "utf8" })
    }

    pub(super) fn string_list(&mut self) -> Result<Vec<String>, TemplateError> {
        let n = self.count()?;
        let mut items = Vec::with_capacity(n);
        for _ in 0..n {
            items.push(self.string()?);
        }
        Ok(items)
    }

    pub(super) fn count(&mut self) -> Result<usize, TemplateError> {
        let n = usize::try_from(self.u32()?).unwrap_or(usize::MAX);
        if n > 100_000 {
            return Err(TemplateError::Limit {
                what: "count",
                limit: 100_000,
            });
        }
        Ok(n)
    }

    pub(super) fn finish(&self, reason: &'static str) -> Result<(), TemplateError> {
        if self.rest() == 0 {
            Ok(())
        } else {
            Err(TemplateError::Corrupt { reason })
        }
    }
}
