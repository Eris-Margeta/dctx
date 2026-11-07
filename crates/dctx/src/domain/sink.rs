// SPDX-License-Identifier: Apache-2.0
//! Bounded output sink. The facade writes a finished page or it writes nothing.
//! Implements contracts/API-PORTS-v1.md `BoundedSink`: a refusal does not emit a partial record.
//! Depends on [`super::error::DctxError`]. Query and pack take a sink; they never write to stdout.
//! [`VecSink`] is the in-memory sink used by tests and embedded hosts.

use super::error::DctxError;

/// Accepts one complete encoded page.
pub trait BoundedSink {
    /// Writes `bytes` or returns an IO error. Callers pass a finished page only.
    /// # Errors
    /// Refuses a page that cannot be written atomically within the sink's byte budget, or an underlying sink failure.
    fn write_page(&mut self, bytes: &[u8]) -> Result<(), DctxError>;
}

/// A sink that appends pages into an owned buffer.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VecSink {
    bytes: Vec<u8>,
}

impl VecSink {
    /// An empty buffer.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    /// Bytes written so far.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// True when no page has been written.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl BoundedSink for VecSink {
    fn write_page(&mut self, bytes: &[u8]) -> Result<(), DctxError> {
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_sink_stays_empty_until_a_page_is_written() {
        let mut sink = VecSink::new();
        assert!(sink.is_empty());
        sink.write_page(b"ok\n").expect("write");
        assert_eq!(sink.as_bytes(), b"ok\n");
    }
}
