// SPDX-License-Identifier: Apache-2.0
//! Binary checks for absolute --lines and over-budget read continuations.
//! Covers DCTX-SERIOUS-GAPS G5b and G9b and contracts/API-PORTS-v1.md.
//! Depends on the compiled dctx-cli and a temporary source file. No network.
//! Used by `cargo nextest -p dctx-cli`. Depth stays unset when --lines is set.
//! A module past the budget returns a marked prefix and a cursor for the rest.

#![forbid(unsafe_code)]

mod common;

use serde_json::{Value, json};
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Repository {
    root: tempfile::TempDir,
}

impl Repository {
    fn new(label: &str) -> Self {
        let root = tempfile::Builder::new()
            .prefix(label)
            .tempdir()
            .expect("temporary repository");
        for folder in [
            "repo",
            "home",
            "config",
            "cache",
            "plugins",
            "map-cache",
            "local-app-data",
            "empty-path",
            "tmp",
        ] {
            fs::create_dir(root.path().join(folder)).expect("isolated directory");
        }
        Self { root }
    }

    fn repo(&self) -> PathBuf {
        self.root.path().join("repo")
    }

    fn output(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .current_dir(self.repo())
            .env_clear()
            .args(args)
            .env("DCTX_AUTO_CLEAN", "0")
            .env("NO_COLOR", "1");
        common::isolated_dirs(&mut command, self.root.path());
        for (variable, folder) in [
            ("DCTX_PLUGIN_ROOT", "plugins"),
            ("DCTX_CACHE_DIR", "cache"),
            ("DCTX_MAP_CACHE", "map-cache"),
            ("PATH", "empty-path"),
        ] {
            command.env(variable, self.root.path().join(folder));
        }
        command.output().expect("run compiled CLI")
    }

    fn read_json(&self, args: &[&str]) -> Value {
        let output = self.output(args);
        assert!(
            output.status.success(),
            "status {}. {}",
            output.status,
            command_diagnostic(args, &output.stdout, &output.stderr),
        );
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid JSON ({:?}). {}",
                error.to_string(),
                command_diagnostic(args, &output.stdout, &output.stderr)
            )
        })
    }
}

fn command_diagnostic(args: &[&str], stdout: &[u8], stderr: &[u8]) -> String {
    format!(
        "{args:?}; stdout: {:?}; stderr: {:?}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr),
    )
}

fn line_slice(text: &str, start: u32, end: u32) -> String {
    let mut out = String::new();
    for (number, line) in text.split_inclusive('\n').enumerate() {
        let Ok(number) = u32::try_from(number) else {
            break;
        };
        let number = number.saturating_add(1);
        if number >= start && number <= end {
            out.push_str(line);
        }
    }
    out
}

fn without_mark(source: &str) -> String {
    let mut lines: Vec<&str> = source.split_inclusive('\n').collect();
    if lines
        .last()
        .is_some_and(|line| line.starts_with("[DCTX_TRUNCATED]") || *line == "[DCTX_OMITTED]\n")
    {
        lines.pop();
    }
    lines.concat()
}

fn wide_file() -> String {
    let mut body = String::from("fn BudgetBeacon() {\n    let kept = 1;\n}\n");
    let pad = "// budget-line ....................................\n";
    while body.len() < 5 * 1024 {
        body.push_str(pad);
    }
    body.push_str("// BUDGET_TAIL\n");
    body
}

#[test]
fn lines_35_36_match_disk_lines() {
    let repo = Repository::new("lines");
    let mut body = String::new();
    for number in 1..=40 {
        let _ = writeln!(body, "// L{number:02}");
    }
    fs::write(repo.repo().join("lines.rs"), &body).expect("source");
    let page = repo.read_json(&["map", "read", "m:lines.rs", "--lines", "35-36", "--json"]);
    let source = read_record(&page)["source_utf8"].as_str().expect("source");
    assert_eq!(source, line_slice(&body, 35, 36));
    assert_eq!(source, "// L35\n// L36\n");
    assert_eq!(page.get("next"), Some(&Value::Null));
}

#[test]
fn a_module_past_the_budget_returns_a_marked_prefix_and_the_rest_once() {
    let repo = Repository::new("cut");
    let body = wide_file();
    assert!(body.len() >= 5 * 1024, "{}", body.len());
    assert!(body.lines().count() <= 120, "{}", body.lines().count());
    fs::write(repo.repo().join("wide.rs"), &body).expect("source");
    // 1,000 tokens admit 3,500 bytes: the signature fits, the module page does not.
    let mut cursor: Option<String> = None;
    let mut seen = String::new();
    let mut pages = 0u32;
    loop {
        pages += 1;
        assert!(pages <= 120, "continuation did not finish");
        let page = match &cursor {
            Some(token) => repo.read_json(&[
                "map",
                "read",
                "m:wide.rs",
                "--limit",
                "1",
                "--json",
                "--budget",
                "1000",
                "--cursor",
                token,
            ]),
            None => repo.read_json(&[
                "map",
                "read",
                "m:wide.rs",
                "--limit",
                "1",
                "--json",
                "--budget",
                "1000",
            ]),
        };
        let source = read_record(&page)["source_utf8"].as_str().expect("source");
        let marked = source.contains("[DCTX_TRUNCATED]");
        seen.push_str(&without_mark(source));
        if let Some(token) = page["next"].as_str() {
            assert!(marked, "{source:?}");
            assert!(token.contains("|read|m:wide.rs|"), "{token:?}");
            cursor = Some(token.to_owned());
        } else {
            assert_eq!(page.get("next"), Some(&Value::Null));
            assert!(!marked, "{source:?}");
            break;
        }
    }
    assert!(pages > 1, "the module fit in one page");
    assert_eq!(seen, body);
}

