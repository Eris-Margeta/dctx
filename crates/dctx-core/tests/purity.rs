//! Source-level forbidden-IO check for the pure core (ARCHITECTURE-PROPOSAL §2: "Public imports and
//! source-level forbidden-IO checks enforce this distinction"). Every non-legacy module under `src/`
//! must not name filesystem, environment, process, network, thread, wall-clock or stdout APIs.
//! Comments and the `#[cfg(test)]` tail of each file are skipped. Run by `cargo test -p dctx-core`.

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &[
    "std::fs", "fs::", "std::env", "env::", "std::process", "process::", "std::net", "std::thread",
    "thread::", "println!", "eprintln!", "print!(", "eprint!(", "dbg!(", "SystemTime", "Instant",
    "Command::",
];

fn rust_files(dir: &Path, skip_dir: Option<&str>, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) != skip_dir {
                rust_files(&path, skip_dir, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn violations(file: &Path) -> Vec<String> {
    let text = fs::read_to_string(file).unwrap();
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        let code = line.split("//").next().unwrap_or("");
        for token in FORBIDDEN {
            if code.contains(token) {
                found.push(format!("{}:{}: {}", file.display(), number + 1, token));
            }
        }
    }
    found
}

#[test]
fn pure_core_modules_import_no_io() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, Some("legacy"), &mut files);
    assert!(files.len() >= 15, "expected the pure modules, found {}", files.len());
    let found: Vec<String> = files.iter().flat_map(|f| violations(f)).collect();
    assert!(found.is_empty(), "IO in pure core:\n{}", found.join("\n"));
}

#[test]
fn the_check_catches_io_in_a_positive_control() {
    let legacy = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/legacy/walker.rs");
    assert!(!violations(&legacy).is_empty());
}
