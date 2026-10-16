//! Bounded parallel parsing of files whose authorized content changed.
//! Implements SPEC-MAP-MODE section 7 without reparsing unchanged files.
//! Uses the public stateless parser; the indexer assembles and commits results.
use crate::{
    Error, Result,
    domain::cached::CachedFile,
    ports::{Snapshot, SourceFile},
};
use dctx_map_parse::{ParsedFile, Parser, TreeSitterParser};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

type Parsed = std::result::Result<ParsedFile, String>;
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
            old.get(*path)
                .is_none_or(|old| old.hash != current.hash || old.readable != current.readable)
        })
        .collect();
    if pending.is_empty() {
        return Ok(BTreeMap::new());
    }
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
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
fn parse(file: &SourceFile, path: &str, started: Instant, deadline: Option<Duration>) -> Parsed {
    if deadline.is_some_and(|limit| started.elapsed() >= limit) {
        return Err("refresh deadline exceeded".into());
    }
    TreeSitterParser
        .parse(path, file.bytes.as_deref().unwrap_or(b""))
        .map_err(|e| e.to_string())
}
