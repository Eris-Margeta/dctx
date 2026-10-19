//! Snapshot walk. Excluded directories are not listed, and excluded files are not opened.
//! Implements contracts/API-PORTS-v1.md source confinement and the freshness modes.
//! Depends on the source port, the effective policy and the structural extractor.
//! The session turns a [`Walked`] tree into one generation. Cancellation leaves no generation.

use dctx_core::facts::ReadGrant;
use dctx_core::ids::RelativePath;
use dctx_core::FileHandlingDecision;
use dctx_map_model::{NodeKind, StableId};
use dctx_ports::{Cancellation, Source};

use super::extract::{extract_structural, index_symbols, language_of, Symbol};
use super::profile::{self, FileWatch, Stage, WalkSpan};
use super::{IndexedEdge, IndexedFile, IndexedNode};
use crate::domain::{
    ancestors_include, check_deadline, digest_text, FreshnessMode, ParserChoice, PolicyView, ScanLimits, DctxError,
    Resource, STRUCTURAL_PARSER,
};

/// What one snapshot read from the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Walked {
    /// Files that were not excluded.
    pub files: Vec<IndexedFile>,
    /// Graph nodes.
    pub nodes: Vec<IndexedNode>,
    /// Contains edges.
    pub edges: Vec<IndexedEdge>,
    /// Coverage label.
    pub coverage: String,
    /// Completion label.
    pub completion: String,
    /// Parser id.
    pub parser: String,
    /// Content reads performed, including reads that returned an error.
    pub reads: u64,
}

/// Borrowed inputs for one walk.
pub(crate) struct WalkArgs<'a> {
    /// Explicit source.
    pub source: &'a dyn Source,
    /// Cooperative cancellation.
    pub cancel: &'a dyn Cancellation,
    /// Effective policy.
    pub policy: PolicyView<'a>,
    /// Safety or caller walk caps.
    pub limits: &'a ScanLimits,
    /// Freshness mode.
    pub freshness: FreshnessMode,
    /// Parser selected by the plan.
    pub parser: ParserChoice,
    /// Deadline measured on the injected clock.
    pub deadline_ms: Option<u64>,
    /// Clock instant when the snapshot started.
    pub started_ms: u64,
    /// Clock instant for this walk. A frozen clock equals `started_ms`.
    pub now_ms: u64,
}

struct State {
    files: Vec<IndexedFile>,
    nodes: Vec<IndexedNode>,
    edges: Vec<IndexedEdge>,
    entries: u64,
    source_bytes: u64,
    reads: u64,
    syntax_files: u32,
    other_files: u32,
    partial: bool,
}

/// Walks `args.source` from the root. The root is listed even when a plan root would skip `""`.
pub(crate) fn walk_source(args: &WalkArgs<'_>) -> Result<Walked, DctxError> {
    // File watches inside this span close per file. Seal and query are not part of it.
    let _walk = WalkSpan::begin();
    checkpoint(args)?;
    args.source.root_kind().map_err(DctxError::from_core)?;
    let languages = args.policy.languages()?;
    let mut state = State {
        files: Vec::new(),
        nodes: Vec::new(),
        edges: Vec::new(),
        entries: 1,
        source_bytes: 0,
        reads: 0,
        syntax_files: 0,
        other_files: 0,
        partial: false,
    };
    if state.entries > args.limits.entries {
        return Err(limit(Resource::Entries, args.limits.entries));
    }
    visit_dir(args, &languages, &RelativePath::root(), &mut state)?;
    Ok(finish(args, state))
}

fn visit_dir(args: &WalkArgs<'_>, languages: &Option<Vec<String>>, dir: &RelativePath, state: &mut State) -> Result<(), DctxError> {
    checkpoint(args)?;
    let mut entries = args.source.entries(dir).map_err(DctxError::from_core)?;
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    for entry in entries {
        checkpoint(args)?;
        let path = dir.child(&entry.name).map_err(DctxError::from_core)?;
        let mut decision = args.policy.decide(path.as_str(), entry.is_dir())?;
        if !entry.is_dir() && decision == FileHandlingDecision::Include && !language_ok(languages, path.as_str()) {
            decision = FileHandlingDecision::OmitContent;
        }
        if decision == FileHandlingDecision::Exclude {
            continue;
        }
        let depth = u64::try_from(path.depth()).unwrap_or(u64::MAX);
        if depth > args.limits.depth {
            continue;
        }
        state.entries += 1;
        if state.entries > args.limits.entries {
            return Err(limit(Resource::Entries, args.limits.entries));
        }
        if entry.is_dir() && decision.should_recurse() {
            visit_dir(args, languages, &path, state)?;
        } else if !entry.is_dir() {
            visit_file(args, path, decision, state)?;
        }
    }
    Ok(())
}

