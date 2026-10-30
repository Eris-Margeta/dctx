//! Literal search over the files of one generation, grouped by the innermost enclosing symbol.
//! Implements the `dctx map grep` verb: no regex, ASCII case folding on request, one record per symbol.
//! Depends on the in-memory generation and an injected verified reader. It never opens a file itself.
//! Only files the walk read and hashed are searched, and secret-shaped names (`.env`, `*.env`) never are.

use std::collections::BTreeMap;

use dctx_core::filter::is_secret_shaped_name;
use dctx_core::FileHandlingDecision;
use dctx_hash::ContentDigest;

use super::query::record_of;
use super::{Generation, IndexedNode};
use crate::domain::{hit_row, AnswerRecord, DctxError, MapRequest};

/// True when `line` contains `needle`; ASCII letters fold when `ignore_case` is set.
fn line_has(line: &str, needle: &str, ignore_case: bool) -> bool {
    if !ignore_case {
        return line.contains(needle);
    }
    let pattern = needle.as_bytes();
    line.as_bytes().windows(pattern.len()).any(|window| window.eq_ignore_ascii_case(pattern))
}

/// True when `path` is `prefix` or lies below it. `.` admits every path.
fn inside(path: &str, prefix: Option<&str>) -> bool {
    let Some(prefix) = prefix else {
        return true;
    };
    let prefix = prefix.trim_end_matches('/');
    prefix == "." || path == prefix || path.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))
}

/// Position in `nodes` of the narrowest node whose lines hold `line`. The later node wins a tie.
fn innermost(nodes: &[&IndexedNode], line: u32) -> Option<usize> {
    let mut best: Option<(usize, u32)> = None;
    for (index, node) in nodes.iter().enumerate() {
        let [first, last] = node.lines;
        if first > line || line > last {
            continue;
        }
        let size = last - first;
        if best.is_none_or(|(_, smallest)| size <= smallest) {
            best = Some((index, size));
        }
    }
    best.map(|(index, _)| index)
}

