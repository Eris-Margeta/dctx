// SPDX-License-Identifier: Apache-2.0
//! Hand-labelled Java accuracy corpus. Historical observations are not expectations.
#![forbid(unsafe_code)]

use dctx_lex_java::{JavaLexer, LexedFile};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Label {
    name: String,
    kind: String,
    line: u32,
    parent: Option<String>,
}

// This dependency-free reader accepts the corpus's deliberately small JSON
// schema, checks every delimiter/key and rejects trailing data or escapes.
// Names are literal UTF-8; no fixture needs JSON string escapes.
struct Json<'a>(&'a str);

impl Json<'_> {
    fn take(&mut self, token: &str) {
        self.0 = self.0.trim_start();
        self.0 = self.0.strip_prefix(token).expect("JSON token");
    }

    fn string(&mut self) -> String {
        self.take("\"");
        let end = self.0.find('"').expect("closing JSON quote");
        let value = &self.0[..end];
        assert!(!value.contains('\\') && !value.chars().any(char::is_control));
        let result = value.to_owned();
        self.0 = &self.0[end + 1..];
        result
    }

    fn key(&mut self, key: &str) {
        assert_eq!(self.string(), key);
        self.take(":");
    }

    fn labels(&mut self) -> Vec<Label> {
        self.take("[");
        let mut labels = Vec::new();
        while !self.0.trim_start().starts_with(']') {
            if !labels.is_empty() {
                self.take(",");
            }
            self.take("{");
            self.key("name");
            let name = self.string();
            self.take(",");
            self.key("kind");
            let kind = self.string();
            assert!(
                ["class", "interface", "enum", "struct", "method", "const"]
                    .contains(&kind.as_str())
            );
            self.take(",");
            self.key("line");
            self.0 = self.0.trim_start();
            let end = self
                .0
                .find(|c: char| !c.is_ascii_digit())
                .expect("line end");
            let line = self.0[..end].parse().expect("positive line number");
            assert!(line > 0);
            self.0 = &self.0[end..];
            self.take(",");
            self.key("parent");
            let parent = if self.0.trim_start().starts_with("null") {
                self.take("null");
                None
            } else {
                Some(self.string())
            };
            self.take("}");
            labels.push(Label {
                name,
                kind,
                line,
                parent,
            });
        }
        self.take("]");
        assert!(self.0.trim().is_empty(), "trailing JSON data");
        labels
    }
}

fn read_labels(path: &Path) -> Vec<Label> {
    let text = fs::read_to_string(path).expect("fixture labels");
    Json(&text).labels()
}

fn verify_baseline(root: &Path) {
    let manifest = fs::read_to_string(root.join("baseline.sha256")).expect("baseline manifest");
    let mut entries = 0;
    for line in manifest.lines() {
        let (digest, name) = line.split_once("  ").expect("SHA-256 manifest row");
        assert!(
            !name.contains(['/', '\\']),
            "manifest only names sibling files"
        );
        let bytes = fs::read(root.join(name)).expect("baseline input");
        let actual = dctx_hash::Sha256::digest(&bytes).unwrap();
        let mut hex = String::new();
        for byte in actual.as_bytes() {
            write!(hex, "{byte:02x}").unwrap();
        }
        assert_eq!(hex, digest, "frozen baseline input changed: {name}");
        entries += 1;
    }
    assert!(
        entries >= 90,
        "source, labels and observations for at least 30 files"
    );
}

fn observations(file: &LexedFile) -> Vec<Label> {
    file.nodes
        .iter()
        .filter_map(|node| {
            let kind = node.fact.symbol_kind?;
            let edge = file
                .edges
                .iter()
                .find(|edge| edge.fact.dst == node.fact.id)
                .expect("symbol has containment edge");
            let parent = file
                .nodes
                .iter()
                .find(|node| node.fact.id == edge.fact.src)
                .expect("parent exists");
            Some(Label {
                name: node.fact.name.clone(),
                kind: kind.as_str().to_owned(),
                line: node.fact.lines[0],
                parent: parent.fact.symbol_kind.map(|_| parent.fact.name.clone()),
            })
        })
        .collect()
}

#[derive(Default)]
struct Counts {
    tp: u32,
    fp: u32,
    missed: u32,
}

type Scores = BTreeMap<String, Counts>;

fn score(expected: &[Label], actual: &[Label], scores: &mut Scores) {
    // Multiset matching: duplicate observations must count as false positives.
    let mut remaining = expected.to_vec();
    for label in actual {
        let count = scores.entry(label.kind.clone()).or_default();
        if let Some(at) = remaining.iter().position(|wanted| wanted == label) {
            remaining.remove(at);
            count.tp += 1;
        } else {
            count.fp += 1;
        }
    }
    for label in remaining {
        scores.entry(label.kind).or_default().missed += 1;
    }
}

fn fraction(numerator: u32, denominator: u32) -> f64 {
    if denominator == 0 {
        1.0
    } else {
        f64::from(numerator) / f64::from(denominator)
    }
}

fn table(stage: &str, scores: &Scores, report: &mut String) -> Counts {
    writeln!(
        report,
        "{stage}: kind       TP   FP   FN  precision    recall"
    )
    .unwrap();
    let mut total = Counts::default();
    for (kind, count) in scores {
        row(kind, count, report);
        total.tp += count.tp;
        total.fp += count.fp;
        total.missed += count.missed;
    }
    row("overall", &total, report);
    total
}

