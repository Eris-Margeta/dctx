// SPDX-License-Identifier: Apache-2.0
//! Content identity and bounded, process-local delivery sessions for named context.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::{DctxError, Session, SnapshotHandle};

use super::named::{NamedContextReport, NamedContextRequest, build, render};

const MAX_SESSIONS: usize = 128;
const MAX_CHUNKS: usize = 4096;

/// Stable content identity: `chunk:v1:sha256:<64 lowercase hex digits>:<payload bytes>`.
/// The hash covers the domain `dctx-chunk-v1\0`, then the role, target, and content,
/// each prefixed by its UTF-8 byte length as a little-endian u64.
/// # Errors
/// Refuses a payload longer than SHA-256 can represent.
pub fn chunk_id(role: &str, target: &str, content: &str) -> Result<String, DctxError> {
    use std::fmt::Write as _;
    let mut hash = dctx_hash::Sha256::new();
    hash.update(b"dctx-chunk-v1\0")
        .map_err(|_| DctxError::invalid("chunk too large"))?;
    for part in [role, target, content] {
        hash.update(&(part.len() as u64).to_le_bytes())
            .map_err(|_| DctxError::invalid("chunk too large"))?;
        hash.update(part.as_bytes())
            .map_err(|_| DctxError::invalid("chunk too large"))?;
    }
    let mut id = String::from("chunk:v1:sha256:");
    for byte in hash.finalize().as_bytes() {
        let _ = write!(id, "{byte:02x}");
    }
    let _ = write!(id, ":{}", content.len());
    Ok(id)
}

/// Engine-owned delivery history, scoped to one client connection or CLI invocation.
/// Different session IDs are isolated. At capacity, requests refuse without evicting
/// history: a previously emitted reference must remain resolvable by the client.
#[derive(Clone, Default)]
pub struct ContextSessions {
    seen: BTreeMap<String, Arc<BTreeSet<String>>>,
}

impl ContextSessions {
    /// Returns minimal ranked context using the session's existing verified reads.
    /// Dedupe is opt-in and only records chunks included in a successful page.
    /// # Errors
    /// Refuses invalid options, stale/denied reads, exhausted history, or a budget
    /// too small for the target chunk. No refusal changes delivery history.
    pub fn context(
        &mut self,
        session: &mut Session,
        handle: &SnapshotHandle,
        request: &NamedContextRequest,
    ) -> Result<NamedContextReport, DctxError> {
        request.validate()?;
        let limits = session.context_output_limits()?;
        let tokens = u64::from(request.budget).min(limits.estimated_tokens.unwrap_or(u64::MAX));
        let bytes = limits.encoded_bytes.unwrap_or(u64::MAX);
        let records = limits.records.unwrap_or(u64::MAX);
        let mut report = build(session, handle, request)?;
        // Selection uses full chunks, so enabling dedupe cannot alter ranking or selection.
        report.text = render(&report, request.output_format);
        while super::page::estimate_tokens(report.text.len()) > tokens
            || report.text.len() as u64 > bytes
            || report.chunks.len() as u64 > records
        {
            if report.chunks.len() <= 1 {
                return Err(DctxError::BudgetTooSmall {
                    minimum_bytes: report.text.len() as u64,
                });
            }
            report.chunks.pop();
            report.more += 1;
            report.text = render(&report, request.output_format);
        }
        if request.dedupe && !report.chunks.is_empty() {
            let name = request.session_id.as_deref().unwrap_or_default();
            if !self.seen.contains_key(name) && self.seen.len() >= MAX_SESSIONS {
                return Err(DctxError::invalid(
                    "context session capacity reached; reset a session",
                ));
            }
            let previous = self.seen.get(name);
            let additions: BTreeSet<_> =
                report.chunks.iter().map(|chunk| chunk.id.clone()).collect();
            let total = previous.map_or(additions.len(), |seen| seen.union(&additions).count());
            if total > MAX_CHUNKS {
                return Err(DctxError::invalid(
                    "context chunk capacity reached; reset the session",
                ));
            }
            for chunk in &mut report.chunks {
                chunk.reference = previous.is_some_and(|seen| seen.contains(&chunk.id));
            }
            report.text = render(&report, request.output_format);
            Arc::make_mut(self.seen.entry(name.to_string()).or_default()).extend(additions);
        }
        Ok(report)
    }

    /// Forgets a delivery session. Its next request emits full chunks again.
    pub fn reset(&mut self, session_id: &str) {
        self.seen.remove(session_id);
    }
}
