//! Find order for a SCREAMING_CASE query (`HOME`, `DJANGO_SETTINGS_MODULE`): a config key or constant, so the page
//! leads with the places that read it. The best symbol named exactly the key comes first ([`EXACT_FIRST`]), then one read
//! site per code file (the key quoted, after `.`, `$` or `${`, never inside a longer name), code before shell and
//! config, then the other symbol matches, then every other literal hit, code before tests before docs.
//! Pure ranking over records the caller already collected; nothing here reads a file.

use std::collections::HashSet;

use super::extract::language_of;
use crate::domain::AnswerRecord;

/// Exact-name symbols listed ahead of the read sites: the definition, then where it is read.
const EXACT_FIRST: usize = 1;

/// Find score of an exact name (see `query::score_of`).
const EXACT: i64 = 3000;

/// Most literal hits per file collected for a key query, so one noisy file cannot fill the hit cap.
pub(crate) const KEY_HITS_PER_FILE: usize = 8;

/// True for a query of four or more ASCII capitals, digits and `_` that starts with a capital.
pub(crate) fn key_shaped(query: &str) -> bool {
    query.len() >= 4
        && query.starts_with(|ch: char| ch.is_ascii_uppercase())
        && query.chars().all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

/// Merges ranked `symbols` and path-ordered literal `texts` for `key` into one find order, each record once.
pub(crate) fn rank(key: &str, symbols: Vec<AnswerRecord>, texts: Vec<AnswerRecord>) -> Vec<AnswerRecord> {
    let (exact, other): (Vec<_>, Vec<_>) = symbols.into_iter().enumerate().partition(|(index, record)| *index < EXACT_FIRST && record.score >= EXACT);
    let mut out: Vec<AnswerRecord> = exact.into_iter().map(|(_, record)| record).collect();
    let mut files: HashSet<String> = HashSet::new();
    let mut reads = Vec::new();
    let mut rest = Vec::new();
    for record in texts {
        let class = class_of(&record.path);
        if class <= 1 && reads_key(&record.summary, key) && files.insert(record.path.clone()) {
            reads.push((class, record));
        } else {
            rest.push((class.max(1), record));
        }
    }
    reads.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.path.cmp(&right.1.path)));
    out.extend(reads.into_iter().map(|(_, record)| record));
    out.extend(other.into_iter().map(|(_, record)| record));
    rest.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.path.cmp(&right.1.path)).then(left.1.lines[0].cmp(&right.1.lines[0])));
    out.extend(rest.into_iter().map(|(_, record)| record));
    out
}

/// 0 program code, 1 shell, build and config files, 2 tests and fixtures, 3 docs and prose.
fn class_of(path: &str) -> u8 {
    let language = language_of(path);
    let segments: Vec<&str> = path.split('/').collect();
    let base = segments.last().copied().unwrap_or(path);
    let dir_is = |names: &[&str]| segments[..segments.len().saturating_sub(1)].iter().any(|segment| names.contains(segment));
    if matches!(language, "markdown" | "text" | "tex" | "rst" | "asciidoc" | "org") || dir_is(&["doc", "docs"]) {
        return 3;
    }
    let test_name = base.starts_with("test_") || base.contains("_test.") || base.contains(".test.") || base.contains(".spec.") || base.starts_with("conftest");
    if test_name || dir_is(&["test", "tests", "testdata", "testing", "fixtures", "fixture", "__tests__", "spec", "benches", "examples", "example"]) {
        return 2;
    }
    if matches!(language, "config" | "shell" | "make" | "docker" | "hcl" | "opaque" | "sql" | "proto" | "graphql") {
        return 1;
    }
    0
}

/// True when `line` names `key` as a whole token that is quoted or follows `.`, `$` or `${`, the shape of a read.
fn reads_key(line: &str, key: &str) -> bool {
    let bytes = line.as_bytes();
    let ident = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut from = 0;
    while let Some(found) = line.get(from..).and_then(|rest| rest.find(key)) {
        let start = from + found;
        let end = start + key.len();
        from = end;
        let before = start.checked_sub(1).and_then(|at| bytes.get(at)).copied();
        if before.is_some_and(ident) || bytes.get(end).copied().is_some_and(ident) {
            continue;
        }
        if matches!(before, Some(b'"' | b'\'' | b'`' | b'.' | b'$' | b'{')) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(path: &str, line: u32, summary: &str) -> AnswerRecord {
        let mut record = AnswerRecord::basic(format!("text:{path}:{line}"), "text", path);
        record.lines = [line, line];
        record.summary = summary.to_string();
        record
    }

    fn symbol(id: &str, path: &str, score: i64) -> AnswerRecord {
        let mut record = AnswerRecord::basic(id, "var", path);
        record.score = score;
        record
    }

    #[test]
    fn key_shape_is_screaming_case_of_four_or_more() {
        assert!(key_shaped("HOME") && key_shaped("DJANGO_SETTINGS_MODULE") && key_shaped("XDG_CONFIG_HOME") && key_shaped("S3_BUCKET"));
        assert!(!key_shaped("ABC") && !key_shaped("Home") && !key_shaped("max_upload_size") && !key_shaped("_HOME") && !key_shaped("HOME DIR"));
    }

    #[test]
    fn a_read_is_a_whole_quoted_or_accessed_token() {
        assert!(reads_key("let home = env::var_os(\"HOME\")?;", "HOME"));
        assert!(reads_key("const dir = process.env.CLAUDE_PROJECT_DIR;", "CLAUDE_PROJECT_DIR"));
        assert!(reads_key("cd ${HOME}/x && ls $HOME", "HOME"));
        assert!(!reads_key("let x = env::var(\"XDG_CONFIG_HOME\");", "HOME"));
        assert!(!reads_key("HOME=/tmp", "HOME"));
        assert!(!reads_key("# set HOME first", "HOME"));
    }

    #[test]
    fn read_sites_in_code_lead_after_a_few_exact_symbols() {
        let symbols = vec![symbol("s:tools/a.sh#HOME", "tools/a.sh", 3000), symbol("s:docs/x.md#HOME", "docs/x.md", 1000)];
        let texts = vec![
            text("docs/config.md", 3, "`HOME` is read"),
            text("src/host.rs", 9, "std::env::var_os(\"HOME\")"),
            text("src/host.rs", 20, "env::var(\"HOME\")"),
            text("tests/t.rs", 4, "env::set_var(\"HOME\", x)"),
            text("tools/run.sh", 2, "cd \"$HOME\""),
            text("src/xdg.rs", 5, "env::var(\"XDG_CONFIG_HOME\")"),
        ];
        let ids: Vec<String> = rank("HOME", symbols, texts).into_iter().map(|record| record.id).collect();
        assert_eq!(
            ids,
            [
                "s:tools/a.sh#HOME",
                "text:src/host.rs:9",
                "text:tools/run.sh:2",
                "s:docs/x.md#HOME",
                "text:src/host.rs:20",
                "text:src/xdg.rs:5",
                "text:tests/t.rs:4",
                "text:docs/config.md:3",
            ]
        );
    }

    #[test]
    fn paths_class_as_code_config_tests_and_docs() {
        assert_eq!(class_of("src/flask/testing.py"), 0);
        assert_eq!(class_of(".claude/helpers/graft-hooks.cjs"), 0);
        assert_eq!(class_of("tools/run.sh"), 1);
        assert_eq!(class_of("tests/test_basic.py"), 2);
        assert_eq!(class_of("pkg/file_test.go"), 2);
        assert_eq!(class_of("docs/topics/settings.txt"), 3);
        assert_eq!(class_of("CHANGES.rst"), 3);
    }
}
