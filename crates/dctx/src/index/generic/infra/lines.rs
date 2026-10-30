//! Line-oriented infra formats: SQL `CREATE` statements, Makefile targets and Pascal routines.
//! One declaration per line, picked from the masked line; the name is read back from the original line so quoted
//! identifiers survive. Bodies are never entered.

use std::collections::HashSet;

use super::super::{ident_at, is_denied, mask, signature_of, Decl, Syntax};
use super::numbered;

/// SQL: `--` and `/* */` comments, single-quoted strings.
const SQL: Syntax = Syntax { line: &["--"], block: Some(("/*", "*/")), nested: false, quotes: b"'", triple: false };
/// Makefile: `#` comments.
const MAKE: Syntax = Syntax { line: &["#"], block: None, nested: false, quotes: b"", triple: false };
/// Pascal first pass: `//` and `(* *)` comments, single-quoted strings.
const PASCAL_ROUND: Syntax = Syntax { line: &["//"], block: Some(("(*", "*)")), nested: false, quotes: b"'", triple: false };
/// Pascal second pass: `{ }` comments and directives.
const PASCAL_BRACE: Syntax = Syntax { line: &[], block: Some(("{", "}")), nested: false, quotes: b"", triple: false };

/// Words that may sit between `CREATE` and the object keyword.
const SQL_MODIFIERS: &[&str] = &[
    "OR", "REPLACE", "TEMP", "TEMPORARY", "UNIQUE", "MATERIALIZED", "RECURSIVE", "GLOBAL", "LOCAL", "UNLOGGED", "EXTERNAL",
    "VIRTUAL", "CLUSTERED", "NONCLUSTERED", "FULLTEXT", "SPATIAL", "EDITIONABLE", "NONEDITIONABLE",
];

/// First words of a Makefile line that are directives, never a target.
const MAKE_DIRECTIVES: &[&str] = &[
    "export", "unexport", "override", "include", "-include", "sinclude", "vpath", "private", "ifeq", "ifneq", "ifdef", "ifndef", "else",
    "endif", "define", "undefine",
];

/// SQL `CREATE [OR REPLACE] TABLE|VIEW|FUNCTION|PROCEDURE|INDEX|TRIGGER|TYPE name`, case-insensitive.
/// A statement runs to the next `;` outside a `$$` body, or is one line when none follows.
pub(super) fn sql(text: &str) -> Vec<Decl> {
    let masked = mask(text, &SQL);
    let mut decls: Vec<Decl> = Vec::new();
    let mut open: Option<usize> = None;
    let mut dollar = false;
    for (number, shown, original) in numbered(&masked, text) {
        let line = shown.trim();
        if line.is_empty() {
            continue;
        }
        if !dollar
            && let Some((name, kind)) = sql_decl(shown, original)
        {
            decls.push(Decl { name, kind, lines: [number, number], signature: signature_of(original) });
            open = Some(decls.len() - 1);
        }
        if shown.matches("$$").count() % 2 == 1 {
            dollar = !dollar;
        }
        if !dollar
            && line.contains(';')
            && let Some(at) = open.take()
        {
            decls[at].lines[1] = number;
        }
    }
    decls
}

/// Name and wire kind when the line starts a `CREATE` statement for a supported object.
fn sql_decl(shown: &str, original: &str) -> Option<(String, &'static str)> {
    let start = shown.trim_start();
    if !ident_at(start).is_some_and(|word| word.eq_ignore_ascii_case("create")) {
        return None;
    }
    let body = original.get(shown.len() - start.len()..)?;
    let mut words = body.split_whitespace().skip(1);
    let kind = loop {
        let upper = words.next()?.to_ascii_uppercase();
        match upper.as_str() {
            "TABLE" | "VIEW" | "TYPE" | "INDEX" => break "type",
            "FUNCTION" | "PROCEDURE" | "TRIGGER" => break "fn",
            word if SQL_MODIFIERS.contains(&word) || word.contains('=') => {}
            _ => return None,
        }
    };
    let name = words.find(|word| !matches!(word.to_ascii_uppercase().as_str(), "IF" | "NOT" | "EXISTS" | "CONCURRENTLY"))?;
    let name: String = name.split(['(', ';', ',']).next()?.chars().filter(|c| !matches!(*c, '"' | '`' | '[' | ']')).collect();
    let valid = name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '-'));
    (valid && !name.is_empty() && !name.eq_ignore_ascii_case("on") && !is_denied(&name.to_ascii_lowercase())).then_some((name, kind))
}

