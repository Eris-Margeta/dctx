//! Snapshot walk. Excluded directories are not listed, and excluded files are not opened.
//! Implements contracts/API-PORTS-v1.md source confinement and the freshness modes.
//! Depends on the source port, the effective policy and the structural extractor.
//! The session turns a [`Walked`] tree into one generation. Cancellation leaves no generation.

use dctx_core::facts::ReadGrant;
use dctx_core::ids::RelativePath;
use dctx_core::FileHandlingDecision;
use dctx_map_model::{NodeKind, StableId};
use dctx_ports::{Cancellation, Source};

use super::extract::{index_prepared, language_of, prepare_structural, Prepared, Symbol};
use super::profile::{self, FileWatch, Stage, WalkSpan};
use super::refs::{self, FileRefs, RawRef};
use super::{IndexedEdge, IndexedFile, IndexedNode, TextBlob, TextSpan};
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
    /// Contains edges, then the resolved calls and imports edges.
    pub edges: Vec<IndexedEdge>,
    /// Ids, paths and edge labels addressed by the node and edge spans.
    pub text: TextBlob,
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
    /// Node range and raw references of each indexed file, for [`refs::link`].
    file_refs: Vec<FileRefs>,
    text: TextBlob,
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
        file_refs: Vec::new(),
        text: TextBlob::default(),
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
    visit_dir(args, languages.as_deref(), &RelativePath::root(), &mut state)?;
    refs::link(&state.nodes, &mut state.edges, &mut state.text, &state.file_refs);
    Ok(finish(args, state))
}

fn visit_dir(args: &WalkArgs<'_>, languages: Option<&[String]>, dir: &RelativePath, state: &mut State) -> Result<(), DctxError> {
    checkpoint(args)?;
    let mut entries = args.source.entries(dir).map_err(DctxError::from_core)?;
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    for entry in entries {
        checkpoint(args)?;
        // A name the confined path cannot carry (a backslash, or a path over the byte cap) is skipped, as the pack
        // scan does, and marks the snapshot partial. One such file never fails the whole walk.
        let Ok(path) = dir.child(&entry.name) else {
            state.partial = true;
            continue;
        };
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
            visit_file(args, &path, decision, state)?;
        }
    }
    Ok(())
}