fn visit_file(args: &WalkArgs<'_>, path: RelativePath, decision: FileHandlingDecision, state: &mut State) -> Result<(), DctxError> {
    let meta = match args.source.metadata(&path).map_err(DctxError::from_core) {
        Ok(meta) => meta,
        Err(error @ (DctxError::Cancelled | DctxError::ResourceLimit { .. })) => return Err(error),
        Err(_) => {
            state.partial = true;
            state.files.push(file_row(path.as_str(), decision, None, 0, 0));
            return Ok(());
        }
    };
    let readable = decision == FileHandlingDecision::Include
        && args.freshness != FreshnessMode::Metadata
        && ancestors_include(|item, is_dir| args.policy.decide(item, is_dir), path.as_str())?;
    if !readable {
        if args.freshness == FreshnessMode::Metadata {
            push_module(&mut state.nodes, path.as_str(), 0, [1, 1]);
        }
        state.files.push(file_row(path.as_str(), decision, None, meta.size, meta.modified.millis()));
        return Ok(());
    }
    if meta.size > args.limits.file_bytes {
        state.partial = true;
        state.files.push(file_row(path.as_str(), decision, None, meta.size, meta.modified.millis()));
        return Ok(());
    }
    if state.source_bytes.saturating_add(meta.size) > args.limits.source_bytes {
        return Err(limit(Resource::SourceBytes, args.limits.source_bytes));
    }
    // Drop writes this file before the function returns, so seal and query cannot join its parse.
    let mut watch = FileWatch::begin(path.as_str());
    let read_mark = profile::start();
    let read = read_file(args, &path, state);
    watch.charge(Stage::Read, read_mark);
    let bytes = match read {
        Ok(bytes) => bytes,
        Err(DctxError::Cancelled) => return Err(DctxError::Cancelled),
        Err(DctxError::ResourceLimit { resource, limit }) => return Err(DctxError::ResourceLimit { resource, limit }),
        Err(_) => {
            state.partial = true;
            state.files.push(file_row(path.as_str(), decision, None, meta.size, meta.modified.millis()));
            return Ok(());
        }
    };
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if state.source_bytes.saturating_add(len) > args.limits.source_bytes {
        return Err(limit(Resource::SourceBytes, args.limits.source_bytes));
    }
    state.source_bytes += len;
    watch.set_bytes(len);
    let hash_mark = profile::start();
    let digested = digest_text(&bytes);
    watch.charge(Stage::Hash, hash_mark);
    let digest = digested?;
    let span_mark = profile::start();
    let lines = line_span(&bytes);
    watch.charge(Stage::SpanConversion, span_mark);
    let module = push_module(&mut state.nodes, path.as_str(), len, [1, lines]);
    if let Some(module_id) = module {
        let parse_mark = profile::start();
        let parsed = symbols_for(args, path.as_str(), &bytes);
        watch.charge(Stage::ParseLex, parse_mark);
        let (symbols, syntax) = parsed?;
        if syntax {
            state.syntax_files += 1;
        } else if args.parser != ParserChoice::Opaque {
            state.other_files += 1;
        }
        watch.set_symbols(u64::try_from(symbols.len()).unwrap_or(u64::MAX));
        let graph_mark = profile::start();
        index_symbols(path.as_str(), &module_id, &symbols, &mut state.nodes, &mut state.edges);
        watch.charge(Stage::GraphAssembly, graph_mark);
    }
    state.files.push(file_row(path.as_str(), decision, Some(digest), meta.size, meta.modified.millis()));
    Ok(())
}

fn read_file(args: &WalkArgs<'_>, path: &RelativePath, state: &mut State) -> Result<Vec<u8>, DctxError> {
    checkpoint(args)?;
    let grant = ReadGrant::authorize(path, FileHandlingDecision::Include, true).ok_or(DctxError::PolicyDenied)?;
    state.reads += 1;
    args.source.read(&grant, args.limits.file_bytes).map_err(DctxError::from_core)
}

