//! Source-level forbidden-IO check for the pure core (ARCHITECTURE-PROPOSAL §2: "Public imports and
//! source-level forbidden-IO checks enforce this distinction"). Every module under `src/`
//! must not name filesystem, environment, process, network, thread, wall-clock or stdout APIs.
//! Comments and the `#[cfg(test)]` tail of each file are skipped. Run by `cargo test -p dctx-core`.

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &[
    "std::fs", "fs::", "std::env", "env::", "std::process", "process::", "std::net", "std::thread",
    "thread::", "println!", "eprintln!", "print!", "eprint!", "dbg!", "SystemTime", "Instant",
    "Command::",
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

fn violations(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        let code = line.split("//").next().unwrap_or("");
        for token in FORBIDDEN {
            if code.contains(token) {
                found.push(format!("{}: {}", number + 1, token));
            }
        }
    }
    found
}

#[test]
fn pure_core_modules_import_no_io() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() >= 15, "expected the pure modules, found {}", files.len());
    let found: Vec<String> = files
        .iter()
        .flat_map(|file| {
            violations(&fs::read_to_string(file).unwrap())
                .into_iter()
                .map(move |violation| format!("{}:{violation}", file.display()))
        })
        .collect();
    assert!(found.is_empty(), "IO in pure core:\n{}", found.join("\n"));
}

#[test]
fn the_check_catches_forbidden_snippets() {
    for source in [
        "use std::fs;",
        "use std::env;",
        "use std::process::Command;",
        "use std::net::TcpStream;",
        "use std::thread;",
        "fn read() { fs::read(path); }",
        "fn home() { env::var(name); }",
        "fn exit() { process::exit(1); }",
        "fn spawn() { thread::spawn(work); }",
        "fn run() { Command::new(name); }",
        "fn now() { SystemTime::now(); Instant::now(); }",
        "fn output() { println!(value); }",
        "fn output() { eprintln!(value); }",
        "fn output() { print!{value}; }",
        "fn output() { eprint![value]; }",
        "fn output() { dbg!(value); }",
    ] {
        assert!(!violations(source).is_empty(), "missed IO: {source}");
    }
}

#[test]
fn the_check_allows_pure_snippets_and_comments() {
    let source = "use std::path::{Path, PathBuf};\n\
                  use std::collections::HashMap;\n\
                  fn bytes(text: &str) -> usize { text.len() }\n\
                  // std::fs, std::env, std::process, println! are forbidden\n";
    assert!(violations(source).is_empty());
    assert_eq!(violations("// pure prelude\nuse std::fs;"), ["2: std::fs"]);
}

#[test]
fn the_check_skips_the_test_tail() {
    let source = "fn bytes(text: &str) -> usize { text.len() }\n\
                  #[cfg(test)]\nmod tests { use std::fs; }";
    assert!(violations(source).is_empty());
}