#[test]
fn read_page_coordinates() {
    const MODULE: &str = "m:src/lib.rs";
    const METHOD: &str = "s:src/lib.rs#Widget::run";
    let repo = Repository::new("read-coordinates");
    fs::create_dir(repo.repo().join("src")).expect("fixture source directory");
    fs::write(
        repo.repo().join("Cargo.toml"),
        include_bytes!("../../dctx-map-parse/tests/fixtures/rust/Cargo.toml"),
    )
    .expect("fixture manifest");
    let body = include_bytes!("../../dctx-map-parse/tests/fixtures/rust/src/lib.rs");
    fs::write(repo.repo().join("src/lib.rs"), body).expect("fixture library");
    fs::write(
        repo.repo().join("src/support.rs"),
        include_bytes!("../../dctx-map-parse/tests/fixtures/rust/src/support.rs"),
    )
    .expect("fixture support");

    let mut offsets = vec![0];
    for (offset, line) in body.split_inclusive(|&byte| byte == b'\n').enumerate() {
        offsets.push(offsets[offset] + line.len());
    }
    let last_line = offsets.len() - 1;
    let page = repo.read_json(&["map", "read", METHOD, "--lines", "24-24", "--json"]);
    let record = read_record(&page);
    assert_eq!(record["id"], METHOD);
    assert_eq!(record["path"], "src/lib.rs");
    assert_eq!(record["lines"], json!([23, 25]), "enclosing method lines");
    assert_eq!(record["span"], json!([438, 483]), "enclosing method bytes");
    let call = &body[offsets[23]..offsets[24]];
    assert_eq!(call, b"        helper()\n");
    assert_eq!(
        record["source_utf8"].as_str().map(str::as_bytes),
        Some(call)
    );
    assert_eq!(page.get("next"), Some(&Value::Null));

    let mut cursor: Option<String> = None;
    // The module starts at line 1. Each resume starts at the line in its cursor.
    let mut start = 1;
    let mut seen = Vec::new();
    loop {
        let mut args = vec!["map", "read", MODULE, "--budget", "250", "--json"];
        if let Some(token) = cursor.as_deref() {
            args.extend(["--cursor", token]);
        }
        let page = repo.read_json(&args);
        let record = read_record(&page);
        assert_eq!(record["id"], MODULE);
        assert_eq!(record["path"], "src/lib.rs");
        assert_eq!(
            record["lines"],
            json!([1, last_line]),
            "enclosing module lines"
        );
        assert_eq!(
            record["span"],
            json!([0, body.len()]),
            "enclosing module bytes"
        );
        let source = record["source_utf8"].as_str().expect("source excerpt");
        let next = page.get("next").expect("continuation field");
        let (excerpt, next_line) = if next.is_null() {
            assert!(cursor.is_some(), "budget 250 must require a continuation");
            (source, last_line + 1)
        } else {
            let token = next.as_str().expect("read cursor");
            let parts: Vec<_> = token.split('|').collect();
            let ["v1", _, "read", MODULE, next_start, end, "0"] = parts.as_slice() else {
                panic!("unexpected read cursor {token:?}");
            };
            let next_start: usize = next_start.parse().expect("cursor start line");
            let end: usize = end.parse().expect("cursor end line");
            assert_eq!(end, last_line);
            assert!(
                (start + 1..=end).contains(&next_start),
                "cursor did not advance within the module: {token:?}"
            );
            let marker = format!("[DCTX_TRUNCATED] lines {next_start}-{end}\n");
            let excerpt = source
                .strip_suffix(&marker)
                .unwrap_or_else(|| panic!("missing trailing marker {marker:?} in {source:?}"));
            (excerpt, next_start)
        };
        assert_eq!(page["more"], json!(last_line + 1 - next_line));
        assert_eq!(
            excerpt.as_bytes(),
            &body[offsets[start - 1]..offsets[next_line - 1]],
            "module excerpt at lines {start} through {}",
            next_line - 1,
        );
        seen.extend_from_slice(excerpt.as_bytes());
        if next.is_null() {
            break;
        }
        cursor = next.as_str().map(str::to_owned);
        start = next_line;
    }
    assert_eq!(
        seen,
        body.as_slice(),
        "module coverage through its last line"
    );
}

fn read_record(page: &Value) -> &Value {
    let records = page["records"].as_array().expect("read records");
    assert_eq!(records.len(), 1, "expected one read record");
    &records[0]
}

#[test]
fn command_diagnostic_escapes_hostile_arguments_and_output() {
    let hostile = "first\n\u{1b}\"second";
    assert_eq!(
        command_diagnostic(&[hostile], hostile.as_bytes(), hostile.as_bytes()),
        r#"["first\n\u{1b}\"second"]; stdout: "first\n\u{1b}\"second"; stderr: "first\n\u{1b}\"second""#,
    );
}
