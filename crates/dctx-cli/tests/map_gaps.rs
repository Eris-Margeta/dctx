//! Binary checks for map targets, single-record verbs, cursors and line ranges.
//! Covers DCTX-SERIOUS-GAPS G2, G3, G4 and G5.
//! Depends on the compiled dctx-cli and temporary source files. No network and no git.
//! Each invocation is a separate process. A cursor is the opaque `next` token.
//! A missing target or a directory path is a usage error and does not scan.

#![forbid(unsafe_code)]

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
        let root = std::env::temp_dir().join(format!("dctx-e02c-{label}-{}-{n}", std::process::id()));
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

    fn write(&self, name: &str, body: &str) {
        fs::write(self.repo().join(name), body).expect("source");
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
        assert!(output.stderr.is_empty(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).expect("UTF-8 stdout")
    }

    fn fail(&self, args: &[&str]) -> String {
        let output = self.output(args);
        assert!(
            !output.status.success(),
            "{args:?} succeeded: {}",
            String::from_utf8_lossy(&output.stdout),
        );
        assert!(
            output.stdout.is_empty(),
            "{args:?} wrote stdout on failure: {}",
            String::from_utf8_lossy(&output.stdout),
        );
        String::from_utf8(output.stderr).expect("UTF-8 stderr")
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

fn record_ids(page: &str) -> Vec<String> {
    let Some(start) = page.find("\"records\":[") else {
        return Vec::new();
    };
    let rest = &page[start..];
    let end = rest.find("],\"more\"").unwrap_or(rest.len());
    let body = &rest[..end];
    let mut ids = Vec::new();
    let mut offset = 0;
    let needle = "\"id\":";
    while let Some(index) = body[offset..].find(needle) {
        let at = offset + index + needle.len();
        let Some(id) = json_string_at(&body[at..]) else {
            break;
        };
        offset = at + id.len();
        ids.push(id);
    }
    ids
}

fn json_lines(page: &str) -> Option<(u32, u32)> {
    let key = "\"lines\":[";
    let index = page.find(key)?;
    let rest = &page[index + key.len()..];
    let end = rest.find(']')?;
    let mut parts = rest[..end].split(',');
    let start = parts.next()?.trim().parse().ok()?;
    let finish = parts.next()?.trim().parse().ok()?;
    Some((start, finish))
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

fn touch_was_not_scanned(stderr: &str) {
    assert!(!stderr.contains("os error"), "{stderr}");
    assert!(!stderr.contains("source unavailable"), "{stderr}");
}

#[test]
fn explain_and_read_use_an_id_returned_by_find() {
    let repo = Repository::new("find");
    repo.write("beacon.rs", "fn Beacon() {}\n");
    let found = repo.run(&["map", "find", "Beacon", "--json"]);
    let id = json_field(&found, "id").expect("find id");
    assert!(id.contains("Beacon"), "{found}");
    let explained = repo.run(&["map", "explain", &id, "--json"]);
    let signature = json_field(&explained, "signature").expect("signature");
    assert!(signature.contains("Beacon"), "{explained}");
    let read = repo.run(&["map", "read", &id, "--json"]);
    let source = json_field(&read, "source_utf8").expect("source");
    assert!(source.contains("Beacon"), "{read}");
    assert!(repo.output(&["map", "read", &id, "--limit", "1", "--json"]).status.success());
}

#[test]
fn missing_target_and_a_directory_are_usage_errors_without_a_scan() {
    let repo = Repository::new("target");
    repo.write("beacon.rs", "fn Beacon() {}\n");
    fs::create_dir(repo.repo().join("nested")).expect("directory");
    let missing = repo.fail(&["map", "map", "--place", "/no/such/dctx-e02c-place"]);
    assert!(missing.contains("requires a target"), "{missing}");
    touch_was_not_scanned(&missing);
    let directory = repo.fail(&["map", "map", "nested"]);
    assert!(directory.contains("directory"), "{directory}");
    touch_was_not_scanned(&directory);
    let absent = repo.fail(&["map", "map", "missing.rs"]);
    assert!(absent.contains("not an existing file"), "{absent}");
    touch_was_not_scanned(&absent);
}

#[test]
fn an_existing_file_path_returns_the_same_records_as_its_module_id() {
    let repo = Repository::new("path");
    repo.write("beacon.rs", "fn Beacon() {}\n");
    let by_path = repo.run(&["map", "map", "beacon.rs", "--json"]);
    let by_id = repo.run(&["map", "map", "m:beacon.rs", "--json"]);
    assert_eq!(record_ids(&by_path), record_ids(&by_id));
    assert!(by_path.contains("s:beacon.rs#Beacon"), "{by_path}");
    assert!(!by_path.contains("gone"), "{by_path}");
    let help = repo.run(&["map", "--help"]);
    assert!(help.contains("--cursor"), "{help}");
    assert!(help.contains("place-relative file"), "{help}");
    assert!(help.contains("crates/dctx-cli/src/lib.rs"), "{help}");
    assert!(!help.contains("crates/dctx-core --depth"), "{help}");
}

#[test]
fn three_pages_visit_each_record_once_across_processes() {
    let repo = Repository::new("pages");
    repo.write("page.rs", "fn aaa() {}\nfn bbb() {}\nfn ccc() {}\n");
    let all = record_ids(&repo.run(&["map", "map", "m:page.rs", "--json"]));
    assert_eq!(all.len(), 3, "{all:?}");
    let mut cursor: Option<String> = None;
    let mut seen = Vec::new();
    for page_index in 0..3 {
        let page = match &cursor {
            Some(token) => repo.run(&["map", "map", "m:page.rs", "--limit", "1", "--json", "--cursor", token]),
            None => repo.run(&["map", "map", "m:page.rs", "--limit", "1", "--json"]),
        };
        let ids = record_ids(&page);
        assert_eq!(ids.len(), 1, "page {page_index}: {page}");
        assert!(!seen.contains(&ids[0]), "duplicate {}", ids[0]);
        seen.push(ids[0].clone());
        cursor = json_field(&page, "next");
        if page_index < 2 {
            assert!(cursor.is_some(), "{page}");
        } else {
            assert!(cursor.is_none(), "{page}");
        }
    }
    assert_eq!(seen, all);
    let stale = repo.fail(&["map", "map", "m:page.rs", "--cursor", "v1|9|map|s:page.rs#aaa", "--json"]);
    assert!(stale.contains("stale continuation"), "{stale}");
}

#[test]
fn lines_35_36_match_disk_and_an_inner_symbol_range_is_not_shifted() {
    let repo = Repository::new("lines");
    let mut body = String::from("// L01\nfn inner() {\n    let kept_line = 1;\n    let next_line = 2;\n}\n");
    for number in 6..=40 {
        body.push_str(&format!("// L{number:02}\n"));
    }
    repo.write("lines.rs", &body);
    let page = repo.run(&["map", "read", "m:lines.rs", "--lines", "35-36", "--json"]);
    let source = json_field(&page, "source_utf8").expect("module source");
    assert_eq!(source, line_slice(&body, 35, 36), "{page}");
    assert!(!source.contains("L01"), "{source}");

    let explained = repo.run(&["map", "explain", "s:lines.rs#inner", "--json"]);
    let (start, end) = json_lines(&explained).expect("symbol lines");
    assert!(end >= start.saturating_add(2), "span {start}-{end} has no inner range: {explained}");
    let inner_start = start.saturating_add(1);
    let inner_end = inner_start.saturating_add(1).min(end);
    let range = format!("{inner_start}-{inner_end}");
    let inner = repo.run(&["map", "read", "s:lines.rs#inner", "--lines", &range, "--json"]);
    let inner_source = json_field(&inner, "source_utf8").expect("inner source");
    assert_eq!(inner_source, line_slice(&body, inner_start, inner_end), "{inner}");
    let prefix = line_slice(&body, start, start.saturating_add(inner_end - inner_start));
    assert_ne!(inner_source, prefix, "range shifted to the start of the symbol");

    let outside = if start > 1 { "1-1".to_string() } else { format!("{}-{}", end.saturating_add(1), end.saturating_add(1)) };
    let refused = repo.fail(&["map", "read", "s:lines.rs#inner", "--lines", &outside, "--json"]);
    assert!(refused.contains("outside"), "{refused}");
}
