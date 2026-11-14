// SPDX-License-Identifier: Apache-2.0
//! Authored C/C++ corpus scored with exact, multiplicity-aware symbol matching.
use dctx_lex_c::CLexer;
use dctx_map_model::{NodeKind, SymbolKind};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Label {
    name: String,
    kind: String,
    line: u32,
    parent: Option<String>,
}

// The corpus uses JSON arrays with one object per line, fixed field order and
// unescaped ASCII names. Reject any other schema instead of guessing at labels.
fn labels(text: &str) -> Vec<Label> {
    assert!(text.trim().starts_with('[') && text.trim().ends_with(']'));
    text.lines()
        .filter(|line| line.trim().starts_with('{'))
        .map(|line| {
            let object = line.trim().trim_end_matches(',');
            let fields: Vec<_> = object
                .strip_prefix('{')
                .unwrap()
                .strip_suffix('}')
                .unwrap()
                .split(", ")
                .collect();
            assert_eq!(fields.len(), 4, "{line}");
            let value = |index: usize, key: &str| {
                fields[index].strip_prefix(&format!("\"{key}\": ")).unwrap()
            };
            let string = |value: &str| {
                let inner = value.strip_prefix('"').unwrap().strip_suffix('"').unwrap();
                assert!(!inner.contains(['"', '\\']));
                inner.to_string()
            };
            Label {
                name: string(value(0, "name")),
                kind: string(value(1, "kind")),
                line: value(2, "line").parse().unwrap(),
                parent: match value(3, "parent") {
                    "null" => None,
                    other => Some(string(other)),
                },
            }
        })
        .collect()
}

#[derive(Default)]
struct Counts {
    tp: usize,
    fp: usize,
    fn_: usize,
}

impl Counts {
    fn precision(&self) -> f64 {
        ratio(self.tp, self.tp + self.fp)
    }

    fn recall(&self) -> f64 {
        ratio(self.tp, self.tp + self.fn_)
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "The small corpus counts fit f64 exactly."
)]
fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        1.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn kind_name(kind: Option<SymbolKind>) -> &'static str {
    match kind {
        None => "untyped",
        Some(SymbolKind::Function) => "function",
        Some(SymbolKind::Method) => "method",
        Some(SymbolKind::Struct) => "struct",
        Some(SymbolKind::Class) => "class",
        Some(SymbolKind::Enum) => "enum",
        Some(SymbolKind::Type) => "type",
        Some(SymbolKind::Const) => "const",
        Some(SymbolKind::Macro) => "macro",
        other => panic!("unexpected C/C++ kind: {other:?}"),
    }
}

fn table(counts: &BTreeMap<String, Counts>) -> (String, Counts) {
    let mut out = String::from("kind          TP   FP   FN   precision   recall\n");
    let mut total = Counts::default();
    for (kind, c) in counts {
        writeln!(
            out,
            "{kind:12} {:3}  {:3}  {:3}    {:7.3}%  {:7.3}%",
            c.tp,
            c.fp,
            c.fn_,
            c.precision() * 100.0,
            c.recall() * 100.0
        )
        .unwrap();
        total.tp += c.tp;
        total.fp += c.fp;
        total.fn_ += c.fn_;
    }
    writeln!(
        out,
        "overall      {:3}  {:3}  {:3}    {:7.3}%  {:7.3}%",
        total.tp,
        total.fp,
        total.fn_,
        total.precision() * 100.0,
        total.recall() * 100.0
    )
    .unwrap();
    (out, total)
}

