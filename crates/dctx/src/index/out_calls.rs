//! Outbound call names inside one symbol's source span, resolved or not (DCX-54b).
//! A call is an identifier followed by optional spaces and `(`; for `a.b.c(` and `x::y(` the name is the last segment.
//! Comments and string literals are blanked first, language keywords and the symbol's own declaration are dropped.
//! Pure text scan; the neighbors wiring that turns these names into `calls:out:?<name>` records is DCX-54c.

use super::generic::{mask, Syntax};

#[cfg(test)]
#[path = "out_calls_tests.rs"]
mod tests;

/// Most distinct names returned for one span.
pub(crate) const MAX_NAMES: usize = 40;

/// Most occurrence lines kept per name.
pub(crate) const MAX_LINES: usize = 3;

/// Keywords and control-flow words that are never calls, even when a `(` follows them. `new` is not listed.
const DENY_LIST: &[&str] = &[
    "if", "for", "while", "match", "return", "switch", "catch",
    "fn", "func", "def", "function", "let", "elif",
    "and", "or", "not", "in", "sizeof", "typeof",
];

/// True for the bytes that make up an identifier or a number.
fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Words of one masked line that are followed by optional spaces or tabs and `(`, in line order.
/// A word that starts with a digit is a number, not an identifier, and is skipped.
fn call_candidates(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut names = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        if !is_word_byte(bytes[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && is_word_byte(bytes[at]) {
            at += 1;
        }
        let starts_ident = bytes[start].is_ascii_alphabetic() || bytes[start] == b'_';
        let mut next = at;
        while next < bytes.len() && (bytes[next] == b' ' || bytes[next] == b'\t') {
            next += 1;
        }
        if starts_ident
            && bytes.get(next) == Some(&b'(')
            && let Some(word) = line.get(start..at)
        {
            names.push(word);
        }
    }
    names
}

/// Call names in `text` (one symbol's span, whose first line is absolute line `first_line`) in first-occurrence order,
/// each with up to [`MAX_LINES`] ascending absolute lines. `own` is the symbol's last name segment: its occurrence on line 1
/// is the declaration, not a call. `language` picks the comment syntax (`#` for python, ruby, shell; `//` and
/// `/* */` otherwise). At most [`MAX_NAMES`] names.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn calls_in(language: &str, text: &str, first_line: u32, own: &str) -> Vec<(String, Vec<u32>)> {
    let syntax = if matches!(language, "python" | "ruby" | "shell") {
        Syntax { line: &["#"], block: None, nested: false, quotes: b"\"'", triple: false }
    } else {
        Syntax { line: &["//"], block: Some(("/*", "*/")), nested: false, quotes: b"\"'`", triple: false }
    };
    let masked = mask(text, &syntax);
    let mut found: Vec<(String, Vec<u32>)> = Vec::new();
    for (index, line) in masked.lines().enumerate() {
        let number = first_line.saturating_add(index as u32);
        for name in call_candidates(line) {
            if DENY_LIST.contains(&name) || (index == 0 && name == own) {
                continue;
            }
            match found.iter().position(|(known, _)| known.as_str() == name) {
                Some(at) => {
                    let lines = &mut found[at].1;
                    if lines.len() < MAX_LINES && lines.last() != Some(&number) {
                        lines.push(number);
                    }
                }
                None => {
                    if found.len() < MAX_NAMES {
                        found.push((name.to_string(), vec![number]));
                    }
                }
            }
        }
    }
    found
}
