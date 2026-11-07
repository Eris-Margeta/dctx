// SPDX-License-Identifier: Apache-2.0
//! Source-level forbidden-IO check for the pure core (ARCHITECTURE-PROPOSAL §2: "Public imports and
//! source-level forbidden-IO checks enforce this distinction"). Every module under `src/`
//! must not name filesystem, environment, process, network, thread, wall-clock or stdout APIs.
//! Comments, string literals and `#[cfg(test)]` items are skipped; code after a test item is not.
//! Run by `cargo test -p dctx-core`.

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
    "print!",
    "eprint!",
    "dbg!",
    "SystemTime",
    "Instant",
    "Command::",
];

const IO_ROOTS: &[&str] = &[
    "std::fs",
    "std::env",
    "std::process",
    "std::net",
    "std::thread",
];

struct Skip {
    end_depth: i32,
    seen_open: bool,
}

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

fn push_violation(found: &mut Vec<String>, item: String) {
    if !found.iter().any(|have| have == &item) {
        found.push(item);
    }
}

fn is_ident_char(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

fn ident_boundary(chars: &[char], index: usize) -> bool {
    index == 0 || !is_ident_char(chars[index - 1])
}

/// End index (exclusive) of a string or character literal starting at `index`.
fn string_end(chars: &[char], index: usize) -> Option<usize> {
    if !ident_boundary(chars, index) {
        return None;
    }
    let mut j = index;
    if chars[j] == 'b' {
        let next = *chars.get(j + 1)?;
        if next == '"' || next == 'r' || next == '\'' {
            j += 1;
        } else {
            return None;
        }
    }
    let mut hashes = 0usize;
    let raw = chars[j] == 'r';
    if raw {
        j += 1;
        while chars.get(j) == Some(&'#') {
            hashes += 1;
            j += 1;
        }
    }
    let quote = *chars.get(j)?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    if quote == '\'' && !raw {
        return char_end(chars, j);
    }
    j += 1;
    if raw {
        while j < chars.len() {
            if chars[j] == '"' && (0..hashes).all(|h| chars.get(j + 1 + h) == Some(&'#')) {
                return Some(j + 1 + hashes);
            }
            j += 1;
        }
        return Some(chars.len());
    }
    while j < chars.len() {
        if chars[j] == '\\' {
            j = j.saturating_add(2);
            continue;
        }
        if chars[j] == '"' {
            return Some(j + 1);
        }
        j += 1;
    }
    Some(chars.len())
}

fn char_end(chars: &[char], quote: usize) -> Option<usize> {
    let a = *chars.get(quote + 1)?;
    if a == '\\' {
        let close = quote + 3;
        if chars.get(close) == Some(&'\'') {
            return Some(close + 1);
        }
        return None;
    }
    if chars.get(quote + 2) == Some(&'\'') {
        return Some(quote + 3);
    }
    None
}

fn mask_line(line: &str, in_block: &mut bool) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if *in_block {
            if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                out.push(' ');
                out.push(' ');
                i += 2;
                *in_block = false;
            } else {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        if chars[i] == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() {
                out.push(' ');
                i += 1;
            }
            break;
        }
        if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
            out.push(' ');
            out.push(' ');
            i += 2;
            *in_block = true;
            continue;
        }
        if let Some(end) = string_end(&chars, i) {
            if end <= i {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            while i < end && i < chars.len() {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out.into_iter().collect()
}

fn peel_attrs(trimmed: &str) -> (bool, &str) {
    let mut cfg_test = false;
    let mut rest = trimmed.trim_start();
    while rest.starts_with('#') {
        let Some(end) = bracket_end(rest) else {
            break;
        };
        if rest[..=end].replace(' ', "").contains("cfg(test)") {
            cfg_test = true;
        }
        rest = rest[end + 1..].trim_start();
    }
    (cfg_test, rest)
}

fn bracket_end(src: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut started = false;
    for (i, c) in src.char_indices() {
        if c == '[' {
            depth += 1;
            started = true;
        } else if c == ']' {
            depth -= 1;
            if started && depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

fn paren_end(src: &str) -> Option<usize> {
    let mut depth = 1i32;
    for (i, c) in src.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// `Some(index after the item)` when the item ends in `code`; `None` while it stays open.
fn advance_skip(state: &mut Skip, code: &str, depth: &mut i32) -> Option<usize> {
    for (i, c) in code.chars().enumerate() {
        match c {
            '{' => {
                *depth += 1;
                state.seen_open = true;
            }
            '}' => {
                *depth -= 1;
                if state.seen_open && *depth == state.end_depth {
                    return Some(i + 1);
                }
            }
            ';' if !state.seen_open && *depth == state.end_depth => return Some(i + 1),
            _ => {}
        }
    }
    None
}

fn update_depth(code: &str, depth: &mut i32) {
    for c in code.chars() {
        match c {
            '{' => *depth += 1,
            '}' => *depth -= 1,
            _ => {}
        }
    }
}

fn is_use_line(trimmed: &str) -> bool {
    let t = trimmed.trim_start();
    if t.starts_with("use ") {
        return true;
    }
    let Some(after_pub) = t.strip_prefix("pub") else {
        return false;
    };
    let after_pub = after_pub.trim_start();
    let after_vis = if let Some(rest) = after_pub.strip_prefix('(') {
        match paren_end(rest) {
            Some(i) => rest[i + 1..].trim_start(),
            None => return false,
        }
    } else {
        after_pub
    };
    after_vis.starts_with("use ")
}

fn has_alias(code: &str, alias: &str) -> bool {
    let needle = format!("{alias}::");
    let mut rest = code;
    while let Some(at) = rest.find(&needle) {
        let boundary = rest[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !is_ident_char(c));
        if boundary {
            return true;
        }
        let next = at + needle.len();
        if next >= rest.len() {
            break;
        }
        rest = &rest[at + 1..];
    }
    false
}

fn scan_tokens(code: &str, line_no: usize, aliases: &[String], found: &mut Vec<String>) {
    for token in FORBIDDEN {
        if code.contains(token) {
            push_violation(found, format!("{line_no}: {token}"));
        }
    }
    for alias in aliases {
        if alias != "*" && alias != "self" && has_alias(code, alias) {
            push_violation(found, format!("{line_no}: {alias}::"));
        }
    }
}

fn note_use(stmt: &str, line_no: usize, aliases: &mut Vec<String>, found: &mut Vec<String>) {
    let Some(spec) = use_spec(stmt) else {
        return;
    };
    let mut hits = Vec::new();
    collect_imports(spec, "", &mut hits);
    for (token, alias) in hits {
        push_violation(found, format!("{line_no}: {token}"));
        if alias != "*" && alias != "self" && !aliases.iter().any(|have| have == &alias) {
            aliases.push(alias);
        }
    }
}

fn use_spec(stmt: &str) -> Option<&str> {
    let trimmed = stmt.trim();
    let use_at = trimmed.find("use ")?;
    let spec = trimmed[use_at + 4..].trim().trim_end_matches(';').trim();
    if spec.is_empty() {
        None
    } else {
        Some(spec)
    }
}

fn forbidden_path(path: &str) -> Option<&'static str> {
    let path = path.trim_start_matches("::");
    for root in IO_ROOTS {
        if path == *root || path.starts_with(&format!("{root}::")) {
            return Some(*root);
        }
    }
    None
}

fn collect_imports(spec: &str, prefix: &str, hits: &mut Vec<(String, String)>) {
    let spec = spec.trim();
    if spec.is_empty() {
        return;
    }
    if let Some(inner) = spec.strip_prefix('{') {
        if let Some(inner) = brace_inner_from_open(inner) {
            for part in split_commas(inner) {
                collect_imports(part, prefix, hits);
            }
        }
        return;
    }
    let mut rest = spec;
    let mut rel = String::new();
    loop {
        rest = rest.trim_start();
        if let Some(stripped) = rest.strip_prefix("::") {
            rest = stripped.trim_start();
        }
        if rest.starts_with('{') {
            if let Some(inner) = brace_inner(rest) {
                let path = join_path(prefix, &rel);
                for part in split_commas(inner) {
                    collect_imports(part, &path, hits);
                }
            }
            return;
        }
        if rest.starts_with('*') {
            record(&join_path(prefix, &rel), "*", hits);
            return;
        }
        let Some((ident, after)) = read_ident(rest) else {
            if !rel.is_empty() {
                record(&join_path(prefix, &rel), last_seg(&rel), hits);
            }
            return;
        };
        if !rel.is_empty() {
            rel.push_str("::");
        }
        rel.push_str(ident);
        rest = after.trim_start();
        if rest.starts_with("::") {
            continue;
        }
        if let Some(after_as) = strip_as(rest) {
            let alias = match read_ident(after_as.trim_start()) {
                Some((name, _)) => name,
                None => "*",
            };
            record(&join_path(prefix, &rel), alias, hits);
            return;
        }
        record(&join_path(prefix, &rel), ident, hits);
        return;
    }
}

fn record(path: &str, alias: &str, hits: &mut Vec<(String, String)>) {
    if let Some(token) = forbidden_path(path) {
        hits.push((token.to_string(), alias.to_string()));
    }
}

fn join_path(prefix: &str, rel: &str) -> String {
    if rel.is_empty() || rel == "self" {
        prefix.to_string()
    } else if prefix.is_empty() {
        rel.to_string()
    } else {
        format!("{prefix}::{rel}")
    }
}

fn last_seg(path: &str) -> &str {
    match path.rsplit("::").next() {
        Some(seg) => seg,
        None => path,
    }
}

fn read_ident(src: &str) -> Option<(&str, &str)> {
    let mut chars = src.chars();
    let first = chars.next()?;
    if first != '_' && !first.is_ascii_alphabetic() {
        return None;
    }
    let mut end = first.len_utf8();
    for c in chars {
        if is_ident_char(c) {
            end += c.len_utf8();
        } else {
            break;
        }
    }
    Some((&src[..end], &src[end..]))
}

fn strip_as(src: &str) -> Option<&str> {
    let src = src.trim_start();
    let rest = src.strip_prefix("as")?;
    if rest.starts_with(|c: char| is_ident_char(c)) {
        None
    } else {
        Some(rest)
    }
}

fn brace_inner(src: &str) -> Option<&str> {
    let src = src.trim_start();
    let inner = src.strip_prefix('{')?;
    brace_inner_from_open(inner)
}

fn brace_inner_from_open(src: &str) -> Option<&str> {
    let mut depth = 1i32;
    for (i, c) in src.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&src[..i]);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_commas(src: &str) -> Vec<&str> {
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut out = Vec::new();
    for (i, c) in src.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(src[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    let tail = src[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

fn suffix_from(code: &str, char_end: usize) -> String {
    code.chars().skip(char_end).collect()
}

fn violations(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut aliases = Vec::new();
    let mut depth = 0i32;
    let mut pending_test = false;
    let mut skip: Option<Skip> = None;
    let mut in_block = false;
    let mut use_acc: Option<(usize, String)> = None;

    for (idx, line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let masked = mask_line(line, &mut in_block);
        let mut code = masked;
        if let Some(mut state) = skip.take() {
            match advance_skip(&mut state, &code, &mut depth) {
                None => {
                    skip = Some(state);
                    continue;
                }
                Some(end) => code = suffix_from(&code, end),
            }
        }
        let trimmed = code.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (cfg_test, rest) = peel_attrs(trimmed);
        if cfg_test {
            pending_test = true;
        }
        if rest.is_empty() {
            continue;
        }
        if pending_test {
            pending_test = false;
            let mut state = Skip {
                end_depth: depth,
                seen_open: false,
            };
            match advance_skip(&mut state, &code, &mut depth) {
                None => {
                    skip = Some(state);
                    continue;
                }
                Some(end) => code = suffix_from(&code, end),
            }
            if code.trim().is_empty() {
                continue;
            }
        }
        if let Some((start, mut acc)) = use_acc.take() {
            acc.push('\n');
            acc.push_str(&code);
            if acc.contains(';') {
                note_use(&acc, start, &mut aliases, &mut found);
            } else {
                use_acc = Some((start, acc));
            }
        } else if is_use_line(code.trim()) {
            if code.contains(';') {
                note_use(&code, line_no, &mut aliases, &mut found);
            } else {
                use_acc = Some((line_no, code.clone()));
            }
        }
        scan_tokens(&code, line_no, &aliases, &mut found);
        update_depth(&code, &mut depth);
    }
    found
}

#[test]
fn pure_core_modules_import_no_io() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(
        files.len() >= 15,
        "expected the pure modules, found {}",
        files.len()
    );
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
                  // std::fs, std::env, std::process, println! are forbidden\n\
                  fn note() { let _ = \"std::fs and println!\"; }\n\
                  fn raw() { let _ = r#\"std::fs::read\"#; }\n\
                  /* std::fs */\n";
    assert!(violations(source).is_empty(), "{:?}", violations(source));
    assert_eq!(violations("// pure prelude\nuse std::fs;"), ["2: std::fs"]);
}

#[test]
fn the_check_skips_the_test_tail() {
    let source = "fn bytes(text: &str) -> usize { text.len() }\n\
                  #[cfg(test)]\nmod tests { use std::fs; }";
    assert_eq!(violations(source), [] as [std::string::String; 0]);
}

#[test]
fn the_check_scans_production_after_a_test_item() {
    // F10: a `#[cfg(test)]` item does not hide production code that follows it.
    let leaked = "#[cfg(test)]\nmod tests {}\npub fn leak() { let _ = std::fs::read(\"x\"); }";
    let found = violations(leaked);
    assert!(
        found.iter().any(|v| v.contains("std::fs")),
        "missed production IO after a test module: {found:?}"
    );
    let test_only = "fn bytes(text: &str) -> usize { text.len() }\n\
                     #[cfg(test)]\n\
                     mod tests { use std::fs; fn log() { println!(hit); } }";
    assert!(
        violations(test_only).is_empty(),
        "test-only IO must stay allowed: {:?}",
        violations(test_only)
    );
}

#[test]
fn the_check_rejects_grouped_fs_alias_and_allows_grouped_path_imports() {
    // F10: a grouped rename of std::fs is reachable IO, not an unknown name.
    let leaked = "use std::{fs as disk};\npub fn leak() { let _ = disk::read(\"x\"); }";
    let found = violations(leaked);
    assert!(
        found.iter().any(|v| v.contains("disk::")),
        "missed grouped fs alias: {found:?}"
    );
    assert!(
        !violations("use std::{fs};").is_empty(),
        "missed grouped fs import"
    );
    let allowed = "use std::{path::Path, collections::HashMap};\n\
                   use std::{path::Path as disk};\n\
                   fn bytes(text: &str) -> usize { text.len() }\n\
                   fn open(disk: &disk) { let _ = disk::new(\"x\"); }\n";
    assert!(
        violations(allowed).is_empty(),
        "grouped path imports must stay allowed: {:?}",
        violations(allowed)
    );
}
