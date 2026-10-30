//! Call-site lines for inbound `calls` records of a neighbors answer.
//! Implements the `sites` field: the 1-based absolute lines inside a caller's span where the callee's last name segment occurs as a call.
//! Depends on the caller's verified bytes, which the session reads; this module only plans the reads and scans the text.
//! At most [`MAX_SITES`] lines per record, ascending, and never the caller's own declaration line.

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

/// Lines of the caller's span, from `bytes` of its file, where `name` is called. Empty when the span is not valid UTF-8 text.
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
        if lines.len() == MAX_SITES {
            break;
        }
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
        assert_eq!(scan(source.as_bytes(), &job("a.py", source, 4), "target"), [6, 7, 9]);
        let own = "fn target() {\n    target();\n}\n";
        assert_eq!(scan(own.as_bytes(), &job("a.rs", own, 10), "target"), [11]);
        let many = "f\n target(\n target(\n target(\n target(\n";
        assert_eq!(scan(many.as_bytes(), &job("a.rs", many, 1), "target"), [2, 3, 4]);
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
