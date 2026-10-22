//! Bounded parallel parsing of files whose authorized content changed.
//! Implements SPEC-MAP-MODE section 7 without reparsing unchanged files.
//! Uses the public stateless parser; the indexer assembles and commits results.
use crate::{
    Error, ParseDiagnostic, ParseStatus, Result,
    domain::cached::CachedFile,
    domain::parse_status::PARSER_REVISION,
    ports::{Snapshot, SourceFile},
};
use dctx_map_parse::{Language, ParseError, ParsedFile, Parser, TreeSitterParser};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

type Parsed = std::result::Result<(ParsedFile, ParseStatus), String>;
pub(super) fn parse_changed(
    snapshot: &Snapshot,
    old: &BTreeMap<String, CachedFile>,
    started: Instant,
    deadline: Option<Duration>,
) -> Result<BTreeMap<String, Parsed>> {
    let pending: Vec<_> = snapshot
        .files
        .iter()
        .filter(|(path, current)| {
            old.get(*path).is_none_or(|old| {
                old.hash != current.hash
                    || old.readable != current.readable
                    || old.parser_revision != PARSER_REVISION
            })
        })
        .collect();
    if pending.is_empty() {
        return Ok(BTreeMap::new());
    }
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(8)
        .min(pending.len());
    let cursor = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let cursor = &cursor;
        let pending = &pending;
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(move || {
                    let mut parsed = Vec::new();
                    while let Some((path, current)) =
                        pending.get(cursor.fetch_add(1, Ordering::Relaxed))
                    {
                        parsed.push(((*path).clone(), parse(current, path, started, deadline)));
                    }
                    parsed
                })
            })
            .collect();
        let mut parsed = BTreeMap::new();
        for handle in handles {
            parsed.extend(
                handle
                    .join()
                    .map_err(|_| Error::Request("parser worker failed".into()))?,
            );
        }
        Ok(parsed)
    })
}
#[allow(clippy::cast_possible_truncation, reason = "The line count is explicitly clamped to 1..=u32::MAX before conversion.")]
fn parse(file: &SourceFile, path: &str, started: Instant, deadline: Option<Duration>) -> Parsed {
    if deadline.is_some_and(|limit| started.elapsed() >= limit) {
        return Err("refresh deadline exceeded".into());
    }
    let bytes = file.bytes.as_deref().unwrap_or(b"");
    let error = match TreeSitterParser.parse(path, bytes) {
        Ok(parsed) => return Ok((parsed, ParseStatus::Parsed)),
        Err(error) => error,
    };
    let line = match &error {
        ParseError::Syntax { line, .. } => *line,
        ParseError::InvalidUtf8(_) | ParseError::TooLarge => 1,
        _ => return Err(error.to_string()),
    };
    let diagnostic = ParseDiagnostic {
        error: error.to_string(),
        lines: [line, line],
        syntax: String::from_utf8_lossy(bytes)
            .lines()
            .nth(line.saturating_sub(1) as usize)
            .unwrap_or_default()
            .chars()
            .take(160)
            .collect(),
    };
    if Language::for_path(path) == Language::Rust
        && matches!(error, ParseError::Syntax { .. })
        && let Some(parsed) = super::recovery::recover(path, bytes, &diagnostic)
    {
        return Ok((
            parsed,
            ParseStatus::Partial {
                diagnostics: vec![diagnostic],
            },
        ));
    }
    let mut parsed = TreeSitterParser
        .parse(path, b"")
        .map_err(|e| e.to_string())?;
    for node in &mut parsed.nodes {
        let lines =
            bytes.split(|byte| *byte == b'\n').count() - usize::from(bytes.ends_with(b"\n"));
        node.lines = [1, lines.clamp(1, u32::MAX as usize) as u32];
        node.fingerprint.clear();
        node.summary.text = format!("unparseable: {}", diagnostic.error);
    }
    Ok((parsed, ParseStatus::Unparseable { diagnostic }))
}
