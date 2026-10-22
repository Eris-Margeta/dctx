//! In-memory segment bytes.
//! Implements the [`crate::ports::SegmentRead`] port for tests and callers that already hold a buffer.
//! Depends on [`crate::decode`] for [`MemorySegment::validate`]. Stores can wrap the same bytes.
//! The adapter does not touch the filesystem.

use std::sync::Arc;

use crate::ports::SegmentRead;
use crate::{decode, SegmentError, SegmentRole, ValidatedSegment};

/// A segment held in an `Arc`.
#[derive(Clone, Debug)]
pub struct MemorySegment {
    bytes: Arc<[u8]>,
}

impl MemorySegment {
    /// Copies `bytes` into an `Arc`.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes: Arc::from(bytes) }
    }

    /// Decodes the stored bytes for `role`.
    ///
    /// # Errors
    /// Refuses malformed, corrupt, oversized or wrong-role bytes according to [`crate::decode`].
    pub fn validate(&self, role: SegmentRole) -> Result<ValidatedSegment<'_>, SegmentError> {
        decode(self.bytes(), role)
    }

    /// Borrows the stored bytes.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl SegmentRead for MemorySegment {
    fn len(&self) -> u64 {
        u64::try_from(self.bytes.len()).unwrap_or(u64::MAX)
    }

    fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), SegmentError> {
        let start = usize::try_from(offset).map_err(|_| SegmentError::OffsetOverflow)?;
        let end = start.checked_add(buf.len()).ok_or(SegmentError::OffsetOverflow)?;
        let src = self.bytes.get(start..end).ok_or(SegmentError::Truncated {
            at: offset,
            need: offset.saturating_add(u64::try_from(buf.len()).unwrap_or(u64::MAX)),
        })?;
        buf.copy_from_slice(src);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SegmentBuilder;

    #[test]
    fn a_memory_segment_validates_the_bytes_it_stores() {
        let bytes = SegmentBuilder::fact().build().unwrap();
        let segment = MemorySegment::new(bytes.clone());
        assert_eq!(segment.bytes(), bytes.as_slice());
        assert_eq!(segment.len(), u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        let decoded = segment.validate(SegmentRole::Fact).unwrap();
        assert_eq!(decoded.dictionary(), [] as [&str; 0]);
        let mut buf = [0u8; 4];
        segment.read_at(0, &mut buf).unwrap();
        assert_eq!(&buf, &bytes[..4]);
    }
}
