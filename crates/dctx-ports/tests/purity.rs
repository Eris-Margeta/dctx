// SPDX-License-Identifier: Apache-2.0
//! Source-level forbidden-IO check for the ports crate: traits and in-memory doubles only,
//! no filesystem, environment, process, network, thread, wall-clock or stdout access, and
//! no dependency besides dctx-core (contracts/API-PORTS-v1.md "imports no engine or adapter").
//! Comments and `#[cfg(test)]` tails are skipped. Run by `cargo test -p dctx-ports`.

#[path = "../../../tests/support/source_files.rs"]
mod source_files;

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &[
    "std::fs",
    "fs::",
    "std::env",
    "env::",
    "std::process",
    "process::",
    "std::net",
    "std::thread",
    "thread::",
    "println!",
    "eprintln!",
    "print!(",
    "eprint!(",
    "dbg!(",
    "SystemTime",
    "Instant",
    "Command::",
    "dctx_pack",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    out.extend(
        source_files::files(dir)
            .into_iter()
            .filter(|path| path.extension().is_some_and(|ext| ext == "rs")),
    );
}

fn violations(text: &str) -> usize {
    let mut found = 0;
    for line in text.lines() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        let code = line.split("//").next().unwrap_or("");
        found += FORBIDDEN
            .iter()
            .filter(|token| code.contains(**token))
            .count();
    }
    found
}

#[test]
fn port_sources_import_no_io() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() >= 9);
    for file in files {
        assert_eq!(
            violations(&fs::read_to_string(&file).unwrap()),
            0,
            "{}",
            file.display()
        );
    }
}

#[test]
fn the_check_catches_io_in_a_positive_control() {
    assert_eq!(violations("let t = std::time::SystemTime::now();\n"), 1);
    assert_eq!(violations("// std::fs only in a comment\n"), 0);
}

#[test]
fn ports_depend_only_on_core() {
    let manifest =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let deps = manifest.split("[dependencies]").nth(1).unwrap();
    let names: Vec<&str> = deps
        .lines()
        .filter_map(|l| l.split('=').next())
        .map(str::trim)
        .filter(|n| !n.is_empty() && !n.starts_with('['))
        .collect();
    assert_eq!(names, ["dctx-core"]);
}
