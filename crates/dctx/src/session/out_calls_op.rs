// SPDX-License-Identifier: Apache-2.0
//! Unresolved outbound calls for a neighbors page (DCX-54c): the call names in the requested node's own span that no
//! resolved outbound `calls` record names, as `calls:out:?<name>` records after the resolved ones.
//! Reads the node's file once through the same verified read as `read` and `sites`; a file that cannot be read adds nothing.
//! Depends on [`crate::index::out_calls`] for the plan, the scan and the records.

use super::Session;
use crate::domain::{AnswerRecord, MapRequest};
use crate::index::{out_calls, Generation};

impl Session {
    /// Appends the unresolved outbound call records of `request` to `records`, after every existing record.
    pub(super) fn attach_out_calls(&mut self, generation: &Generation, request: &MapRequest, records: &mut Vec<AnswerRecord>) {
        let Some(plan) = out_calls::plan(generation, request, records) else {
            return;
        };
        let digest = generation.files.iter().find(|file| file.path == plan.path).and_then(|file| file.digest);
        let bytes = match digest {
            Some(expected) if self.authorize(&plan.path).is_ok() => self.read_verified(&plan.path, expected).ok(),
            _ => None,
        };
        let Some(bytes) = bytes else {
            return;
        };
        let len = bytes.len();
        let start = usize::try_from(plan.span[0]).unwrap_or(len).min(len);
        let end = usize::try_from(plan.span[1]).unwrap_or(len).min(len);
        let text = String::from_utf8_lossy(bytes.get(start..end).unwrap_or(&[]));
        let mut found = out_calls::calls_in(plan.language, &text, plan.first, &plan.own);
        found.retain(|(name, _)| !plan.known.contains(name));
        records.extend(out_calls::records(&plan.path, found));
    }
}
