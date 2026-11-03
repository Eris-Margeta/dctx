// SPDX-License-Identifier: Apache-2.0
//! Nix outlines: the attribute names at depth 1 of the file's top-level attribute set (`name = ...;`).
//! The set is the first `{` outside `let` bindings, parentheses and brackets that is not a function argument pattern
//! (`{ pkgs, ... }:`, `args@{ ... }:`). An attribute is a function (`fn`) when its value starts with `<ident>:` or `{ ... }:`,
//! else a `var`. `inherit` lines, quoted names and deeper attributes are skipped; bodies are never entered.
//! Comments are `#` and `/* */`; strings are `"..."` and `''...''`, blanked before scanning.

use super::Lines;
use crate::index::generic::{is_denied, signature_of, Decl};

/// Declarations of the top-level attribute set of `text`; none when the file has no such set.
pub(super) fn decls(text: &str) -> Vec<Decl> {
    let masked = mask_nix(text);
    let bytes = masked.as_bytes();
    let Some((open, close)) = top_set(bytes) else {
        return Vec::new();
    };
    let lines = Lines::new(text);
    let mut out = Vec::new();
    let mut at = open + 1;
    while at < close {
        at = skip_space(bytes, at);
        if at >= close {
            break;
        }
        let end = statement_end(bytes, at, close);
        let path_end = bytes[at..end].iter().position(|byte| !is_attr_byte(*byte)).map_or(end, |offset| at + offset);
        let path = masked.get(at..path_end).unwrap_or("");
        let value = skip_space(bytes, path_end);
        let assigns = bytes.get(value) == Some(&b'=') && bytes.get(value + 1) != Some(&b'=');
        if assigns && !path.is_empty() && path != "inherit" && !is_denied(path) && !path.starts_with(|c: char| c.is_ascii_digit() || c == '.' || c == '-') && !path.ends_with('.') {
            let start = skip_space(bytes, value + 1);
            let kind = if is_function(bytes, start) { "fn" } else { "var" };
            let last = if end < close { end } else { bytes[..end].iter().rposition(|byte| !byte.is_ascii_whitespace()).unwrap_or(at).max(at) };
            let signature = signature_of(text.get(at..).and_then(|rest| rest.lines().next()).unwrap_or(""));
            out.push(Decl { name: path.to_string(), kind, lines: [lines.of(at), lines.of(last)], signature });
        }
        at = end + 1;
    }
    out
}

/// True for a byte of an attribute path: identifier characters and the dots between segments.
fn is_attr_byte(byte: u8) -> bool {
    is_word(byte) || byte == b'.'
}

/// True for a byte of a Nix identifier. Non-ASCII bytes count, so slicing stays on character boundaries.
fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'\'') || byte >= 0x80
}

/// Offset of the first byte at or after `from` that is not whitespace.
fn skip_space(bytes: &[u8], mut from: usize) -> usize {
    while bytes.get(from).is_some_and(u8::is_ascii_whitespace) {
        from += 1;
    }
    from
}

/// Offset of the `}` that closes the `{` at `open`, or the end of input.
fn matching(bytes: &[u8], open: usize) -> usize {
    let mut depth = 0usize;
    for (offset, byte) in bytes[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return open + offset;
                }
            }
            _ => {}
        }
    }
    bytes.len()
}

/// The word that starts at `at`, if `at` starts one.
fn word_at(bytes: &[u8], at: usize) -> Option<&[u8]> {
    if at > 0 && is_word(bytes[at - 1]) {
        return None;
    }
    let end = bytes[at..].iter().position(|byte| !is_word(*byte)).map_or(bytes.len(), |offset| at + offset);
    (end > at).then(|| &bytes[at..end])
}

