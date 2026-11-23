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
            self.dedupe(name, &mut report, request.output_format)?;
        }
        Ok(report)
    }

    fn dedupe(
        &mut self,
        name: &str,
        report: &mut NamedContextReport,
        output_format: u32,
    ) -> Result<(), DctxError> {
        let previous = self.seen.get(name);
        if previous.is_none() && self.seen.len() >= MAX_SESSIONS {
            return Err(DctxError::invalid(
                "context session capacity reached; reset a session",
            ));
        }

        let mut additions = BTreeSet::new();
        // Only prior pages can satisfy a reference, even for duplicate IDs in this page.
        for chunk in &mut report.chunks {
            chunk.reference = previous.is_some_and(|seen| seen.contains(&chunk.id));
            if !chunk.reference {
                additions.insert(&chunk.id);
            }
        }
        let previous_count = previous.map_or(0, |seen| seen.len());
        if previous_count + additions.len() > MAX_CHUNKS {
            return Err(DctxError::invalid(
                "context chunk capacity reached; reset the session",
            ));
        }
        if !additions.is_empty() {
            Arc::make_mut(self.seen.entry(name.to_string()).or_default())
                .extend(additions.into_iter().cloned());
        }
        report.text = render(report, output_format);
        Ok(())
    }

    /// Forgets a delivery session. Its next request emits full chunks again.
    pub fn reset(&mut self, session_id: &str) {
        self.seen.remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextChunk;

    fn sessions_with_history(count: usize) -> ContextSessions {
        let history = (0..count).map(|id| id.to_string()).collect();
        ContextSessions {
            seen: BTreeMap::from([("session".into(), Arc::new(history))]),
        }
    }

    fn page(ids: &[&str]) -> NamedContextReport {
        NamedContextReport {
            text: String::new(),
            target: "s:a.rs#f".into(),
            chunks: ids
                .iter()
                .map(|&id| ContextChunk {
                    id: id.into(),
                    role: "source".into(),
                    target: "s:a.rs#f".into(),
                    content: "fn f() {}\n".into(),
                    reference: false,
                })
                .collect(),
            more: 0,
            next: None,
        }
    }

    #[test]
    fn repeated_pages_share_history_and_preserve_reference_bytes() {
        let mut sessions = sessions_with_history(MAX_CHUNKS);
        let cloned = sessions.clone();
        for (format, expected) in [
            (
                1,
                "# dctx context s:a.rs#f\n## ref 0 source s:a.rs#f\n# end chunks=1 more=0\n",
            ),
            (
                2,
                concat!(
                    "{\"v\":2,\"verb\":\"context\",\"target\":\"s:a.rs#f\",\"chunks\":[",
                    "{\"id\":\"0\",\"role\":\"source\",\"target\":\"s:a.rs#f\",\"ref\":true}],",
                    "\"more\":0,\"next\":null}\n",
                ),
            ),
        ] {
            let mut report = page(&["0"]);
            sessions.dedupe("session", &mut report, format).unwrap();
            assert!(report.chunks[0].reference);
            assert_eq!(report.text, expected);
            assert!(Arc::ptr_eq(
                &sessions.seen["session"],
                &cloned.seen["session"]
            ));
        }
    }

    #[test]
    fn duplicate_new_ids_count_once_at_the_chunk_limit() {
        let mut sessions = sessions_with_history(MAX_CHUNKS - 1);
        let cloned = sessions.clone();
        let mut report = page(&["new", "new"]);
        sessions.dedupe("session", &mut report, 1).unwrap();

        assert!(report.chunks.iter().all(|chunk| !chunk.reference));
        assert_eq!(sessions.seen["session"].len(), MAX_CHUNKS);
        assert!(sessions.seen["session"].contains("new"));
        assert!(!cloned.seen["session"].contains("new"));
    }

    #[test]
    fn mixed_overflow_preserves_shared_history_and_allows_retry() {
        let mut sessions = sessions_with_history(MAX_CHUNKS - 1);
        let cloned = sessions.clone();
        let mut overflow = page(&["0", "new", "other"]);
        assert!(matches!(
            sessions.dedupe("session", &mut overflow, 1),
            Err(DctxError::InvalidRequest {
                reason: "context chunk capacity reached; reset the session",
                ..
            })
        ));
        assert!(Arc::ptr_eq(
            &sessions.seen["session"],
            &cloned.seen["session"]
        ));

        let mut retry = page(&["0", "new"]);
        sessions.dedupe("session", &mut retry, 1).unwrap();
        assert!(retry.chunks[0].reference);
        assert!(!retry.chunks[1].reference);
        assert_eq!(sessions.seen["session"].len(), MAX_CHUNKS);
        assert!(sessions.seen["session"].contains("new"));
        assert!(!sessions.seen["session"].contains("other"));
    }
}
