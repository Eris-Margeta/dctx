//! JSON and Markdown baseline reports for DX-A1.
//! Implements ARCHITECTURE-PROPOSAL §7 Stage 0 evidence, without a pass/fail threshold.
//! Depends on serde; `run` writes `reports/baseline.json` and `reports/baseline.md`.

use crate::error::BenchError;
use crate::host::HostInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

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

/// Markdown summary. Full verb JSON stays in the JSON report.
pub fn markdown(report: &BaselineReport) -> String {
    let mut out = String::new();
    out.push_str("# DX-A1 baseline (current map mode)\n\n");
    out.push_str("Measured for ARCHITECTURE-PROPOSAL §3 and §7 Stage 0.\n");
    out.push_str(&format!("Implementation: {}.\n", report.implementation));
    out.push_str(
        "This report records observations. It does not assert a performance threshold.\n\n",
    );
    out.push_str("| Corpus | Included files | Cold index | Warm p50 | Warm p95 | RSS | Cache bytes | Cache / 1,000 files | Parse calls | Stale |\n");
    out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
    for corpus in &report.corpora {
        let density = corpus
            .cache_bytes_per_1000_files
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".into());
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            corpus.id,
            corpus.included_files,
            ms(corpus.cold_index_us),
            ms(corpus.warm_p50_us),
            ms(corpus.warm_p95_us),
            corpus.rss_bytes,
            corpus.cache_bytes,
            density,
            corpus.parse_calls,
            corpus.stale_files,
        ));
    }
    out.push_str("\nTimes are milliseconds. RSS and cache sizes are bytes.\n\n");
    out.push_str("## Pins\n\n");
    for corpus in &report.corpora {
        out.push_str(&format!(
            "- `{}`: kind `{}`, revision `{}`, sha256 `{}`, files {}, bytes {}.\n",
            corpus.id,
            corpus.pin.kind,
            corpus.pin.revision.as_deref().unwrap_or("none"),
            corpus.pin.content_sha256,
            corpus.pin.file_count,
            corpus.pin.byte_count,
        ));
        if let Some(generator) = &corpus.pin.generator {
            out.push_str(&format!(
                "  generator `{generator}`, seed `{}`.\n",
                corpus.pin.seed.as_deref().unwrap_or("none")
            ));
        }
    }
    out.push_str("\n## Five verbs\n\n");
    for corpus in &report.corpora {
        out.push_str(&format!("### {}\n\n", corpus.id));
        for verb in &corpus.verbs {
            if verb.ok {
                let records = verb
                    .answer
                    .as_ref()
                    .and_then(|value| value.get("records"))
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0);
                let first = verb
                    .answer
                    .as_ref()
                    .and_then(first_id)
                    .unwrap_or_else(|| "none".into());
                out.push_str(&format!(
                    "- `{}` target `{}`: {records} records, first id `{first}`.\n",
                    verb.verb, verb.target
                ));
            } else {
                out.push_str(&format!(
                    "- `{}` target `{}`: refused: {}.\n",
                    verb.verb,
                    verb.target,
                    verb.error.as_deref().unwrap_or("unknown")
                ));
            }
        }
        out.push('\n');
    }
    out.push_str("## Precision and recall\n\n");
    out.push_str("Not measured. The current map mode does not ship an adjudicated declaration corpus, and bounded verb answers are not a gold set.\n\n");
    out.push_str("## Notes\n\n");
    for note in &report.notes {
        out.push_str(&format!("- {note}\n"));
    }
    out.push('\n');
    out
}

fn first_id(value: &Value) -> Option<String> {
    value
        .get("records")?
        .as_array()?
        .iter()
        .find_map(|record| record.get("id")?.as_str().map(str::to_owned))
}

fn ms(us: u64) -> String {
    format!("{}.{:03}", us / 1000, us % 1000)
}

/// Writes pretty JSON.
pub fn to_json(value: &impl Serialize) -> Result<Vec<u8>, BenchError> {
    Ok(serde_json::to_vec_pretty(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::HostInfo;

    fn corpus(id: &str) -> CorpusReport {
        CorpusReport {
            schema: "dx-a1-corpus-report-v1".into(),
            id: id.into(),
            measured_at_unix: 0,
            host: HostInfo {
                os: "test".into(),
                arch: "test".into(),
                parallelism: 1,
                rustc: "unavailable".into(),
                profile: "debug".into(),
            },
            pin: Pin {
                kind: "generated".into(),
                revision: None,
                content_sha256: "abc".into(),
                generator: Some("dx-a1-fixture-v1".into()),
                seed: Some("seed".into()),
                file_count: 1,
                byte_count: 4,
                symlink_policy: "not-followed".into(),
            },
            cold_index_us: 1500,
            cold_phases_us: BTreeMap::new(),
            warm_query_us: vec![10, 20],
            warm_p50_us: 10,
            warm_p95_us: 20,
            warm_percentile: "ceil".into(),
            warm_terms: vec!["parse".into()],
            rss_bytes: 100,
            rss_source: "proc-vmhwm".into(),
            cache_bytes: 50,
            cache_files: vec![],
            cache_bytes_per_1000_files: Some(50_000),
            parse_calls: 1,
            stale_files: 0,
            stale_sample: vec![],
            included_files: 1,
            verbs: vec![VerbAnswer {
                verb: "map".into(),
                target: String::new(),
                ok: true,
                answer: Some(serde_json::json!({"records": [{"id": "m:."}]})),
                error: None,
            }],
            precision_recall: PrecisionRecall {
                status: "not_measured".into(),
                reason: "no gold set".into(),
            },
        }
    }

    #[test]
    fn summary_states_no_threshold_and_roundtrips() {
        let mut observed = corpus("dctx");
        observed.pin.revision = Some("release/next".into());
        let report = BaselineReport::new(vec![observed]);
        assert!(report.no_performance_assertion);
        let text = markdown(&report);
        assert!(text.contains("does not assert a performance threshold"));
        assert!(text.contains("1.500"));
        assert!(text.contains("revision `release/next`"));
        assert!(text.contains("`m:.`"));
        let bytes = to_json(&report).unwrap();
        let parsed: BaselineReport = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.corpora[0].id, "dctx");
        assert_eq!(parsed.corpora[0].pin.revision.as_deref(), Some("release/next"));
        assert!(BaselineReport::new(vec![]).corpora.is_empty());
    }
}
