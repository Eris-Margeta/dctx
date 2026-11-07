// SPDX-License-Identifier: Apache-2.0
//! Minimal named-target context composed solely from typed engine queries.

use super::chunks::chunk_id;
use crate::{Answer, DctxError, MapRequest, Session, SnapshotHandle, VecSink, Verb};

/// Named file/symbol context options, identical on all transports.
#[derive(Debug, Clone)]
pub struct NamedContextRequest {
    /// Stable node ID, place-relative file path, or symbol name.
    pub target: String,
    /// Final encoded output budget in estimated tokens, 100..=100000.
    pub budget: u32,
    /// Text (1) or JSON (2).
    pub output_format: u32,
    /// Opaque delivery session name, required when dedupe is enabled.
    pub session_id: Option<String>,
    /// Emit references to chunks previously delivered in this session.
    pub dedupe: bool,
}

impl NamedContextRequest {
    /// Text context with a 3000-token budget and dedupe disabled.
    #[must_use]
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            budget: 3000,
            output_format: 1,
            session_id: None,
            dedupe: false,
        }
    }

    /// Validates options without reading a source or changing delivery history.
    /// # Errors
    /// Refuses empty/oversized targets, invalid budgets/formats, and invalid session options.
    pub fn validate(&self) -> Result<(), DctxError> {
        if self.target.trim().is_empty()
            || self.target.len() > 1024
            || self.target.chars().any(char::is_control)
        {
            return Err(DctxError::invalid(
                "context requires a target of 1..=1024 bytes without control characters",
            ));
        }
        if !(100..=100_000).contains(&self.budget) || !matches!(self.output_format, 1 | 2) {
            return Err(DctxError::invalid(
                "context requires budget 100..=100000 and format 1 or 2",
            ));
        }
        if self
            .session_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
        {
            return Err(DctxError::invalid(
                "session id must be 1..=128 bytes without control characters",
            ));
        }
        if self.dedupe && self.session_id.is_none() {
            return Err(DctxError::invalid("--dedupe requires --session ID"));
        }
        Ok(())
    }
}

/// One ranked content chunk. A reference omits content from the rendered page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextChunk {
    /// Engine-produced content address.
    pub id: String,
    /// `source` or `related`.
    pub role: String,
    /// Stable node ID.
    pub target: String,
    /// Canonical chunk payload, including source location.
    pub content: String,
    /// True when the client already received this chunk in its delivery session.
    pub reference: bool,
}

impl ContextChunk {
    fn new(role: &str, target: &str, content: String) -> Result<Self, DctxError> {
        Ok(Self {
            id: chunk_id(role, target, &content)?,
            role: role.into(),
            target: target.into(),
            content,
            reference: false,
        })
    }
}

/// One engine-rendered context page, best chunk first.
#[derive(Debug, Clone)]
pub struct NamedContextReport {
    /// Encoded page, including the final newline.
    pub text: String,
    /// Resolved stable target, or the original target if no result exists.
    pub target: String,
    /// Selected chunks, with the verified target source first.
    pub chunks: Vec<ContextChunk>,
    /// Related records omitted by query caps or the output budget.
    pub more: u64,
    /// Engine read continuation when the target source was truncated.
    pub next: Option<String>,
}

fn query(
    session: &mut Session,
    handle: &SnapshotHandle,
    verb: Verb,
    target: &str,
) -> Result<Answer, DctxError> {
    let mut request = MapRequest::new(verb);
    request.output_format = 1;
    if verb == Verb::Find {
        request.query = target.into();
    } else {
        request.target = target.into();
    }
    if verb == Verb::Neighbors {
        request.edges = vec!["calls".into(), "imports".into()];
        request.limits.records = Some(4);
    }
    if verb == Verb::Read {
        request.lines = Some(40);
    }
    session.query(handle, &request, &mut VecSink::new())
}

