// SPDX-License-Identifier: Apache-2.0
//! Bounded scalar encoding and parsing for the ledger image.
//! Implements CACHE-v1 ledger counts, text limits and typed corruption refusals.
//! Depends on `CacheError`; restart and the domain map codecs use these primitives.

use crate::error::CacheError;

const COUNT_MAX: u32 = 100_000;
const TEXT_MAX: u32 = 4_096;

pub(super) fn corrupt(reason: &'static str) -> CacheError {
    CacheError::CorruptLedger { reason }
}

pub(super) struct Put {
    buf: Vec<u8>,
}

impl Put {
    pub(super) fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub(super) fn u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    pub(super) fn u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(super) fn u64(&mut self, value: u64) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(super) fn opt_u64(&mut self, value: Option<u64>) {
        match value {
            None => self.u8(0),
            Some(inner) => {
                self.u8(1);
                self.u64(inner);
            }
        }
    }

    pub(super) fn count(&mut self, len: usize) -> Result<(), CacheError> {
        let len = u32::try_from(len).map_err(|_| corrupt("ledger is too large"))?;
        if len > COUNT_MAX {
            return Err(corrupt("ledger is too large"));
        }
        self.u32(len);
        Ok(())
    }

    pub(super) fn text(&mut self, value: &str) -> Result<(), CacheError> {
        let len = u32::try_from(value.len()).map_err(|_| corrupt("ledger is too large"))?;
        if len > TEXT_MAX {
            return Err(corrupt("ledger is too large"));
        }
        self.u32(len);
        self.buf.extend_from_slice(value.as_bytes());
        Ok(())
    }

    pub(super) fn finish(self) -> Vec<u8> {
        self.buf
    }
}

pub(super) struct Take<'a> {
    rest: &'a [u8],
}

impl<'a> Take<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { rest: bytes }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], CacheError> {
        if self.rest.len() < len {
            return Err(corrupt("truncated ledger"));
        }
        let (head, tail) = self.rest.split_at(len);
        self.rest = tail;
        Ok(head)
    }

    pub(super) fn u8(&mut self) -> Result<u8, CacheError> {
        let bytes = self.take(1)?;
        Ok(bytes[0])
    }

    pub(super) fn u32(&mut self) -> Result<u32, CacheError> {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(buf))
    }

    pub(super) fn u64(&mut self) -> Result<u64, CacheError> {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(buf))
    }

    pub(super) fn opt_u64(&mut self) -> Result<Option<u64>, CacheError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err(corrupt("optional")),
        }
    }

    pub(super) fn count(&mut self) -> Result<usize, CacheError> {
        let len = self.u32()?;
        if len > COUNT_MAX {
            return Err(corrupt("ledger is too large"));
        }
        Ok(len as usize)
    }

    pub(super) fn u64s(&mut self) -> Result<Vec<u64>, CacheError> {
        let len = self.count()?;
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            values.push(self.u64()?);
        }
        Ok(values)
    }

    pub(super) fn text(&mut self) -> Result<String, CacheError> {
        let len = self.u32()?;
        if len > TEXT_MAX {
            return Err(corrupt("ledger is too large"));
        }
        let bytes = self.take(len as usize)?;
        std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| corrupt("text"))
    }

    pub(super) fn finish(self) -> Result<(), CacheError> {
        if self.rest.is_empty() { Ok(()) } else { Err(corrupt("trailing bytes")) }
    }
}