fn row(kind: &str, count: &Counts, report: &mut String) {
    writeln!(
        report,
        "{kind:16} {:3}  {:3}  {:3}  {:9.4}% {:8.4}%",
        count.tp,
        count.fp,
        count.missed,
        100.0 * fraction(count.tp, count.tp + count.fp),
        100.0 * fraction(count.tp, count.tp + count.missed)
    )
    .unwrap();
}

// Independent raw-byte masking oracle (not the production lexer). Corpus
// comments contain no Unicode escapes. Preserve byte offsets and CRLF lines.
fn mask_non_code(source: &[u8]) -> Vec<u8> {
    let mut masked = source.to_vec();
    let mut at = 0;
    while at < source.len() {
        let start = at;
        let rest = &source[at..];
        if rest.starts_with(b"//") {
            while at < source.len() && !matches!(source[at], b'\r' | b'\n') {
                at += 1;
            }
        } else if rest.starts_with(b"/*") {
            at += 2;
            while at < source.len() && !source[at..].starts_with(b"*/") {
                at += 1;
            }
            at = (at + 2).min(source.len());
        } else if matches!(source[at], b'"' | b'\'') {
            let quote = source[at];
            let width = if rest.starts_with(b"\"\"\"") { 3 } else { 1 };
            at += width;
            while at < source.len() {
                if source[at] == b'\\' {
                    at = (at + 2).min(source.len());
                } else if source[at..].starts_with(&source[start..start + width]) {
                    at += width;
                    break;
                } else {
                    assert!(
                        width == 3 || !matches!(source[at], b'\r' | b'\n'),
                        "unclosed quote {quote}"
                    );
                    at += 1;
                }
            }
        } else {
            at += 1;
            continue;
        }
        for byte in &mut masked[start..at] {
            if !matches!(byte, b'\r' | b'\n') {
                *byte = b' ';
            }
        }
    }
    masked
}

#[test]
fn d1113b_accuracy() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/accuracy");
    verify_baseline(&root);
    let mut files: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "java"))
        .collect();
    files.sort();
    assert!(files.len() >= 30, "keep the breadth of the labelled corpus");
    let mut before = Scores::new();
    let mut after = Scores::new();
    let mut report = format!(
        "Java accuracy: {} original, hand-labelled source files\n",
        files.len()
    );
    for path in files {
        let name = path.file_name().unwrap().to_str().unwrap();
        let source = fs::read(&path).unwrap();
        let expected = read_labels(&path.with_extension("expected.json"));
        let facts = JavaLexer::default()
            .extract(name, &source)
            .expect("accepted fixture");
        let actual = observations(&facts);
        let baseline = read_labels(&path.with_extension("before.json"));
        score(&expected, &baseline, &mut before);
        score(&expected, &actual, &mut after);
        for label in &expected {
            if !actual.contains(label) {
                writeln!(report, "MISS {name}: {label:?}").unwrap();
            }
        }
        for label in &actual {
            if !expected.contains(label) {
                writeln!(report, "EXTRA {name}: {label:?}").unwrap();
            }
            assert!(
                !label.name.contains("Phantom"),
                "literal/comment symbol: {name}: {label:?}"
            );
        }
        let masked = mask_non_code(&source);
        let clean = JavaLexer::default()
            .extract(name, &masked)
            .expect("masked fixture");
        assert_eq!(
            actual,
            observations(&clean),
            "symbols changed after masking strings/comments: {name}"
        );
    }
    table("before (frozen baseline)", &before, &mut report);
    let total = table("after (current extractor)", &after, &mut report);
    // Direct stdout also displays the table under cargo test's default capture.
    std::io::stdout()
        .lock()
        .write_all(report.as_bytes())
        .unwrap();
    assert!(
        u64::from(total.tp) * 100 >= u64::from(total.tp + total.missed) * 97,
        "overall recall < 0.97"
    );
    assert!(
        u64::from(total.tp) * 100 >= u64::from(total.tp + total.fp) * 99,
        "overall precision < 0.99"
    );
}

#[test]
fn scoring_penalizes_duplicates_wrong_lines_and_wrong_parents() {
    let label = Label {
        name: "A::run".into(),
        kind: "method".into(),
        line: 2,
        parent: Some("A".into()),
    };
    let mut wrong_line = label.clone();
    wrong_line.line = 3;
    let mut wrong_parent = label.clone();
    wrong_parent.parent = None;
    let mut scores = Scores::new();
    score(
        std::slice::from_ref(&label),
        &[label.clone(), label.clone(), wrong_line, wrong_parent],
        &mut scores,
    );
    assert_eq!(
        (
            scores["method"].tp,
            scores["method"].fp,
            scores["method"].missed
        ),
        (1, 3, 0)
    );
    let mut missing = Scores::new();
    score(&[label], &[], &mut missing);
    assert_eq!((missing["method"].tp, missing["method"].missed), (0, 1));
}

#[test]
fn masking_removes_bait_and_preserves_code_and_line_endings() {
    let source = b"// class PhantomLine {}\r\nclass Live { String s = \"class PhantomString {}\"; char c = '\\''; /* class PhantomBlock {} */ }";
    let masked = mask_non_code(source);
    assert_eq!(source.len(), masked.len());
    assert!(!std::str::from_utf8(&masked).unwrap().contains("Phantom"));
    assert!(std::str::from_utf8(&masked).unwrap().contains("class Live"));
    let line_breaks = |bytes: &[u8]| {
        bytes
            .iter()
            .enumerate()
            .filter(|(_, byte)| matches!(byte, b'\r' | b'\n'))
            .map(|(at, _)| at)
            .collect::<Vec<_>>()
    };
    assert_eq!(line_breaks(source), line_breaks(&masked));
}