/// Makefile `target:` rules at column 0. Special `.NAME` targets, assignments, patterns and `define` bodies are skipped.
/// A target runs through its tab-indented recipe; a target repeated later is not repeated.
pub(super) fn make(text: &str) -> Vec<Decl> {
    let masked = mask(text, &MAKE);
    let rows: Vec<(u32, &str, &str)> = numbered(&masked, text).collect();
    let mut decls: Vec<Decl> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut define = false;
    for (at, &(number, shown, original)) in rows.iter().enumerate() {
        if define {
            define = !shown.starts_with("endef");
            continue;
        }
        if shown.strip_prefix("define").is_some_and(|rest| rest.starts_with(char::is_whitespace)) {
            define = true;
            continue;
        }
        let names = make_targets(shown.trim_end());
        if names.is_empty() {
            continue;
        }
        let mut end = number;
        for &(later, _, body) in &rows[at + 1..] {
            if body.starts_with('\t') {
                end = later;
            } else if !body.trim().is_empty() {
                break;
            }
        }
        for name in names {
            if seen.insert(name.clone()) {
                decls.push(Decl { name, kind: "fn", lines: [number, end], signature: signature_of(original) });
            }
        }
    }
    decls
}

/// The target names of a rule line, or none when the line is not a rule.
fn make_targets(line: &str) -> Vec<String> {
    if line.starts_with(char::is_whitespace) {
        return Vec::new();
    }
    let Some((head, tail)) = line.split_once(':') else {
        return Vec::new();
    };
    let words: Vec<&str> = head.split_whitespace().collect();
    let valid = |word: &&str| {
        !word.starts_with('.') && !is_denied(word) && word.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '+'))
    };
    if words.is_empty() || MAKE_DIRECTIVES.contains(&words[0]) || tail.trim_start_matches(':').starts_with('=') || !words.iter().all(valid) {
        return Vec::new();
    }
    words.into_iter().map(str::to_string).collect()
}

/// Pascal `procedure` and `function` (optionally `class`-prefixed), each name once, compared case-insensitively.
pub(super) fn pascal(text: &str) -> Vec<Decl> {
    let masked = mask(&mask(text, &PASCAL_ROUND), &PASCAL_BRACE);
    let mut seen: HashSet<String> = HashSet::new();
    let mut decls: Vec<Decl> = Vec::new();
    for (number, shown, original) in numbered(&masked, text) {
        let Some(name) = pascal_name(shown.trim()) else {
            continue;
        };
        if seen.insert(name.to_ascii_lowercase()) {
            decls.push(Decl { name, kind: "fn", lines: [number, number], signature: signature_of(original) });
        }
    }
    decls
}

/// Routine name (`Name` or `Type.Name`) when the masked, trimmed line declares one.
fn pascal_name(line: &str) -> Option<String> {
    let mut rest = line;
    let mut word = ident_at(rest)?;
    if word.eq_ignore_ascii_case("class") {
        rest = rest[word.len()..].trim_start();
        word = ident_at(rest)?;
    }
    if !["procedure", "function"].iter().any(|keyword| word.eq_ignore_ascii_case(keyword)) {
        return None;
    }
    let rest = rest[word.len()..].strip_prefix(char::is_whitespace)?.trim_start();
    let end = rest.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.'))).unwrap_or(rest.len());
    let name = rest[..end].trim_end_matches('.');
    let first = name.chars().next()?;
    let bad_start = first.is_ascii_digit() || first == '.';
    (!bad_start && !is_denied(&name.to_ascii_lowercase())).then(|| name.to_string())
}
