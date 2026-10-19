//! JSON and Markdown baseline reports for DX-A1.
//! Implements ARCHITECTURE-PROPOSAL §7 Stage 0 evidence, without a pass/fail threshold.
//! Depends on serde; `run` writes `reports/baseline.json` and `reports/baseline.md`.

use crate::host::HostInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

mod render;

pub use render::{markdown, to_json};

/// One corpus observation. Written by a child process and merged by the parent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusReport {
    /// `dx-a1-corpus-report-v1`.
    pub schema: String,
    /// Corpus id from the manifest.
    pub id: String,
    /// Unix seconds when this process finished measuring.
    pub measured_at_unix: u64,
    /// Host facts for this process.
    pub host: HostInfo,
    /// How the bytes were pinned.
    pub pin: Pin,
    /// Cold `Index::query` time in microseconds, including open and the map answer.
    pub cold_index_us: u64,
    /// Scan, parse, resolve, and store portions of the cold refresh.
    pub cold_phases_us: BTreeMap<String, u64>,
    /// Thirty resident `Index::answer` find timings, in order.
    pub warm_query_us: Vec<u64>,
    /// Nearest-rank p50 of `warm_query_us`.
    pub warm_p50_us: u64,
    /// Nearest-rank p95 of `warm_query_us`.
    pub warm_p95_us: u64,
    /// Percentile definition.
    pub warm_percentile: String,
    /// The thirty find terms.
    pub warm_terms: Vec<String>,
    /// Process RSS after the queries.
    pub rss_bytes: u64,
    /// `proc-vmhwm` or `ps-rss`.
    pub rss_source: String,
    /// Bytes in the cache directory after the cold index.
    pub cache_bytes: u64,
    /// Direct cache files.
    pub cache_files: Vec<CacheFile>,
    /// `cache_bytes * 1000 / included_files`.
    pub cache_bytes_per_1000_files: Option<u64>,
    /// Files parsed on the cold index.
    pub parse_calls: u64,
    /// Files the cold index marked stale.
    pub stale_files: u64,
    /// Up to twenty stale paths, sorted.
    pub stale_sample: Vec<StaleFile>,
    /// `parse_calls + stale_files` on a cold cache.
    pub included_files: u64,
    /// One answer for each of the five verbs.
    pub verbs: Vec<VerbAnswer>,
    /// Declaration precision and recall are not invented.
    pub precision_recall: PrecisionRecall,
}

/// Content pin stored with a corpus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    /// `worktree` or `generated`.
    pub kind: String,
    /// Caller label from DCTX_BENCH_REVISION; `unversioned` by default for worktrees.
    pub revision: Option<String>,
    /// Aggregate SHA-256.
    pub content_sha256: String,
    /// Generator id for the fixture.
    pub generator: Option<String>,
    /// Fixture seed.
    pub seed: Option<String>,
    /// Files in the content hash.
    pub file_count: u64,
    /// Bytes in the content hash.
    pub byte_count: u64,
    /// Symlink policy for the hash.
    pub symlink_policy: String,
}

/// One cache file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheFile {
    /// File name.
    pub name: String,
    /// Length in bytes.
    pub bytes: u64,
}

/// One stale path kept in the sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleFile {
    /// Place-relative path.
    pub path: String,
    /// Indexer reason.
    pub reason: String,
}

/// A five-verb answer, successful or refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerbAnswer {
    /// `map`, `find`, `neighbors`, `explain`, or `read`.
    pub verb: String,
    /// Target passed to the verb.
    pub target: String,
    /// Whether rendering produced JSON.
    pub ok: bool,
    /// Bounded JSON answer.
    pub answer: Option<Value>,
    /// Refusal text when `ok` is false.
    pub error: Option<String>,
}

/// Explicit absence of an adjudicated precision/recall score.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrecisionRecall {
    /// `not_measured`.
    pub status: String,
    /// Why a number would be invented.
    pub reason: String,
}

/// Merged baseline written to `reports/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineReport {
    /// `dx-a1-baseline-v1`.
    pub schema: String,
    /// `current-map`.
    pub mode: String,
    /// Spec citation.
    pub spec: String,
    /// Always true. The runner never compares timings to a limit.
    pub no_performance_assertion: bool,
    /// Unix seconds when the parent merged the children.
    pub measured_at_unix: u64,
    /// Library path that was measured.
    pub implementation: String,
    /// Reading notes.
    pub notes: Vec<String>,
    /// dctx, then the generated fixture.
    pub corpora: Vec<CorpusReport>,
}

impl BaselineReport {
    /// Merges the two child reports.
    pub fn new(corpora: Vec<CorpusReport>) -> Self {
        Self {
            schema: "dx-a1-baseline-v1".into(),
            mode: "current-map".into(),
            spec: "docs/design/ARCHITECTURE-PROPOSAL.md §3 and §7 Stage 0 (DX-A1)".into(),
            no_performance_assertion: true,
            measured_at_unix: unix_now(),
            implementation: "dctx-map-query Index, SQLite/FTS5, tree-sitter; the library path of dctx map".into(),
            notes: vec![
                "Cold index time is the first Index::open plus Index::query on an empty cache.".into(),
                "Warm p50/p95 are thirty resident Index::answer find calls after that index is built.".into(),
                "Percentile rank is ceil(percent * n / 100) on samples sorted ascending.".into(),
                "cache_bytes_per_1000_files is cache_bytes * 1000 / included_files.".into(),
                "included_files is cold parse_calls plus stale_files.".into(),
                "Process RSS is VmHWM when /proc/self/status exists, otherwise ps rss.".into(),
                "The place label is the corpus id. The CLI labels place with the directory name.".into(),
                "The content hash skips the manifest directories and does not follow symlinks.".into(),
                "Parser precision and recall are not measured; this mode has no adjudicated gold set.".into(),
                "No timing, RSS, or disk figure is compared with a threshold.".into(),
            ],
            corpora,
        }
    }
}

/// Seconds since the Unix epoch. Zero if the clock is before the epoch.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