/// Offsets of the braces of the top-level attribute set: the first `{` outside `let` bindings and brackets that is no argument pattern.
fn top_set(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut lets = 0usize;
    let mut at = 0usize;
    while at < bytes.len() {
        if let Some(word) = word_at(bytes, at) {
            if depth == 0 {
                match word {
                    b"let" => lets += 1,
                    b"in" => lets = lets.saturating_sub(1),
                    _ => {}
                }
            }
            at += word.len();
            continue;
        }
        match bytes[at] {
            b'{' if depth == 0 && lets == 0 => {
                let close = matching(bytes, at);
                if !is_pattern(bytes, close + 1) {
                    return Some((at, close));
                }
                at = close;
            }
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        at += 1;
    }
    None
}

/// True when the text at `from` (after a `}`) ends a function argument pattern: `:` or `@name:`.
fn is_pattern(bytes: &[u8], from: usize) -> bool {
    let mut at = skip_space(bytes, from);
    if bytes.get(at) == Some(&b'@') {
        at = skip_space(bytes, at + 1);
        while bytes.get(at).is_some_and(|byte| is_word(*byte)) {
            at += 1;
        }
        at = skip_space(bytes, at);
    }
    colon_at(bytes, at)
}

/// True for a `:` that is not part of `::` or a URL's `://`.
fn colon_at(bytes: &[u8], at: usize) -> bool {
    bytes.get(at) == Some(&b':') && !matches!(bytes.get(at + 1), Some(b':' | b'/'))
}

/// True when the value at `start` is a function: `ident:`, `ident@{ ... }:`, `{ ... }:` or `{ ... }@ident:`.
fn is_function(bytes: &[u8], start: usize) -> bool {
    match bytes.get(start) {
        Some(b'{') => is_pattern(bytes, matching(bytes, start) + 1),
        Some(byte) if is_word(*byte) => {
            let mut at = start;
            while bytes.get(at).is_some_and(|byte| is_word(*byte)) {
                at += 1;
            }
            at = skip_space(bytes, at);
            if bytes.get(at) == Some(&b'@') {
                at = skip_space(bytes, at + 1);
                return bytes.get(at) == Some(&b'{') && is_pattern(bytes, matching(bytes, at) + 1);
            }
            colon_at(bytes, at)
        }
        _ => false,
    }
}

/// Offset of the `;` that ends the binding starting at `from`, or `close`. A `;` inside brackets, a `let` or after
/// `with`/`assert` does not end it.
fn statement_end(bytes: &[u8], from: usize, close: usize) -> usize {
    let mut depth = 0usize;
    let mut lets = 0usize;
    let mut pending = 0usize;
    let mut at = from;
    while at < close {
        if let Some(word) = word_at(bytes, at) {
            if depth == 0 {
                match word {
                    b"let" => lets += 1,
                    b"in" => lets = lets.saturating_sub(1),
                    b"with" | b"assert" => pending += 1,
                    _ => {}
                }
            }
            at += word.len();
            continue;
        }
        match bytes[at] {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => depth = depth.saturating_sub(1),
            b';' if depth == 0 && lets == 0 => {
                if pending == 0 {
                    return at;
                }
                pending -= 1;
            }
            _ => {}
        }
        at += 1;
    }
    close
}

/// Copy of `text` with comments and strings replaced by spaces. Newlines and every byte offset stay put.
fn mask_nix(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let mut at = 0usize;
    while at < bytes.len() {
        let rest = &bytes[at..];
        let end = if rest.starts_with(b"#") {
            Some(rest.iter().position(|byte| *byte == b'\n').map_or(bytes.len(), |offset| at + offset))
        } else if rest.starts_with(b"/*") {
            Some(rest.windows(2).skip(2).position(|pair| pair == b"*/").map_or(bytes.len(), |offset| at + offset + 4))
        } else if rest.starts_with(b"\"") {
            Some(quoted_end(bytes, at))
        } else if rest.starts_with(b"''") && (at == 0 || !is_word(bytes[at - 1])) {
            Some(indented_end(bytes, at))
        } else {
            None
        };
        match end {
            Some(end) => {
                for byte in out.iter_mut().take(end).skip(at) {
                    if *byte != b'\n' && *byte != b'\r' {
                        *byte = b' ';
                    }
                }
                at = end.max(at + 1);
            }
            None => at += 1,
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// End of the `"..."` string that opens at `from`. A `${ ... }` interpolation is skipped whole; an unclosed string runs to the end.
fn quoted_end(bytes: &[u8], from: usize) -> usize {
    let mut at = from + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'"' => return at + 1,
            b'$' if bytes.get(at + 1) == Some(&b'{') => at = matching(bytes, at + 1) + 1,
            _ => at += 1,
        }
    }
    bytes.len()
}

/// End of the `''...''` string that opens at `from`. `'''`, `''$` and `''\x` are escapes; an unclosed string runs to the end.
fn indented_end(bytes: &[u8], from: usize) -> usize {
    let mut at = from + 2;
    while at < bytes.len() {
        let rest = &bytes[at..];
        if rest.starts_with(b"'''") || rest.starts_with(b"''$") || rest.starts_with(b"''\\") {
            at += 3;
        } else if rest.starts_with(b"''") {
            return at + 2;
        } else if rest.starts_with(b"${") {
            at = matching(bytes, at + 1) + 1;
        } else {
            at += 1;
        }
    }
    bytes.len()
}