fn visit_file(args: &WalkArgs<'_>, path: &RelativePath, decision: FileHandlingDecision, state: &mut State) -> Result<(), DctxError> {
    // The stamp is taken with the stat, before any byte is read: an edit after the stat changes it.
    let (meta, stamp) = match args.source.stat(path).map_err(DctxError::from_core) {
        Ok(pair) => pair,
        Err(error @ (DctxError::Cancelled | DctxError::ResourceLimit { .. })) => return Err(error),
        Err(_) => {
            state.partial = true;
            state.files.push(file_row(path.as_str(), decision, None, 0, 0, None));
            return Ok(());
        }
    };
    let readable = decision == FileHandlingDecision::Include
        && args.freshness != FreshnessMode::Metadata
        && ancestors_include(|item, is_dir| args.policy.decide(item, is_dir), path.as_str())?;
    if !readable {
        if args.freshness == FreshnessMode::Metadata {
            push_module(&mut state.nodes, &mut state.text, path.as_str(), 0, [1, 1]);
        }
        state.files.push(file_row(path.as_str(), decision, None, meta.size, meta.modified.millis(), stamp));
        return Ok(());
    }
    if meta.size > args.limits.file_bytes {
        state.partial = true;
        state.files.push(file_row(path.as_str(), decision, None, meta.size, meta.modified.millis(), stamp));
        return Ok(());
    }
    if state.source_bytes.saturating_add(meta.size) > args.limits.source_bytes {
        return Err(limit(Resource::SourceBytes, args.limits.source_bytes));
    }
    // Drop writes this file before the function returns, so seal and query cannot join its parse.
    let mut watch = FileWatch::begin(path.as_str());
    let read_mark = profile::start();
    let read = read_file(args, path, state);
    watch.charge(Stage::Read, read_mark);
    let bytes = match read {
        Ok(bytes) => bytes,
        Err(DctxError::Cancelled) => return Err(DctxError::Cancelled),
        Err(DctxError::ResourceLimit { resource, limit }) => return Err(DctxError::ResourceLimit { resource, limit }),
        Err(_) => {
            state.partial = true;
            state.files.push(file_row(path.as_str(), decision, None, meta.size, meta.modified.millis(), stamp));
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
    let module = push_module(&mut state.nodes, &mut state.text, path.as_str(), len, [1, lines]);
    if let Some((module_id, path_span)) = module {
        // The export list rides on the module node, so `map` orders an outline without a second read (DCX-55c).
        state.nodes[module_id as usize].signature = super::outline_order::export_signature(path.as_str(), &bytes);
        let parse_mark = profile::start();
        let parsed = prepare_file(args, path.as_str(), &bytes);
        watch.charge(Stage::ParseLex, parse_mark);
        let (prepared, syntax, raw_refs) = parsed?;
        if syntax {
            state.syntax_files += 1;
        } else if args.parser != ParserChoice::Opaque {
            state.other_files += 1;
        }
        let graph_mark = profile::start();
        let count = index_prepared(
            path.as_str(),
            &bytes,
            path_span,
            module_id,
            prepared,
            &mut state.nodes,
            &mut state.edges,
            &mut state.text,
        );
        watch.set_symbols(u64::try_from(count).unwrap_or(u64::MAX));
        // Files without references stay in the list: a call elsewhere may resolve to their symbols.
        if let Some(end) = u32::try_from(state.nodes.len()).ok().filter(|end| !raw_refs.is_empty() || *end > module_id.saturating_add(1)) {
            state.file_refs.push(FileRefs { first: module_id, end, refs: raw_refs });
        }
        watch.charge(Stage::GraphAssembly, graph_mark);
    }
    state.files.push(file_row(path.as_str(), decision, Some(digest), meta.size, meta.modified.millis(), stamp));
    Ok(())
}

fn read_file(args: &WalkArgs<'_>, path: &RelativePath, state: &mut State) -> Result<Vec<u8>, DctxError> {
    checkpoint(args)?;
    let grant = ReadGrant::authorize(path, FileHandlingDecision::Include, true).ok_or(DctxError::PolicyDenied)?;
    state.reads += 1;
    args.source.read(&grant, args.limits.file_bytes).map_err(DctxError::from_core)
}

fn prepare_file(args: &WalkArgs<'_>, path: &str, bytes: &[u8]) -> Result<(Prepared, bool, Vec<RawRef>), DctxError> {
    if bytes.contains(&0) || args.parser == ParserChoice::Opaque || language_of(path) == "opaque" {
        return Ok((Prepared::Symbols(Vec::new()), false, Vec::new()));
    }
    if args.parser == ParserChoice::Syn && language_of(path) == "rust" {
        let (symbols, raw_refs, syntax) = extract_syn(path, bytes, args.limits.file_bytes)?;
        return Ok((Prepared::Symbols(symbols), syntax, raw_refs));
    }
    let (prepared, raw_refs) = prepare_structural(path, bytes);
    Ok((prepared, false, raw_refs))
}

fn extract_syn(path: &str, bytes: &[u8], max_file: u64) -> Result<(Vec<Symbol>, Vec<RawRef>, bool), DctxError> {
    #[cfg(feature = "syntax-rust")]
    {
        super::syn_extract::extract(path, bytes, max_file)
    }
    #[cfg(not(feature = "syntax-rust"))]
    {
        let _ = (path, bytes, max_file);
        Err(DctxError::unsupported("syntax-rust"))
    }
}

fn push_module(
    nodes: &mut Vec<IndexedNode>,
    text: &mut TextBlob,
    path: &str,
    len: u64,
    lines: [u32; 2],
) -> Option<(u32, TextSpan)> {
    StableId::new(NodeKind::Module, path, None, None).ok()?;
    let module_index = u32::try_from(nodes.len()).ok()?;
    let path_span = text.push(path);
    nodes.push(IndexedNode {
        letter: b'm',
        ordinal: 0,
        kind: "module".to_string(),
        name: String::new(),
        path: path_span,
        lines,
        span: [0, len],
        summary: String::new(),
        summary_source: String::new(),
        signature: String::new(),
    });
    Some((module_index, path_span))
}

fn file_row(
    path: &str,
    decision: FileHandlingDecision,
    digest: Option<dctx_hash::ContentDigest>,
    size: u64,
    modified: u64,
    stamp: Option<dctx_core::facts::EntryStamp>,
) -> IndexedFile {
    IndexedFile { path: path.to_string(), decision, digest, size, modified, stamp }
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
        text: state.text,
        coverage: coverage.to_string(),
        completion: completion.to_string(),
        parser: parser.to_string(),
        reads: state.reads,
    }
}

fn language_ok(languages: Option<&[String]>, path: &str) -> bool {
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

#[allow(clippy::naive_bytecount, reason = "Line counting uses standard iterators over the already bounded source buffer without another dependency.")]
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
mod tests;