/// One record per symbol that holds a hit, ordered by path and then by its first hit line.
///
/// `read` returns the verified bytes of a path, or `None` to skip a file the policy no longer admits.
/// Each record carries its hit rows in `source_utf8`.
pub(crate) fn grep_records(
    generation: &Generation,
    request: &MapRequest,
    mut read: impl FnMut(&str, ContentDigest) -> Result<Option<Vec<u8>>, DctxError>,
) -> Result<Vec<AnswerRecord>, DctxError> {
    let needle = request.query.as_str();
    if needle.is_empty() {
        return Err(DctxError::invalid("grep requires a literal"));
    }
    let text = &generation.text;
    let mut by_path: BTreeMap<&str, Vec<&IndexedNode>> = BTreeMap::new();
    for node in &generation.nodes {
        by_path.entry(text.get(node.path)).or_default().push(node);
    }
    let mut groups: Vec<(String, u32, AnswerRecord)> = Vec::new();
    for file in &generation.files {
        let name = file.path.rsplit('/').next().unwrap_or("");
        let Some(digest) = file.digest else {
            continue;
        };
        if file.decision != FileHandlingDecision::Include
            || is_secret_shaped_name(name)
            || !inside(&file.path, request.within.as_deref())
        {
            continue;
        }
        let Some(bytes) = read(&file.path, digest)? else {
            continue;
        };
        let Ok(source) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let nodes = by_path.get(file.path.as_str()).map_or(&[][..], Vec::as_slice);
        // `None` is the file itself, for a hit that no indexed node holds.
        let mut hits: BTreeMap<Option<usize>, (u32, Vec<String>)> = BTreeMap::new();
        for (index, line) in source.split('\n').enumerate() {
            let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
            if !line_has(line, needle, request.ignore_case) {
                continue;
            }
            let entry = hits.entry(innermost(nodes, number)).or_insert_with(|| (number, Vec::new()));
            entry.1.push(hit_row(number, line));
        }
        for (owner, (first, rows)) in hits {
            let mut record = match owner.and_then(|index| nodes.get(index)) {
                Some(node) => record_of(text, node),
                None => {
                    let mut module = AnswerRecord::basic(format!("m:{}", file.path), "module", file.path.as_str());
                    module.lines = [1, u32::try_from(source.split('\n').count()).unwrap_or(u32::MAX)];
                    module
                }
            };
            record.source_utf8 = Some(rows.join("\n"));
            groups.push((file.path.clone(), first, record));
        }
    }
    groups.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)).then(left.2.id.cmp(&right.2.id)));
    Ok(groups.into_iter().map(|(_, _, record)| record).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{encode_text, Answer, Freshness, FreshnessMode, Precision, Verb};
    use crate::index::{IndexedFile, TextBlob};

    fn node(text: &mut TextBlob, kind: &str, name: &str, lines: [u32; 2]) -> IndexedNode {
        IndexedNode {
            letter: if kind == "module" { b'm' } else { b's' },
            ordinal: 0,
            kind: kind.to_string(),
            name: name.to_string(),
            path: text.push("a.py"),
            lines,
            span: [0, 0],
            summary: String::new(),
            summary_source: String::new(),
            signature: String::new(),
        }
    }

    fn fixture() -> Generation {
        let mut text = TextBlob::default();
        let nodes = vec![
            node(&mut text, "module", "", [1, 6]),
            node(&mut text, "fn", "alpha", [1, 3]),
            node(&mut text, "fn", "beta", [5, 6]),
        ];
        let digest = crate::domain::digest_text(b"x").expect("digest");
        let file = |path: &str, decision| IndexedFile { path: path.to_string(), decision, digest: Some(digest), size: 1, modified: 0 };
        Generation {
            id: 1,
            digest: "v1".to_string(),
            encoded_len: 0,
            files: vec![
                file("a.py", FileHandlingDecision::Include),
                file(".env", FileHandlingDecision::Include),
                file("skip.py", FileHandlingDecision::OmitContent),
            ],
            nodes,
            edges: Vec::new(),
            text,
            freshness: FreshnessMode::Strict,
            taken_at: 0,
            epoch: 0,
            coverage: "structural".to_string(),
            completion: "complete".to_string(),
            parser: "p".to_string(),
        }
    }

    const SOURCE: &str = "def alpha():\n    x = \"NEEDLE one\"\n    return x\n\ndef beta():\n    return \"needle two\"\n";

    #[test]
    fn hits_group_by_the_innermost_symbol_and_skip_omitted_and_env_files() {
        let generation = fixture();
        let mut asked = Vec::new();
        let mut request = MapRequest::new(Verb::Grep);
        request.query = "NEEDLE".to_string();
        let records = grep_records(&generation, &request, |path, _| {
            asked.push(path.to_string());
            Ok(Some(SOURCE.as_bytes().to_vec()))
        })
        .expect("grep");
        assert_eq!(asked, vec!["a.py".to_string()], "only included, non-env files are read");
        let ids: Vec<&str> = records.iter().map(|record| record.id.as_str()).collect();
        assert_eq!(ids, vec!["s:a.py#alpha"]);
        assert_eq!(records[0].source_utf8.as_deref(), Some("L2: x = \"NEEDLE one\""));

        request.ignore_case = true;
        let records = grep_records(&generation, &request, |_, _| Ok(Some(SOURCE.as_bytes().to_vec()))).expect("grep -i");
        let ids: Vec<&str> = records.iter().map(|record| record.id.as_str()).collect();
        assert_eq!(ids, vec!["s:a.py#alpha", "s:a.py#beta"]);
        request.within = Some("nowhere".to_string());
        assert!(grep_records(&generation, &request, |_, _| Ok(Some(SOURCE.as_bytes().to_vec()))).expect("in").is_empty());

        let answer = Answer {
            generation: 1,
            snapshot: String::new(),
            verb: Verb::Grep,
            records,
            more: 0,
            next: None,
            freshness: Freshness { mode: "strict".to_string(), state: "verified".to_string(), age_s: 0 },
            precision: Precision { coverage: "structural".to_string(), completion: "complete".to_string(), parser: "p".to_string() },
            outcome: None,
        };
        let page = encode_text("p", &answer, false);
        assert_eq!(
            page,
            "# p grep\ns:a.py#alpha\tfn\ta.py\n  L2: x = \"NEEDLE one\"\ns:a.py#beta\tfn\ta.py\n  L6: return \"needle two\"\n2 hits in 2 symbols\n"
        );
    }
}
