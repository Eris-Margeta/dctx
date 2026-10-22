//! Source-level forbidden-IO check. This crate does not touch the filesystem, process or network.

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &[
    "std::fs", "fs::", "std::env", "env::", "std::process", "process::", "std::net", "std::thread",
    "thread::", "println!", "eprintln!", "print!(", "eprint!(", "dbg!(", "SystemTime", "Instant",
    "Command::",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn violations(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        let code = line.split("//").next().unwrap_or("");
        for token in FORBIDDEN {
            if code.contains(token) {
                found.push(format!("{}: {token}", number + 1));
            }
        }
    }
    found
}

#[test]
fn production_sources_name_no_io() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() >= 8, "expected the crate sources, found {}", files.len());
    let mut found = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).expect("read source");
        for hit in violations(&text) {
            found.push(format!("{}:{hit}", file.display()));
        }
    }
    assert!(found.is_empty(), "IO in dctx-template:\n{}", found.join("\n"));
}

#[test]
fn the_check_catches_io_and_allows_std_io() {
    let bad = "fn bad() { let _ = std::fs::read(\"x\"); }\n";
    assert_ne!(violations(bad), [] as [std::string::String; 0]);
    let ok = "fn read(r: &mut std::io::Stdin) {}\n";
    assert_eq!(violations(ok), [] as [std::string::String; 0]);
}