pub(super) fn build(
    session: &mut Session,
    handle: &SnapshotHandle,
    request: &NamedContextRequest,
) -> Result<NamedContextReport, DctxError> {
    let mut target = request.target.clone();
    if target.parse::<dctx_map_model::IdReference>().is_err() {
        let path = target.strip_prefix("./").unwrap_or(&target);
        if path.contains('/') || path.starts_with('.') {
            dctx_core::ids::RelativePath::parse(path)
                .map_err(|_| DctxError::invalid("context requires a place-relative file path"))?;
        }
        let module = format!("m:{path}");
        let file = query(session, handle, Verb::Explain, &module)?;
        if file.records.is_empty() {
            let found = query(session, handle, Verb::Find, &target)?;
            let Some(record) = found.records.first() else {
                return Ok(NamedContextReport {
                    text: String::new(),
                    target,
                    chunks: Vec::new(),
                    more: 0,
                    next: None,
                });
            };
            target.clone_from(&record.id);
        } else {
            target = module;
        }
    }
    let source = query(session, handle, Verb::Read, &target)?;
    let mut report = NamedContextReport {
        text: String::new(),
        target,
        chunks: Vec::new(),
        more: 0,
        next: source.next,
    };
    let Some(record) = source.records.first() else {
        return Ok(report);
    };
    report.target.clone_from(&record.id);
    let content = format!(
        "{}:{}-{}\n{}",
        record.path,
        record.lines[0],
        record.lines[1],
        record.source_utf8.as_deref().unwrap_or_default()
    );
    report
        .chunks
        .push(ContextChunk::new("source", &record.id, content)?);
    let neighbors = query(session, handle, Verb::Neighbors, &report.target)?;
    report.more = neighbors.more;
    let mut seen = std::collections::BTreeSet::from([report.target.clone()]);
    for neighbor in neighbors.records {
        let Some(id) = neighbor.target else {
            continue;
        };
        if !seen.insert(id.clone()) {
            continue;
        }
        let explained = query(session, handle, Verb::Explain, &id)?;
        if let Some(record) = explained.records.first() {
            let content = format!(
                "{}:{}-{}\n{}\n{}",
                record.path,
                record.lines[0],
                record.lines[1],
                record.signature.as_deref().unwrap_or(&record.kind),
                record.summary
            );
            report
                .chunks
                .push(ContextChunk::new("related", &id, content)?);
        }
    }
    Ok(report)
}

pub(super) fn render(report: &NamedContextReport, format: u32) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if format == 1 {
        let _ = writeln!(out, "# dctx context {}", report.target);
        for chunk in &report.chunks {
            let tag = if chunk.reference { "ref" } else { "chunk" };
            let _ = writeln!(out, "## {tag} {} {} {}", chunk.id, chunk.role, chunk.target);
            if !chunk.reference {
                out.push_str(&chunk.content);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
        let _ = writeln!(
            out,
            "# end chunks={} more={}",
            report.chunks.len(),
            report.more
        );
        if let Some(next) = &report.next {
            let _ = writeln!(out, "# next {next}");
        }
    } else {
        out.push_str("{\"v\":2,\"verb\":\"context\",\"target\":");
        crate::domain::quote_json(&mut out, &report.target);
        out.push_str(",\"chunks\":[");
        for (index, chunk) in report.chunks.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"id\":");
            crate::domain::quote_json(&mut out, &chunk.id);
            out.push_str(",\"role\":");
            crate::domain::quote_json(&mut out, &chunk.role);
            out.push_str(",\"target\":");
            crate::domain::quote_json(&mut out, &chunk.target);
            let _ = write!(out, ",\"ref\":{}", chunk.reference);
            if !chunk.reference {
                out.push_str(",\"content\":");
                crate::domain::quote_json(&mut out, &chunk.content);
            }
            out.push('}');
        }
        let _ = write!(out, "],\"more\":{},\"next\":", report.more);
        if let Some(next) = &report.next {
            crate::domain::quote_json(&mut out, next);
        } else {
            out.push_str("null");
        }
        out.push_str("}\n");
    }
    out
}
