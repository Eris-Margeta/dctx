//! Find for a dotted config key (DCX-56c): `find server.port` lists the config symbol that defines the key, then the code
//! lines that read it, then the other matches. Read lines come from the same verified literal search as the find fallback.
//! Depends on [`crate::index::dotted_key`] for the query shape, the definitions and the read test.

use std::collections::{BTreeMap, HashSet};

use super::grep_op::text_fallback_applies;
use super::Session;
use crate::domain::{AnswerRecord, DctxError, MapRequest};
use crate::index::{dotted_key, Generation};

/// Most read lines kept from one file.
const READS_PER_FILE: usize = 8;

impl Session {
    /// Find records for a dotted key with `segments`: the defining config symbols first ([`dotted_key::definitions`]), then the
    /// literal hits of the last segment whose line [`dotted_key::reads`] accepts (code files before tests before docs, at most 8
    /// per file, config files excluded), then the other `records`, each id once. The `bool` is true when no symbol is listed.
    /// A key that no config symbol of the index defines (`find os.path`, `find Gamma.method`) keeps the plain find:
    /// [`Self::find_literal`]. Definitions outside `--in` or the kind filter are dropped; the key still counts as defined.
    pub(super) fn find_dotted(&mut self, generation: &Generation, request: &MapRequest, records: Vec<AnswerRecord>, segments: &[&str]) -> Result<(Vec<AnswerRecord>, bool), DctxError> {
        let definitions = dotted_key::definitions(generation, segments);
        if definitions.is_empty() {
            return self.find_literal(generation, request, records);
        }
        let mut reads = Vec::new();
        // A kind filter that excludes `text` keeps the read lines out, as it keeps out the plain find fallback.
        if let Some(last) = segments.last()
            && text_fallback_applies(request)
        {
            let mut literal = request.clone();
            literal.query = (*last).to_string();
            reads = read_sites(self.text_hits(generation, &literal)?, segments);
        }
        let definitions: Vec<AnswerRecord> = definitions.into_iter().filter(|record| admits(request, record)).collect();
        let text_only = definitions.is_empty() && records.is_empty() && !reads.is_empty();
        Ok((each_id_once(definitions.into_iter().chain(reads).chain(records)), text_only))
    }
}

/// The lowercase extension of the last path component of `path`, if it has one.
fn extension(path: &str) -> Option<String> {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase())
}

/// True for a YAML, TOML or JSON file (the `config` tag of `index::extract::language_of`, which the session cannot call).
fn is_config(path: &str) -> bool {
    extension(path).is_some_and(|extension| matches!(extension.as_str(), "yaml" | "yml" | "toml" | "json" | "jsonc"))
}

/// Place rank of a read line: 0 for code, 1 for a path containing `test`, 2 for a `.md`, `.rst` or `.txt` doc.
fn place_rank(path: &str) -> u8 {
    if extension(path).is_some_and(|extension| matches!(extension.as_str(), "md" | "rst" | "txt")) {
        2
    } else if path.to_ascii_lowercase().contains("test") {
        1
    } else {
        0
    }
}

/// The literal `hits` whose trimmed line (`summary`) reads the key, config files excluded, at most [`READS_PER_FILE`] per
/// file, ordered by [`place_rank`] and otherwise kept in hit order.
fn read_sites(hits: Vec<AnswerRecord>, segments: &[&str]) -> Vec<AnswerRecord> {
    let mut per_file: BTreeMap<String, usize> = BTreeMap::new();
    let mut kept = Vec::new();
    for hit in hits {
        if is_config(&hit.path) || !dotted_key::reads(&hit.summary, segments) {
            continue;
        }
        let count = per_file.entry(hit.path.clone()).or_insert(0);
        if *count == READS_PER_FILE {
            continue;
        }
        *count += 1;
        kept.push(hit);
    }
    kept.sort_by_key(|hit| place_rank(&hit.path));
    kept
}

/// True when `record` passes the kind filter and the `--in` path prefix of `request`, as `find` applies them to symbols.
fn admits(request: &MapRequest, record: &AnswerRecord) -> bool {
    let kind = request.kinds.is_empty() || request.kinds.iter().any(|kind| *kind == record.kind);
    let place = request.within.as_deref().is_none_or(|prefix| {
        let prefix = prefix.trim_end_matches('/');
        prefix == "." || record.path == prefix || record.path.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))
    });
    kind && place
}

/// `records` in order with every later record whose id was already listed dropped.
fn each_id_once(records: impl IntoIterator<Item = AnswerRecord>) -> Vec<AnswerRecord> {
    let mut seen = HashSet::new();
    records.into_iter().filter(|record| seen.insert(record.id.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Verb;

    fn hit(path: &str, line: u32, summary: &str) -> AnswerRecord {
        let mut record = AnswerRecord::basic(format!("text:{path}:{line}"), "text", path);
        record.lines = [line, line];
        record.summary = summary.to_string();
        record
    }

    #[test]
    fn read_sites_keep_code_then_tests_then_docs_and_cap_each_file() {
        let mut hits = vec![
            hit("README.md", 1, "read cfg[\"server\"][\"port\"]"),
            hit("config/app.json", 2, "\"server.port\": 1"),
            hit("src/main.py", 4, "port = cfg[\"server\"][\"port\"]"),
            hit("src/main.py", 6, "return port"),
            hit("tests/test_main.py", 3, "'server.port': 8080,"),
        ];
        for line in 10..20 {
            hits.push(hit("src/many.py", line, "p = settings.server.port"));
        }
        let kept = read_sites(hits, &["server", "port"]);
        let ids: Vec<&str> = kept.iter().map(|record| record.id.as_str()).collect();
        assert_eq!(ids.len(), 11, "{ids:?}");
        assert_eq!(ids[0], "text:src/main.py:4");
        assert_eq!(ids[1], "text:src/many.py:10");
        assert_eq!(ids[8], "text:src/many.py:17");
        assert_eq!(ids[9], "text:tests/test_main.py:3");
        assert_eq!(ids[10], "text:README.md:1");
    }

    #[test]
    fn definitions_follow_the_find_filters_and_ids_are_listed_once() {
        let mut request = MapRequest::new(Verb::Find);
        let yaml = hit("config/app.yaml", 3, "port: 8080");
        assert!(admits(&request, &yaml));
        request.within = Some("config/".to_string());
        assert!(admits(&request, &yaml));
        request.within = Some("src".to_string());
        assert!(!admits(&request, &yaml));
        request.within = None;
        request.kinds = vec!["fn".to_string()];
        assert!(!admits(&request, &yaml));
        let merged = each_id_once([yaml.clone(), hit("src/a.py", 1, "x"), yaml]);
        assert_eq!(merged.len(), 2);
        assert!(is_config("config/APP.YML") && !is_config("src/config.py"));
    }
}
