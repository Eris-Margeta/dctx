// SPDX-License-Identifier: Apache-2.0
//! Raw, verified source for the versioned pack contract; no display truncation markers.

use super::Session;
use crate::{AnswerRecord, DctxError, SnapshotHandle};

impl Session {
    pub(crate) fn context_source(
        &mut self,
        handle: &SnapshotHandle,
        record: &mut AnswerRecord,
    ) -> Result<(), DctxError> {
        self.authorize(&record.path)?;
        let generation = self.generation_for_query(handle)?;
        let expected = generation
            .files
            .iter()
            .find(|file| file.path == record.path)
            .and_then(|file| file.digest)
            .ok_or(DctxError::Stale {
                reason: "no stored digest",
            })?;
        let bytes = self.read_verified(&record.path, expected)?;
        let span = record.span.ok_or(DctxError::CorruptIndex {
            reason: "missing span",
        })?;
        let start =
            usize::try_from(span[0]).map_err(|_| DctxError::CorruptIndex { reason: "span" })?;
        let end =
            usize::try_from(span[1]).map_err(|_| DctxError::CorruptIndex { reason: "span" })?;
        let source = bytes
            .get(start..end)
            .ok_or(DctxError::CorruptIndex { reason: "span" })?;
        let text = std::str::from_utf8(source).map_err(|_| DctxError::CorruptIndex {
            reason: "UTF-8 span",
        })?;
        let line = 1 + bytes[..start].iter().filter(|&&byte| byte == b'\n').count();
        record.lines[0] = u32::try_from(line).map_err(|_| DctxError::CorruptIndex {
            reason: "line overflow",
        })?;
        record.source_utf8 = Some(text.to_owned());
        Ok(())
    }
}
