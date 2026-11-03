// SPDX-License-Identifier: Apache-2.0
//! Gates decision 078 production SHA-256 and unsafe integration-test code.
//! Closes audit findings F5 and F6 for scan, parse, and the query tests.
//! Reads manifests and sources only. The sha2 oracle stays a dctx-hash dev-dependency.
//! `cargo test -p dctx-map-query` consumes this gate.

#![forbid(unsafe_code)]

use std::{
    fs,
    path::{Path, PathBuf},
};

fn crates_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates")
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|err| panic!("read_dir {}: {err}", dir.display()));
    for entry in entries {
        let path = entry.unwrap_or_else(|err| panic!("dir entry: {err}")).path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn toml_code(line: &str) -> &str {
    line.split('#').next().unwrap_or(line).trim()
}

fn manifest_declares_sha2(manifest: &str, dev: bool) -> bool {
    let wanted = if dev { "dev-dependencies" } else { "dependencies" };
    let named = if dev { "dev-dependencies.sha2" } else { "dependencies.sha2" };
    let mut in_section = false;
    for line in manifest.lines() {
        let code = toml_code(line);
        if let Some(inner) = code.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            let header = inner.trim();
            let leaf = header.rsplit('.').next().unwrap_or(header).trim();
            if header == named || header.ends_with(&format!(".{named}")) {
                return true;
            }
            in_section = leaf == wanted;
            continue;
        }
        let key = code.split('=').next().unwrap_or("").trim().trim_matches('"');
        if in_section && (key == "sha2" || key.starts_with("sha2.")) {
            return true;
        }
    }
    false
}

fn ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn references_sha2_crate(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while let Some(found) = text[index..].find("sha2") {
        let at = index + found;
        let before = at == 0 || !ident_byte(bytes[at - 1]);
        let after_at = at + 4;
        let after = after_at >= bytes.len() || !ident_byte(bytes[after_at]);
        if before && after {
            return true;
        }
        index = at + 1;
    }
    false
}

fn skip_block(bytes: &[u8], start: usize) -> usize {
    let mut index = start + 2;
    let mut depth = 1;
    while index + 1 < bytes.len() && depth > 0 {
        if bytes[index] == b'/' && bytes[index + 1] == b'*' {
            depth += 1;
            index += 2;
        } else if bytes[index] == b'*' && bytes[index + 1] == b'/' {
            depth -= 1;
            index += 2;
        } else {
            index += 1;
        }
    }
    index
}

fn finish_string(bytes: &[u8], mut index: usize) -> usize {
    let mut hashes = 0;
    while index < bytes.len() && bytes[index] == b'#' {
        hashes += 1;
        index += 1;
    }
    if index >= bytes.len() || bytes[index] != b'"' {
        return index;
    }
    index += 1;
    while index < bytes.len() {
        if hashes == 0 && bytes[index] == b'\\' {
            index = index.saturating_add(2).min(bytes.len());
            continue;
        }
        let tail = index + 1 + hashes;
        let closed = bytes[index] == b'"' && tail <= bytes.len() && bytes[index + 1..tail].iter().all(|byte| *byte == b'#');
        if closed {
            return tail;
        }
        index += 1;
    }
    index
}

fn skip_quoted_char(bytes: &[u8], mut index: usize) -> usize {
    if bytes.get(index) == Some(&b'\\') {
        index += 1;
        if bytes.get(index) == Some(&b'x') {
            index += 3;
        } else if bytes.get(index) == Some(&b'u') {
            index += 1;
            if bytes.get(index) == Some(&b'{') {
                index += 1;
                while index < bytes.len() && bytes[index] != b'}' {
                    index += 1;
                }
                index += 1;
            }
        } else {
            index += 1;
        }
    } else if index < bytes.len() {
        index += 1;
    }
    if bytes.get(index) == Some(&b'\'') {
        index += 1;
    }
    index.min(bytes.len())
}

fn skip_string(bytes: &[u8], index: usize) -> Option<usize> {
    for prefix in [b"br".as_slice(), b"cr", b"b", b"c", b"r"] {
        if bytes[index..].starts_with(prefix) {
            let next = index + prefix.len();
            if bytes.get(next).is_some_and(|byte| *byte == b'"' || *byte == b'#') {
                return Some(finish_string(bytes, next));
            }
        }
    }
    if bytes[index] == b'"' {
        Some(finish_string(bytes, index))
    } else {
        None
    }
}

