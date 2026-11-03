// SPDX-License-Identifier: Apache-2.0
//! Wire-budget and continuation invariants through the public query API.
//! Implements SPEC-MAP-MODE sections 3 and 4 for text and JSON.
//! Uses real parsed source to catch skipped pages and oversized source bodies.
use std::fmt::Write as _;
use dctx_map_query::{AnswerBudget, FileSource, Format, Index, Request, Verb, render_answer};
use std::{collections::BTreeSet, fs};

#[test]
fn budget_cut_pages_preserve_every_record_and_are_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let mut code = String::new();
    for i in 0..35 {
        writeln!(code, "/// Executes a bounded operation.\npub fn operation_{i:02}() {{}}"
        ).expect("writing to a String cannot fail");
    }
    fs::write(dir.path().join("lib.rs"), code).unwrap();
    let source = FileSource::new(dir.path()).unwrap();
    let mut index = Index::in_memory("budget").unwrap();
    index.refresh(&source, None).unwrap();
    let mut request = Request::new(Verb::Find, "operation");
    request.limit = 40;
    request.kinds = vec!["fn".into()];
    request.budget = AnswerBudget::new(300).unwrap();
    let mut seen = BTreeSet::new();
    for _ in 0..40 {
        let answer = index.answer(&source, &request).unwrap();
        let output = render_answer(&answer, &request, Format::Json).unwrap();
        assert_eq!(
            output,
            render_answer(&answer, &request, Format::Json).unwrap()
        );
        assert!(request.budget.fits(&output));
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        let records = value["records"].as_array().unwrap();
        assert!(!records.is_empty(), "{output}");
        for record in records {
            assert!(seen.insert(record["id"].as_str().unwrap().to_owned()));
        }
        if value["more"] == 0 {
            break;
        }
        request.offset += records.len();
        assert!(
            value["next"]
                .as_str()
                .unwrap()
                .contains(&format!("--offset {}", request.offset))
        );
    }
    assert_eq!(seen.len(), 35);
}

#[test]
fn source_is_cut_at_whole_lines_and_empty_answers_have_next_steps() {
    let dir = tempfile::tempdir().unwrap();
    let mut code = "pub fn long_body() {\n".to_owned();
    for _ in 0..100 {
        code.push_str("    let bounded_value = 123456789;\n");
    }
    code.push_str("}\n");
    fs::write(dir.path().join("lib.rs"), code).unwrap();
    let source = FileSource::new(dir.path()).unwrap();
    let mut index = Index::in_memory("budget").unwrap();
    index.refresh(&source, None).unwrap();
    let mut request = Request::new(Verb::Read, "s:lib.rs#long_body");
    request.budget = AnswerBudget::new(500).unwrap();
    let answer = index.answer(&source, &request).unwrap();
    for format in [Format::Text, Format::Json] {
        let output = render_answer(&answer, &request, format).unwrap();
        assert!(request.budget.fits(&output));
        assert!(output.contains("pub fn long_body()"));
        assert!(output.contains("--lines"));
    }
    let request = Request::new(Verb::Find, "notexistinganywhere");
    let answer = index.answer(&source, &request).unwrap();
    assert_eq!(answer.records, [] as [dctx_map_query::Record; 0]);
    assert_ne!(answer.next, "");
    let mut bad = Request::new(Verb::Read, "s:lib.rs#long_body");
    bad.lines = Some([200, 210]);
    assert!(index.answer(&source, &bad).is_err());
    bad.lines = Some([1, 2]);
    assert!(index.answer(&source, &bad).is_ok());
}
