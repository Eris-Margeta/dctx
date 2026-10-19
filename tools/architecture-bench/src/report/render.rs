//! JSON serialization and Markdown summaries of baseline reports.
//! Implements ARCHITECTURE-PROPOSAL §7 Stage 0 evidence.
//! Depends on the parent report types, serde and BenchError.
//! Re-exported by report for the benchmark runner.
//! Rendering records observations without performance thresholds.

use super::BaselineReport;
use crate::error::BenchError;
use serde::Serialize;
use serde_json::Value;

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
    use super::super::{CorpusReport, Pin, PrecisionRecall, VerbAnswer};
    use crate::host::HostInfo;
    use std::collections::BTreeMap;

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