fn unsafe_at(source: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index = bytes[index..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |at| index + at + 1);
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index = skip_block(bytes, index);
            continue;
        }
        if bytes[index] == b'b' && bytes.get(index + 1) == Some(&b'\'') {
            index = skip_quoted_char(bytes, index + 2);
            continue;
        }
        if let Some(next) = skip_string(bytes, index) {
            index = next.max(index + 1);
            continue;
        }
        if bytes[index] == b'\'' {
            let body = index + 1;
            let char_literal = bytes.get(body) == Some(&b'\\')
                || bytes.get(body + 1) == Some(&b'\'');
            if char_literal {
                index = skip_quoted_char(bytes, body);
            } else {
                index = body;
                while index < bytes.len() && ident_byte(bytes[index]) {
                    index += 1;
                }
            }
            continue;
        }
        if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len() && ident_byte(bytes[index]) {
                index += 1;
            }
            if &bytes[start..index] == b"unsafe" {
                return Some(start);
            }
            continue;
        }
        index += 1;
    }
    None
}

fn has_unsafe_code(source: &str) -> bool {
    unsafe_at(source).is_some()
}

#[test]
fn production_sha2_rejected_dev_oracle_accepted() {
    let refused = "[dependencies]\nsha2 = \"0.10\"\n";
    let accepted = "[dependencies]\nthiserror = \"2\"\n\n[dev-dependencies]\nsha2 = \"=0.10.9\"\n";
    assert!(manifest_declares_sha2(refused, false));
    assert!(manifest_declares_sha2("[dependencies.sha2]\nversion = \"0.10\"\n", false));
    assert!(!manifest_declares_sha2(accepted, false));
    assert!(manifest_declares_sha2(accepted, true));
    assert!(references_sha2_crate("use sha2::{Digest, Sha256};"));
    assert!(!references_sha2_crate("fn sha256_hex() -> &'static str { \"sha256\" }"));

    let root = crates_dir();
    for name in ["dctx-map-scan", "dctx-map-parse"] {
        let dir = root.join(name);
        let manifest = read_text(&dir.join("Cargo.toml"));
        assert!(!manifest_declares_sha2(&manifest, false), "{name} production dependency declares sha2");
        let mut sources = Vec::new();
        rust_files(&dir.join("src"), &mut sources);
        assert!(!sources.is_empty(), "{name} src missing");
        for file in sources {
            assert!(!references_sha2_crate(&read_text(&file)), "{} references the sha2 crate", file.display());
        }
    }

    let hash = root.join("dctx-hash");
    let hash_manifest = read_text(&hash.join("Cargo.toml"));
    assert!(!manifest_declares_sha2(&hash_manifest, false));
    assert!(manifest_declares_sha2(&hash_manifest, true), "dctx-hash dev oracle must keep sha2");
    let mut sources = Vec::new();
    rust_files(&hash.join("src"), &mut sources);
    let mut oracle = false;
    for file in sources {
        let text = read_text(&file);
        if file.file_name().is_some_and(|name| name == "differential.rs") {
            assert!(references_sha2_crate(&text), "dev oracle lost sha2");
            oracle = true;
        } else {
            assert!(!references_sha2_crate(&text), "{} is production source", file.display());
        }
    }
    assert!(oracle, "dctx-hash differential oracle missing");
}

#[test]
fn integration_tests_reject_unsafe_with_safe_control() {
    assert!(has_unsafe_code("fn demo() { unsafe { let _n = 1; } }"));
    assert!(!has_unsafe_code("fn demo() { let _n = 1; }"));
    assert!(!has_unsafe_code("// unsafe block\nfn demo() { let _n = 1; }\n"));
    assert!(!has_unsafe_code(
        "fn demo() { let _s = \"unsafe extern \\\"C\\\" { pub safe fn abs(x: i32) -> i32; }\"; }"
    ));

    let tests = crates_dir().join("dctx-map-query/tests");
    let mut files = Vec::new();
    rust_files(&tests, &mut files);
    files.sort();
    let mut saw_recovery = false;
    let mut saw_no_git = false;
    assert!(!files.is_empty(), "query integration tests missing");
    for file in &files {
        let text = read_text(file);
        let name = file.file_name().and_then(|item| item.to_str()).unwrap_or("");
        if name == "recovery.rs" {
            assert!(text.contains("unsafe"), "recovery fixture string missing");
            saw_recovery = true;
        }
        if name == "no_git.rs" {
            saw_no_git = true;
        }
        assert!(unsafe_at(&text).is_none(), "{} contains unsafe code", file.display());
    }
    assert!(saw_recovery && saw_no_git, "recovery.rs and no_git.rs were not both scanned");
}
