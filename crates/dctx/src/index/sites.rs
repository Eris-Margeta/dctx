//! Call-site lines for inbound `calls` records of a neighbors answer.
//! Implements the `sites` field: the 1-based absolute lines inside a caller's span where the callee's last name segment occurs as a call.
//! Depends on the caller's verified bytes, which the session reads; this module only plans the reads and scans the text.
//! At most [`MAX_SITES`] lines per record, ascending, and never the caller's own declaration line.
//! [`rank`] puts the direct callers with the most call-site lines first, so a budget-cut first page covers the most call sites.

use super::extract::language_of;
use super::{query, Generation};
use crate::domain::{AnswerRecord, MapRequest};

/// Most call-site lines kept on one record.
pub(crate) const MAX_SITES: usize = 3;

/// One caller record whose span must be scanned.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SiteJob {
    /// Position of the record in the page.
    pub index: usize,
    /// Root-relative path of the caller's file.
    pub path: String,
    /// Byte span `[start, end)` of the caller in that file.
    pub span: [u64; 2],
    /// Absolute line of the caller's first line, its declaration.
    pub first: u32,
}

/// The callee name to look for and the caller records to scan.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SitePlan {
    /// Last name segment of the requested node.
    pub name: String,
    /// One job per depth-one inbound `calls` record.
    pub jobs: Vec<SiteJob>,
}

/// Plans the scans for `records`, or `None` when the page has no depth-one inbound `calls` record or the target has no plain name.
pub(crate) fn plan(generation: &Generation, request: &MapRequest, records: &[AnswerRecord]) -> Option<SitePlan> {
    let root = query::lookup(&generation.nodes, &generation.text, &request.target)?;
    let name = root.name.rsplit(['.', ':', '#']).next().unwrap_or("");
    if name.is_empty() || !name.chars().all(is_word) {
        return None;
    }
    let mut jobs = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let (Some("calls"), Some("in"), Some(target)) = (record.edge.as_deref(), record.direction.as_deref(), record.target.as_deref()) else {
            continue;
        };
        // A deeper record calls an intermediate node, not the requested one.
        if record.id != format!("calls:in:{target}") {
            continue;
        }
        let Some(caller) = query::lookup(&generation.nodes, &generation.text, target) else {
            continue;
        };
        jobs.push(SiteJob { index, path: record.path.clone(), span: caller.span, first: caller.lines[0] });
    }
    (!jobs.is_empty()).then(|| SitePlan { name: name.to_string(), jobs })
}

/// True for a depth-one neighbors record: its id is `{edge}:{direction}:{target}` with no depth in between.
fn depth_one(record: &AnswerRecord) -> bool {
    let (Some(edge), Some(direction), Some(target)) = (record.edge.as_deref(), record.direction.as_deref(), record.target.as_deref()) else {
        return false;
    };
    record.id.strip_prefix(edge).and_then(|rest| rest.strip_prefix(':')).and_then(|rest| rest.strip_prefix(direction)).and_then(|rest| rest.strip_prefix(':')) == Some(target)
}

/// Stably reorders the leading depth-one records so that, inside one edge kind, a record with more call-site lines (`counts[i]` for record `i`) comes first.
/// Ties keep their earlier order (target, then id). Deeper records stay where they are, after every depth-one record.
pub(crate) fn rank(records: &mut Vec<AnswerRecord>, counts: &[usize]) {
    let head = records.iter().take_while(|record| depth_one(record)).count();
    let count = |index: usize| counts.get(index).copied().unwrap_or(0);
    let mut order: Vec<usize> = (0..head).collect();
    order.sort_by(|&left, &right| records[left].edge.cmp(&records[right].edge).then(count(right).cmp(&count(left))));
    if order.iter().enumerate().all(|(position, &index)| position == index) {
        return;
    }
    let ranked: Vec<AnswerRecord> = order.iter().map(|&index| records[index].clone()).collect();
    records.splice(..head, ranked);
}

/// Every line of the caller's span, from `bytes` of its file, where `name` is called, ascending. Empty when the span is not valid UTF-8 text.
/// The page shows the first [`MAX_SITES`]; the full count orders the callers.
pub(crate) fn scan(bytes: &[u8], job: &SiteJob, name: &str) -> Vec<u32> {
    let (Ok(start), Ok(end)) = (usize::try_from(job.span[0]), usize::try_from(job.span[1])) else {
        return Vec::new();
    };
    let Some(text) = bytes.get(start..end).and_then(|slice| std::str::from_utf8(slice).ok()) else {
        return Vec::new();
    };
    let tokens = matches!(language_of(&job.path), "ocaml" | "haskell" | "shell");
    let mut lines = Vec::new();
    for (offset, line) in text.split('\n').enumerate().skip(1) {
        if has_call(line, name, tokens) {
            lines.push(job.first.saturating_add(u32::try_from(offset).unwrap_or(u32::MAX)));
        }
    }
    lines
}