fn symbols_for(args: &WalkArgs<'_>, path: &str, bytes: &[u8]) -> Result<(Vec<Symbol>, bool), DctxError> {
    if bytes.contains(&0) || args.parser == ParserChoice::Opaque || language_of(path) == "opaque" {
        return Ok((Vec::new(), false));
    }
    if args.parser == ParserChoice::Syn && language_of(path) == "rust" {
        return extract_syn(bytes, args.limits.file_bytes);
    }
    Ok((extract_structural(path, bytes), false))
}

fn extract_syn(bytes: &[u8], max_file: u64) -> Result<(Vec<Symbol>, bool), DctxError> {
    #[cfg(feature = "syntax-rust")]
    {
        super::syn_extract::extract(bytes, max_file)
    }
    #[cfg(not(feature = "syntax-rust"))]
    {
        let _ = (bytes, max_file);
        Err(DctxError::unsupported("syntax-rust"))
    }
}

fn push_module(nodes: &mut Vec<IndexedNode>, path: &str, len: u64, lines: [u32; 2]) -> Option<String> {
    let id = StableId::new(NodeKind::Module, path, None, None).ok()?;
    let id = id.as_str().to_string();
    nodes.push(IndexedNode {
        id: id.clone(),
        kind: "module".to_string(),
        name: String::new(),
        path: path.to_string(),
        lines,
        span: [0, len],
        summary: String::new(),
        summary_source: String::new(),
        signature: String::new(),
    });
    Some(id)
}

fn file_row(path: &str, decision: FileHandlingDecision, digest: Option<dctx_hash::ContentDigest>, size: u64, modified: u64) -> IndexedFile {
    IndexedFile { path: path.to_string(), decision, digest, size, modified }
}

fn finish(args: &WalkArgs<'_>, state: State) -> Walked {
    let metadata = args.freshness == FreshnessMode::Metadata;
    let coverage = if metadata {
        "structural"
    } else if args.parser == ParserChoice::Opaque {
        "opaque"
    } else if state.syntax_files > 0 && state.other_files == 0 {
        "syntax"
    } else {
        "structural"
    };
    let parser = if coverage == "syntax" {
        "dctx-lex-rust/syn-3.0.9"
    } else if coverage == "opaque" {
        "opaque/1"
    } else {
        STRUCTURAL_PARSER
    };
    let completion = if metadata {
        "metadata"
    } else if state.partial {
        "partial"
    } else {
        "complete"
    };
    Walked {
        files: state.files,
        nodes: state.nodes,
        edges: state.edges,
        coverage: coverage.to_string(),
        completion: completion.to_string(),
        parser: parser.to_string(),
        reads: state.reads,
    }
}

fn language_ok(languages: &Option<Vec<String>>, path: &str) -> bool {
    match languages {
        None => true,
        Some(list) => list.iter().any(|language| language == language_of(path)),
    }
}

fn line_span(bytes: &[u8]) -> u32 {
    if bytes.is_empty() {
        return 1;
    }
    let mut lines = bytecount(bytes, b'\n');
    if !bytes.ends_with(b"\n") {
        lines += 1;
    }
    u32::try_from(lines).unwrap_or(u32::MAX).max(1)
}

fn bytecount(bytes: &[u8], needle: u8) -> usize {
    bytes.iter().filter(|byte| **byte == needle).count()
}

fn checkpoint(args: &WalkArgs<'_>) -> Result<(), DctxError> {
    args.cancel.checkpoint().map_err(DctxError::from_core)?;
    check_deadline(args.started_ms, args.now_ms, args.deadline_ms)
}

