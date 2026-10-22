//! Source-level forbidden-IO check for the pack engine (ARCHITECTURE-PROPOSAL §2: engines call
//! `Source::read` but never `std::fs`, the environment, processes, the wall clock or stdout).
//! Also checks the manifest names no git or filesystem crate (decision 078). Comments and `#[cfg(test)]`
//! tails are skipped. Run by `cargo test -p dctx-pack`.

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &[
    "std::fs", "fs::", "std::env", "env::", "std::process", "process::", "std::net", "std::thread",
    "thread::", "println!", "eprintln!", "print!(", "eprint!(", "dbg!(", "SystemTime", "Instant",
    "Command::", "dctx_core::walker", "dctx_core::content_reader", "dctx_core::config", "dctx_core::state",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn violations(text: &str, label: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        let code = line.split("//").next().unwrap_or("");
        for token in FORBIDDEN {
            if code.contains(token) {
                found.push(format!("{label}:{}: {token}", number + 1));
            }
        }
    }
    found
}

#[test]
fn engine_sources_import_no_io() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() >= 4);
    let found: Vec<String> = files
        .iter()
        .flat_map(|f| violations(&fs::read_to_string(f).unwrap(), &f.display().to_string()))
        .collect();
    assert!(found.is_empty(), "IO in dctx-pack:\n{}", found.join("\n"));
}

#[test]
fn the_check_catches_io_in_a_positive_control() {
    assert_eq!(violations("use std::fs::File;\nlet x = 1; // fs:: in a comment\n", "sample").len(), 2);
    assert_eq!(violations("let walker = 1;\n", "sample"), [] as [std::string::String; 0]);
}

#[test]
fn manifest_has_no_git_or_io_crates() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let deps = manifest.split("[dependencies]").nth(1).unwrap().split("[dev-dependencies]").next().unwrap();
    for banned in ["git", "walkdir", "ignore", "tokio", "rusqlite"] {
        assert!(!deps.contains(banned), "dependency {banned} in dctx-pack");
    }
}