/// True for a character that can continue a name.
fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '\''
}

/// True when `name` occurs in `line` as a call: followed by optional blanks and `(` or `!(`. With `tokens` any whole-word occurrence counts.
fn has_call(line: &str, name: &str, tokens: bool) -> bool {
    line.match_indices(name).any(|(at, _)| {
        if line[..at].chars().next_back().is_some_and(is_word) {
            return false;
        }
        let rest = &line[at + name.len()..];
        if tokens {
            return !rest.chars().next().is_some_and(is_word);
        }
        let rest = rest.trim_start_matches([' ', '\t']);
        rest.starts_with('(') || rest.starts_with("!(")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A job over the whole of `source`, whose first line is line `first`.
    fn job(path: &str, source: &str, first: u32) -> SiteJob {
        SiteJob { index: 0, path: path.to_string(), span: [0, u64::try_from(source.len()).unwrap_or(0)], first }
    }

    #[test]
    fn calls_are_found_by_name_and_open_paren_and_the_declaration_is_skipped() {
        let source = "def caller():\n    x = 1\n    y = target()\n    return target () + y\n    z = retarget(1) + target_b(2) + target\n    m = target!(1)\n    n = obj.target(2)\n";
        assert_eq!(scan(source.as_bytes(), &job("a.py", source, 4), "target"), [6, 7, 9, 10]);
        let own = "fn target() {\n    target();\n}\n";
        assert_eq!(scan(own.as_bytes(), &job("a.rs", own, 10), "target"), [11]);
        let many = "f\n target(\n target(\n target(\n target(\n";
        assert_eq!(scan(many.as_bytes(), &job("a.rs", many, 1), "target"), [2, 3, 4, 5]);
    }

    /// A depth-one record `{edge}:{direction}:{target}`, or a depth-two one when `level` is 2.
    fn record(edge: &str, direction: &str, target: &str, level: u32) -> AnswerRecord {
        let id = if level > 1 { format!("{edge}:{direction}:{level}:{target}") } else { format!("{edge}:{direction}:{target}") };
        let mut record = AnswerRecord::basic(id, "fn", "a.rs");
        record.edge = Some(edge.to_string());
        record.direction = Some(direction.to_string());
        record.target = Some(target.to_string());
        record
    }

    #[test]
    fn rank_puts_callers_with_more_sites_first_and_keeps_ties_and_deeper_records() {
        let mut records = vec![
            record("calls", "in", "s:a.rs#a", 1),
            record("calls", "in", "s:a.rs#b", 1),
            record("calls", "in", "s:a.rs#c", 1),
            record("calls", "in", "s:a.rs#d", 1),
            record("imports", "in", "m:b.rs", 1),
            record("calls", "in", "s:a.rs#e", 2),
        ];
        rank(&mut records, &[1, 5, 0, 1, 9, 7]);
        let ids: Vec<&str> = records.iter().map(|record| record.id.as_str()).collect();
        assert_eq!(ids, ["calls:in:s:a.rs#b", "calls:in:s:a.rs#a", "calls:in:s:a.rs#d", "calls:in:s:a.rs#c", "imports:in:m:b.rs", "calls:in:2:s:a.rs#e"]);
        let mut same = records.clone();
        rank(&mut same, &[0; 6]);
        assert_eq!(same, records, "equal counts keep the order");
    }

    #[test]
    fn ocaml_haskell_and_shell_count_whole_word_tokens() {
        let source = "let caller x =\n  let y = target x in\n  retarget y\n  target\n";
        assert_eq!(scan(source.as_bytes(), &job("a.ml", source, 1), "target"), [2, 4]);
        assert!(scan(source.as_bytes(), &job("a.py", source, 1), "target").is_empty());
        let shell = "caller() {\n  target \"$1\"\n}\n";
        assert_eq!(scan(shell.as_bytes(), &job("run.sh", shell, 5), "target"), [6]);
    }

    #[test]
    fn a_span_outside_the_bytes_or_not_text_yields_no_sites() {
        let bad = SiteJob { index: 0, path: "a.py".to_string(), span: [0, 99], first: 1 };
        assert!(scan(b"x", &bad, "target").is_empty());
        assert!(scan(&[0xff, 0xfe, b'\n', 0xff], &SiteJob { span: [0, 4], ..bad }, "target").is_empty());
    }
}