fn limit(resource: Resource, limit: u64) -> DctxError {
    DctxError::ResourceLimit { resource, limit }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FreshnessMode, HostGrants, OperationLimits, ParserChoice, PolicyView};
    use dctx_core::selection::{RuleTexts, SelectionPolicy};
    use dctx_ports::memory::{CancelFlag, MemorySource};
    use dctx_template::HostCeiling;

    #[test]
    fn excluded_git_is_not_read() {
        let mut source = MemorySource::new();
        source.insert_dir(".git", 0).expect("dir");
        source.insert_file(".git/config", b"secret", 0).expect("git");
        source.insert_file("src/lib.rs", b"fn ping() {}\n", 0).expect("src");
        let selection = SelectionPolicy::from_texts(&RuleTexts::default());
        let ceiling = HostCeiling::base();
        let limits = crate::domain::resolve_scan(&OperationLimits::default()).expect("limits");
        let walked = walk_source(&WalkArgs {
            source: &source,
            cancel: &CancelFlag::new(),
            policy: PolicyView {
                selection: &selection,
                ceiling: &ceiling,
                grants: HostGrants::PermitAll,
                plan: None,
                view: "map",
            },
            limits: &limits,
            freshness: FreshnessMode::ImmutableSnapshot,
            parser: ParserChoice::Structural,
            deadline_ms: None,
            started_ms: 0,
            now_ms: 0,
        })
        .expect("walk");
        assert!(walked.files.iter().all(|file| !file.path.starts_with(".git")));
        assert_eq!(source.reads(), vec!["src/lib.rs".to_string()]);
    }

    #[test]
    fn profiled_files_report_bytes_symbols_and_leave_finalization_out() {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                profile::discard();
            }
        }
        let _reset = Reset;
        profile::enable();
        let mut source = MemorySource::new();
        let mut expected = Vec::new();
        for count in 1..=5 {
            let mut body = String::new();
            for index in 0..count {
                body.push_str(&format!("fn f{index}() {{}}\n"));
            }
            let path = format!("f{count}.rs");
            source.insert_file(&path, body.as_bytes(), 0).expect("file");
            expected.push((path, body.len() as u64, count as u64));
        }
        let selection = SelectionPolicy::from_texts(&RuleTexts::default());
        let ceiling = HostCeiling::base();
        let limits = crate::domain::resolve_scan(&OperationLimits::default()).expect("limits");
        let walked = walk_source(&WalkArgs {
            source: &source,
            cancel: &CancelFlag::new(),
            policy: PolicyView {
                selection: &selection,
                ceiling: &ceiling,
                grants: HostGrants::PermitAll,
                plan: None,
                view: "map",
            },
            limits: &limits,
            freshness: FreshnessMode::ImmutableSnapshot,
            parser: ParserChoice::Structural,
            deadline_ms: None,
            started_ms: 0,
            now_ms: 0,
        })
        .expect("walk");
        assert_eq!(walked.files.len(), 5);
        profile::add_ns(Stage::CanonicalizeSeal, 1_000);
        profile::add_ns(Stage::PersistReuse, 2_000);
        profile::add_ns(Stage::Query, 4_000);
        let report = profile::report().expect("report");
        let mut files = Vec::new();
        let mut stages = Vec::new();
        for line in report.lines() {
            if let Some(rest) = line.strip_prefix("file ") {
                let mut parts = rest.splitn(4, ' ');
                let nanos: u128 = parts.next().and_then(|value| value.parse().ok()).expect("file ns");
                let bytes: u64 = parts.next().and_then(|value| value.parse().ok()).expect("bytes");
                let symbols: u64 = parts.next().and_then(|value| value.parse().ok()).expect("symbols");
                let path = parts.next().expect("path").to_string();
                files.push((nanos, bytes, symbols, path));
            } else if let Some(rest) = line.strip_prefix("stage ") {
                let (name, value) = rest.rsplit_once(' ').expect("stage");
                stages.push((name.to_string(), value.parse::<u128>().expect("stage ns")));
            }
        }
        let names: Vec<_> = stages.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "walk",
                "read",
                "hash",
                "parse/lex",
                "span conversion",
                "graph assembly",
                "canonicalize/seal",
                "persist/reuse",
                "query",
            ]
        );
        assert!(files.windows(2).all(|pair| pair[0].0 >= pair[1].0), "{report}");
        assert_eq!(files.len(), 5, "{report}");
        let mut seen: Vec<_> = files.iter().map(|(_, bytes, symbols, path)| (path.clone(), *bytes, *symbols)).collect();
        seen.sort();
        expected.sort();
        assert_eq!(seen, expected, "{report}");
        let file_sum: u128 = files.iter().map(|row| row.0).sum();
        let stage_sum: u128 = stages
            .iter()
            .filter(|(name, _)| matches!(name.as_str(), "read" | "hash" | "parse/lex" | "span conversion" | "graph assembly"))
            .map(|(_, ns)| *ns)
            .sum();
        assert_eq!(file_sum, stage_sum, "{report}");
        assert_eq!(stages.iter().find(|(name, _)| name == "canonicalize/seal").map(|(_, ns)| *ns), Some(1_000));
        assert_eq!(stages.iter().find(|(name, _)| name == "persist/reuse").map(|(_, ns)| *ns), Some(2_000));
        assert_eq!(stages.iter().find(|(name, _)| name == "query").map(|(_, ns)| *ns), Some(4_000));
    }
}
