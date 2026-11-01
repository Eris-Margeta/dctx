use super::{MAX_COUNT, MAX_STRING};
use crate::domain::DctxError;

pub(super) struct Reader<'a> {
    pub(super) rest: &'a [u8],
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

    pub(super) fn string(&mut self) -> Result<String, DctxError> {
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
        Ok(text.to_string())
    }

    pub(super) fn count(&mut self) -> Result<usize, DctxError> {
        let count = usize::try_from(self.u64()?).map_err(|_| DctxError::CorruptIndex {
            reason: "generation count",
        })?;
        if count > MAX_COUNT {
            return Err(DctxError::CorruptIndex {
                reason: "generation count",
            });
        }
        Ok(count)
    }
}
