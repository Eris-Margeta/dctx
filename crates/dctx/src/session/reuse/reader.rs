// SPDX-License-Identifier: Apache-2.0
use super::{MAX_STRING, records_cap};
use crate::domain::DctxError;
use crate::index::{TextBlob, TextSpan};

pub(super) struct Reader<'a> {
    pub(super) rest: &'a [u8],
    pub(super) strings: Vec<&'a str>,
}

impl<'a> Reader<'a> {
    pub(super) fn take(&mut self, len: usize) -> Result<&'a [u8], DctxError> {
        if self.rest.len() < len {
            return Err(DctxError::CorruptIndex {
                reason: "generation truncated",
            });
        }
        let (head, tail) = self.rest.split_at(len);
        self.rest = tail;
        Ok(head)
    }

    pub(super) fn u8(&mut self) -> Result<u8, DctxError> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn boolean(&mut self) -> Result<bool, DctxError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DctxError::CorruptIndex {
                reason: "generation boolean",
            }),
        }
    }

    pub(super) fn u32(&mut self) -> Result<u32, DctxError> {
        let bytes = self.take(4)?;
        let raw: [u8; 4] = bytes.try_into().map_err(|_| DctxError::CorruptIndex {
            reason: "generation truncated",
        })?;
        Ok(u32::from_le_bytes(raw))
    }

    pub(super) fn u64(&mut self) -> Result<u64, DctxError> {
        let bytes = self.take(8)?;
        let raw: [u8; 8] = bytes.try_into().map_err(|_| DctxError::CorruptIndex {
            reason: "generation truncated",
        })?;
        Ok(u64::from_le_bytes(raw))
    }

    pub(super) fn u128(&mut self) -> Result<u128, DctxError> {
        let bytes = self.take(16)?;
        let raw: [u8; 16] = bytes.try_into().map_err(|_| DctxError::CorruptIndex {
            reason: "generation truncated",
        })?;
        Ok(u128::from_le_bytes(raw))
    }

    pub(super) fn load_strings(&mut self) -> Result<(), DctxError> {
        let count = usize::try_from(self.u64()?).map_err(|_| DctxError::CorruptIndex {
            reason: "generation strings",
        })?;
        // Every entry needs a four-byte length. Bound allocation by both input and record limits.
        if count > self.rest.len() / 4 || count > records_cap().saturating_mul(10).saturating_add(8)
        {
            return Err(DctxError::CorruptIndex {
                reason: "generation strings",
            });
        }
        for _ in 0..count {
            let value = self.raw_string()?;
            self.strings.push(value);
        }
        Ok(())
    }

    pub(super) fn string(&mut self) -> Result<String, DctxError> {
        let id = self.u32()? as usize;
        self.strings
            .get(id)
            .map(|value| (*value).to_string())
            .ok_or(DctxError::CorruptIndex {
                reason: "generation string reference",
            })
    }

    /// Materializes the validated table in one owned buffer. Each node field
    /// shares this buffer and a range; no field borrows the retained byte slice.
    pub(super) fn node_texts(&self) -> crate::index::NodeTextTable {
        let mut buffer = String::with_capacity(self.strings.iter().map(|s| s.len()).sum());
        let mut ranges = Vec::with_capacity(self.strings.len());
        for value in &self.strings {
            let start = buffer.len();
            buffer.push_str(value);
            ranges.push(start..buffer.len());
        }
        crate::index::NodeTextTable::new(buffer, ranges)
    }

    pub(super) fn node_text(
        &mut self,
        texts: &crate::index::NodeTextTable,
    ) -> Result<crate::index::NodeText, DctxError> {
        let id = self.u32()? as usize;
        texts.get(id).ok_or(DctxError::CorruptIndex {
            reason: "generation string reference",
        })
    }

    /// Intern paths and labels directly into the generation blob. Repeated references
    /// share a span; no temporary owned string is allocated per node or edge.
    pub(super) fn span(
        &mut self,
        text: &mut TextBlob,
        spans: &mut [Option<TextSpan>],
    ) -> Result<TextSpan, DctxError> {
        let id = self.u32()? as usize;
        let value = self.strings.get(id).ok_or(DctxError::CorruptIndex {
            reason: "generation string reference",
        })?;
        let slot = spans.get_mut(id).ok_or(DctxError::CorruptIndex {
            reason: "generation string reference",
        })?;
        Ok(*slot.get_or_insert_with(|| text.push(value)))
    }

    fn raw_string(&mut self) -> Result<&'a str, DctxError> {
        let len = usize::try_from(self.u32()?).map_err(|_| DctxError::CorruptIndex {
            reason: "generation string",
        })?;
        if len > MAX_STRING {
            return Err(DctxError::CorruptIndex {
                reason: "generation string",
            });
        }
        let bytes = self.take(len)?;
        let text = std::str::from_utf8(bytes).map_err(|_| DctxError::CorruptIndex {
            reason: "generation string",
        })?;
        Ok(text)
    }

    pub(super) fn count(&mut self, record_bytes: usize) -> Result<usize, DctxError> {
        let count = usize::try_from(self.u64()?).map_err(|_| DctxError::CorruptIndex {
            reason: "generation count",
        })?;
        if count > records_cap() || count > self.rest.len() / record_bytes {
            return Err(DctxError::CorruptIndex {
                reason: "generation count",
            });
        }
        Ok(count)
    }
}
