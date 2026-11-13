// SPDX-License-Identifier: Apache-2.0
//! The export verb: the whole graph of the pinned generation as one document.
//! Implements the `dctx map export` read; it has no cursor, no page header and no token cap, because it is an export.
//! Depends on the index export encoder only.

use super::Session;
use crate::domain::{DctxError, MapRequest};
use crate::index::export::{ExportFormat, write_graph};

impl Session {
    /// Returns every node and every edge of `handle`'s generation as `json` or `mermaid`, sorted so equal generations give equal bytes.
    /// The request only carries the transport checks (version, format, revision, cancellation); its limits are not applied.
    /// # Errors
    /// Refuses invalid/stale handles or requests and a format other than `json` or `mermaid`.
    pub fn export_graph(
        &mut self,
        handle: &super::SnapshotHandle,
        format: &str,
        request: &MapRequest,
    ) -> Result<String, DctxError> {
        let mut output = Vec::new();
        self.export_graph_to(handle, format, request, &mut output)?;
        String::from_utf8(output).map_err(|_| DctxError::io("invalid export UTF-8"))
    }

    /// Writes a graph incrementally, retaining only ordering tables beyond the pinned index.
    /// Every write checks cancellation; a sink failure stops the export immediately.
    /// The caller owns flushing and any transport envelope.
    /// # Errors
    /// Refuses invalid requests/handles/formats, cancellation, or a failed writer.
    pub fn export_graph_to(
        &mut self,
        handle: &super::SnapshotHandle,
        format: &str,
        request: &MapRequest,
        output: &mut dyn std::io::Write,
    ) -> Result<(), DctxError> {
        self.reject_transport(request)?;
        let format = ExportFormat::parse(format)
            .map_err(|_| DctxError::invalid_detail("unknown export format", format))?;
        let generation = self.generation(handle)?;
        let mut writer = CheckedWriter::new(output, self.cancel.as_ref());
        write_graph(&generation, format, &mut writer).map_err(|error| {
            self.cancel
                .checkpoint()
                .map_err(DctxError::from_core)
                .err()
                .unwrap_or_else(|| DctxError::io(error.to_string()))
        })
    }
}

pub(super) struct CheckedWriter<'a> {
    output: &'a mut dyn std::io::Write,
    cancel: &'a dyn dctx_ports::Cancellation,
}

impl<'a> CheckedWriter<'a> {
    pub(super) fn new(
        output: &'a mut dyn std::io::Write,
        cancel: &'a dyn dctx_ports::Cancellation,
    ) -> Self {
        Self { output, cancel }
    }
}

impl std::io::Write for CheckedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut text = std::str::from_utf8(bytes).map_err(std::io::Error::other)?;
        while !text.is_empty() {
            self.cancel.checkpoint().map_err(std::io::Error::other)?;
            let mut end = text.len().min(64 * 1024);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let mut pending = &text.as_bytes()[..end];
            while !pending.is_empty() {
                self.cancel.checkpoint().map_err(std::io::Error::other)?;
                match self.output.write(pending) {
                    Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
                    Ok(count) => pending = &pending[count..],
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error),
                }
            }
            text = &text[end..];
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.output.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HostCeiling, SnapshotRequest, StoragePolicy, Verb};
    use dctx_ports::memory::{CancelFlag, FixedClock, MemorySource};
    use std::io::{self, Write};
    use std::sync::Arc;

    #[test]
    fn interrupted_writes_check_cancellation_before_retrying() {
        struct Client {
            cancel: Arc<CancelFlag>,
            calls: usize,
        }
        impl Write for Client {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                assert_eq!(self.calls, 0, "a cancelled write must not be retried");
                self.calls += 1;
                self.cancel.cancel();
                Err(io::ErrorKind::Interrupted.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let cancel = Arc::new(CancelFlag::new());
        let mut client = Client {
            cancel: cancel.clone(),
            calls: 0,
        };
        let error = CheckedWriter::new(&mut client, cancel.as_ref())
            .write_all(b"page")
            .unwrap_err();
        assert_eq!(error.to_string(), "cancelled; submit a new request to resume work");
        assert_eq!(client.calls, 1);
    }

    #[test]
    fn checked_chunks_are_valid_utf8_for_windows_console_writers() {
        struct Console(usize);
        impl Write for Console {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                assert!(bytes.len() <= 64 * 1024);
                assert!(std::str::from_utf8(bytes).is_ok());
                self.0 += bytes.len();
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let cancel = CancelFlag::new();
        let mut console = Console(0);
        let body = "€".repeat(100_000);
        CheckedWriter::new(&mut console, &cancel)
            .write_all(body.as_bytes())
            .unwrap();
        assert_eq!(console.0, body.len());
    }

    #[test]
    fn cancellation_during_export_stops_writes_and_preserves_the_generation() {
        let cancel = Arc::new(CancelFlag::new());
        let mut source = MemorySource::new();
        source
            .insert_file("a.py", b"def alpha():\n    return 1\n", 0)
            .unwrap();
        let mut session = Session::builder(source)
            .clock(FixedClock(dctx_core::Timestamp(0)))
            .selection(dctx_core::SelectionPolicy::from_texts(
                &crate::RuleTexts::default(),
            ))
            .grants(crate::HostGrants::PermitAll)
            .place("fixture")
            .view("default")
            .ceiling(HostCeiling::base())
            .cancellation(cancel.clone())
            .storage(StoragePolicy::Ephemeral {
                max_bytes: 8 * 1024 * 1024,
            })
            .build()
            .unwrap();
        let handle = session.snapshot(&SnapshotRequest::strict()).unwrap();
        struct Client {
            cancel: Arc<CancelFlag>,
            writes: usize,
        }
        impl Write for Client {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.writes += 1;
                self.cancel.cancel();
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut client = Client { cancel, writes: 0 };
        let request = MapRequest::new(Verb::Neighbors);
        let error = session
            .export_graph_to(&handle, "json", &request, &mut client)
            .unwrap_err();
        assert_eq!(error.code(), crate::ErrorCode::Cancelled);
        assert_eq!(client.writes, 1);
        assert_eq!(session.generation_count(), 1);
        assert!(session.generation(&handle).is_ok());
    }
}
