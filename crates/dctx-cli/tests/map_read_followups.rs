// SPDX-License-Identifier: Apache-2.0
//! Binary checks for absolute --lines and an over-budget read.
//! Covers DCTX-SERIOUS-GAPS G5b and G9b and contracts/API-PORTS-v1.md.
//! Depends on the compiled dctx-cli and a temporary source file. No network.
//! Used by `cargo nextest -p dctx-cli`. Depth stays unset when --lines is set.
//! A module past the budget returns a marked prefix and a cursor for the rest.

#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Repository {
    root: PathBuf,
}

impl Repository {
    fn new(label: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("dctx-e02n-{label}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("repo")).expect("repo");
        for folder in ["home", "config", "cache", "plugins", "map-cache", "local-app-data", "empty-path", "tmp"] {
            fs::create_dir(root.join(folder)).expect("isolated directory");
        }
        Self { root }
    }

    fn repo(&self) -> PathBuf {
        self.root.join("repo")
    }

    fn output(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command.current_dir(self.repo()).env_clear().args(args).env("DCTX_AUTO_CLEAN", "0").env("NO_COLOR", "1");
        for (variable, folder) in [
            ("HOME", "home"),
            ("USERPROFILE", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("LOCALAPPDATA", "local-app-data"),
            ("DCTX_PLUGIN_ROOT", "plugins"),
            ("DCTX_CACHE_DIR", "cache"),
            ("DCTX_MAP_CACHE", "map-cache"),
            ("PATH", "empty-path"),
            ("TMPDIR", "tmp"),
            ("TMP", "tmp"),
            ("TEMP", "tmp"),
        ] {
            command.env(variable, self.root.join(folder));
        }
        command.output().expect("run compiled CLI")
    }

    fn run(&self, args: &[&str]) -> String {
        let output = self.output(args);
        assert!(
            output.status.success(),
            "{args:?}: status {}; stdout: {}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        String::from_utf8(output.stdout).expect("UTF-8 stdout")
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn json_string_at(text: &str) -> Option<String> {
    let mut chars = text.chars();
    if chars.next() != Some('"') {
        return None;
    }
    let mut out = String::new();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            return Some(out);
        }
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                let code = u32::from_str_radix(&hex, 16).ok()?;
                out.push(char::from_u32(code)?);
            }
            _ => return None,
        }
    }
    None
}

fn json_field(page: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":");
    let index = page.find(&pattern)?;
    let rest = &page[index + pattern.len()..];
    if rest.starts_with("null") {
        None
    } else {
        json_string_at(rest)
    }
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
    if lines.last().is_some_and(|line| line.starts_with("[DCTX_TRUNCATED]") || *line == "[DCTX_OMITTED]\n") {
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
    let page = repo.run(&["map", "read", "m:lines.rs", "--lines", "35-36", "--json"]);
    let source = json_field(&page, "source_utf8").expect("source");
    assert_eq!(source, line_slice(&body, 35, 36), "{page}");
    assert_eq!(source, "// L35\n// L36\n");
    assert!(json_field(&page, "next").is_none(), "{page}");
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
            Some(token) => repo.run(&[
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
            None => repo.run(&["map", "read", "m:wide.rs", "--limit", "1", "--json", "--budget", "1000"]),
        };
        let source = json_field(&page, "source_utf8").expect("source");
        let marked = source.contains("[DCTX_TRUNCATED]");
        seen.push_str(&without_mark(&source));
        if let Some(token) = json_field(&page, "next") {
            assert!(marked, "{page}");
            assert!(token.contains("|read|m:wide.rs|"), "{token}");
            cursor = Some(token);
        } else {
            assert!(!marked, "{page}");
            break;
        }
    }
    assert!(pages > 1, "the module fit in one page");
    assert_eq!(seen, body);
}