#[test]
fn d1113b_accuracy() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/accuracy");
    let mut files: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("c" | "cpp")
            )
        })
        .collect();
    files.sort();
    assert!(files.len() >= 30);
    let mut counts = BTreeMap::<String, Counts>::new();
    let mut differences = String::new();
    let mut forbidden = Vec::new();
    let mut label_count = 0;
    let mut fingerprint = dctx_hash::Sha256::new();
    for path in &files {
        let name = path.file_name().unwrap().to_str().unwrap();
        let source = std::fs::read(path).unwrap();
        let expected_text = std::fs::read_to_string(path.with_extension("expected.json")).unwrap();
        for bytes in [name.as_bytes(), source.as_slice(), expected_text.as_bytes()] {
            fingerprint.update(bytes).unwrap();
            fingerprint.update(b"\0").unwrap();
        }
        let mut expected = labels(&expected_text);
        let negative_only = expected.is_empty();
        label_count += expected.len();
        let facts = CLexer::default().extract(name, &source).unwrap();
        let nodes: BTreeMap<_, _> = facts
            .nodes
            .iter()
            .map(|node| (&node.fact.id, &node.fact))
            .collect();
        for node in facts
            .nodes
            .iter()
            .filter(|node| node.fact.kind == NodeKind::Symbol)
        {
            let node = &node.fact;
            let parent = facts
                .edges
                .iter()
                .find(|edge| edge.fact.dst == node.id)
                .map(|edge| nodes[&edge.fact.src])
                .unwrap();
            let actual = Label {
                name: node.name.clone(),
                kind: kind_name(node.symbol_kind).to_string(),
                line: node.lines[0],
                parent: (parent.kind == NodeKind::Symbol).then(|| parent.name.clone()),
            };
            // All declaration names planted in comments/literals use the
            // reserved ghost_/GHOST_ prefixes or `fake`, including poison on
            // a real declaration's own starting line. Leakage fails outright,
            // even when the aggregate precision threshold would tolerate it.
            if negative_only
                || actual.name.contains("ghost_")
                || actual.name.contains("GHOST_")
                || actual.name.contains("fake")
            {
                forbidden.push(format!("{name}: {actual:?}"));
            }
            let c = counts.entry(actual.kind.clone()).or_default();
            if let Some(index) = expected.iter().position(|label| *label == actual) {
                c.tp += 1;
                expected.remove(index);
            } else {
                c.fp += 1;
                writeln!(differences, "FP {name}: {actual:?}").unwrap();
            }
        }
        for missed in expected {
            counts.entry(missed.kind.clone()).or_default().fn_ += 1;
            writeln!(differences, "FN {name}: {missed:?}").unwrap();
        }
    }
    let mut digest = String::new();
    for byte in fingerprint.finalize().as_bytes() {
        write!(digest, "{byte:02x}").unwrap();
    }
    assert_eq!(
        digest,
        include_str!("accuracy/corpus.sha256").trim(),
        "remeasure the baseline when changing sources or labels"
    );
    let mut before = BTreeMap::new();
    for row in include_str!("accuracy/before.tsv")
        .lines()
        .filter(|row| !row.starts_with('#'))
    {
        let cells: Vec<_> = row.split_whitespace().collect();
        assert_eq!(cells.len(), 4);
        if cells[0] != "overall" {
            before.insert(
                cells[0].to_string(),
                Counts {
                    tp: cells[1].parse().unwrap(),
                    fp: cells[2].parse().unwrap(),
                    fn_: cells[3].parse().unwrap(),
                },
            );
        }
    }
    let (before, baseline) = table(&before);
    assert_eq!(
        files.len(),
        38,
        "remeasure the baseline when extending the corpus"
    );
    assert_eq!(label_count, baseline.tp + baseline.fn_);
    let (after, total) = table(&counts);
    let report = format!(
        "\nD1113B-ACCC: {} files, {label_count} labels\nBEFORE (recorded revision 2)\n{before}AFTER (current extractor)\n{after}{differences}",
        files.len()
    );
    std::io::stdout()
        .lock()
        .write_all(report.as_bytes())
        .unwrap();
    assert!(
        forbidden.is_empty(),
        "symbols from strings/comments: {forbidden:?}"
    );
    assert!(total.recall() >= 0.97, "recall below 0.97");
    assert!(total.precision() >= 0.99, "precision below 0.99");
}
