// SPDX-License-Identifier: Apache-2.0
//! Preprocessor directives as facts, never as expanded tokens.
//! Implements the C and C++ translation phases for `#include` and `#define` only.
//! The structural walker records the result and does not rescan macro bodies.

/// One directive after the leading `#` and its keyword.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Dir {
    /// `#include` / `#include_next`. `None` means the path is a macro; abstain.
    Include(Option<String>),
    /// `#define`. `None` means the directive has no macro name.
    Define(Option<MacroDef>),
    /// `#undef` of one identifier.
    Undef(String),
    /// `#if`, `#ifdef` or `#ifndef`.
    Open,
    /// `#elif`, `#elifdef`, `#elifndef` or `#else`.
    Branch,
    /// `#endif`.
    Close,
    /// A directive this extractor does not record (`#pragma`, `#line`, `#error`).
    Other,
}

/// A macro name and whether the `(` of a function-like macro is immediate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MacroDef {
    /// Identifier after `#define`.
    pub name: String,
    /// True when `(` follows the name with no whitespace.
    pub function_like: bool,
}

/// Classifies one directive token, comments stripped, macros not expanded.
pub(crate) fn classify(text: &str) -> Dir {
    let cleaned = scrub(text);
    let (keyword, rest) = split_keyword(&cleaned);
    match keyword {
        "include" | "include_next" => Dir::Include(include_path(rest)),
        "define" => Dir::Define(macro_def(rest)),
        "undef" => ident_at(rest).map_or(Dir::Other, |name| Dir::Undef(name.to_string())),
        "if" | "ifdef" | "ifndef" => Dir::Open,
        "elif" | "elifdef" | "elifndef" | "else" => Dir::Branch,
        "endif" => Dir::Close,
        _ => Dir::Other,
    }
}

/// Diagnostic for a directive that names a macro where a path or declaration was required.
pub(crate) fn hidden_include(dir: &Dir) -> bool {
    matches!(dir, Dir::Include(None))
}

fn split_keyword(text: &str) -> (&str, &str) {
    let text = text.trim().trim_start_matches('#').trim_start();
    let end = text
        .find(|ch: char| !(ch == '_' || ch.is_ascii_alphanumeric()))
        .unwrap_or(text.len());
    let (keyword, rest) = text.split_at(end);
    (keyword, rest.trim_start())
}

/// `Some(true)` when an object-like macro's replacement is `{`, `Some(false)` for `}`.
///
/// Function-like macros and any other body stay `None`. Invocations of these two
/// shapes stand in for braces in real headers (`BEGIN` / `END`).
pub(crate) fn define_brace(text: &str) -> Option<bool> {
    let cleaned = scrub(text);
    let (keyword, rest) = split_keyword(&cleaned);
    if keyword != "define" {
        return None;
    }
    let name = ident_at(rest)?;
    let after = rest.trim_start().strip_prefix(name)?;
    if after.starts_with('(') {
        return None;
    }
    match after.trim() {
        "{" => Some(true),
        "}" => Some(false),
        _ => None,
    }
}

/// Drops comments but keeps directive text that follows them, including inside strings.
fn scrub(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        if raw_boundary(text, index)
            && let Some(end) = raw_end(bytes, index)
        {
            out.push_str(&text[index..end]);
            index = end;
            continue;
        }
        let byte = bytes[index];
        if byte == b'"' || byte == b'\'' {
            let end = quoted_end(bytes, index, byte);
            out.push_str(&text[index..end]);
            index = end;
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            if index + 1 < bytes.len() {
                index += 2;
            } else {
                index = bytes.len();
            }
            out.push(' ');
            continue;
        }
        let ch = text[index..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn raw_boundary(text: &str, index: usize) -> bool {
    if index > 0 {
        let prev = text[..index].chars().next_back().unwrap_or('\0');
        if prev == '_' || prev == '$' || prev.is_alphanumeric() {
            return false;
        }
    }
    let rest = &text.as_bytes()[index..];
    [b"u8R\"".as_slice(), b"uR\"", b"UR\"", b"LR\"", b"R\""]
        .iter()
        .any(|prefix| rest.starts_with(prefix))
}

fn raw_end(bytes: &[u8], index: usize) -> Option<usize> {
    let rest = &bytes[index..];
    let prefix_len = [b"u8R\"".as_slice(), b"uR\"", b"UR\"", b"LR\"", b"R\""]
        .iter()
        .find(|prefix| rest.starts_with(prefix))
        .map(|prefix| prefix.len())?;
    let mut pos = index + prefix_len;
    let delim_at = pos;
    while pos < bytes.len()
        && bytes[pos] != b'('
        && bytes[pos] != b'\n'
        && bytes[pos] != b'"'
        && pos - delim_at < 16
    {
        pos += 1;
    }
    if pos >= bytes.len() || bytes[pos] != b'(' {
        return None;
    }
    let delim = bytes[delim_at..pos].to_vec();
    pos += 1;
    while pos < bytes.len() {
        if bytes[pos] == b')'
            && bytes[pos + 1..].starts_with(&delim)
            && bytes.get(pos + 1 + delim.len()) == Some(&b'"')
        {
            return Some(pos + 2 + delim.len());
        }
        pos += 1;
    }
    Some(bytes.len())
}

fn quoted_end(bytes: &[u8], index: usize, quote: u8) -> usize {
    let mut pos = index + 1;
    while pos < bytes.len() {
        let byte = bytes[pos];
        if byte == quote {
            return pos + 1;
        }
        if byte == b'\n' {
            return pos;
        }
        pos += 1;
        if byte == b'\\' && pos < bytes.len() && bytes[pos] != b'\n' {
            pos += 1;
        }
    }
    pos
}

fn include_path(rest: &str) -> Option<String> {
    let rest = rest.trim_start();
    if let Some(body) = rest.strip_prefix('<') {
        let end = body.find('>')?;
        let path = body[..end].trim();
        if path.is_empty() || path.chars().any(char::is_whitespace) {
            return None;
        }
        return Some(path.to_string());
    }
    if let Some(body) = rest.strip_prefix('"') {
        let end = body.find('"')?;
        if end == 0 {
            return None;
        }
        return Some(body[..end].to_string());
    }
    None
}

fn macro_def(rest: &str) -> Option<MacroDef> {
    let name = ident_at(rest)?;
    let function_like = rest
        .trim_start()
        .strip_prefix(name)
        .is_some_and(|after| after.starts_with('('));
    Some(MacroDef {
        name: name.to_string(),
        function_like,
    })
}

/// Whether a `#define` body is an attribute or expands away.
///
/// `__attribute__`, `__declspec`, `[[...]]` and an empty body are attributes.
/// A body that is an identifier still hides the declaration it stands in for.
pub(crate) fn define_attribute(text: &str) -> bool {
    let cleaned = scrub(text);
    let (keyword, rest) = split_keyword(&cleaned);
    if keyword != "define" {
        return false;
    }
    let Some(name) = ident_at(rest) else {
        return false;
    };
    let Some(after) = rest.trim_start().strip_prefix(name) else {
        return false;
    };
    let body = macro_body(after).trim();
    body.is_empty()
        || body.contains("__attribute__")
        || body.contains("__declspec")
        || body.contains("[[")
}

/// Replacement list after an optional function-like parameter list.
fn macro_body(after_name: &str) -> &str {
    let Some(rest) = after_name.strip_prefix('(') else {
        return after_name;
    };
    let bytes = rest.as_bytes();
    let mut depth = 1i32;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[index + 1..];
                }
            }
            _ => {}
        }
        index += 1;
    }
    ""
}

fn ident_at(rest: &str) -> Option<&str> {
    let rest = rest.trim_start();
    let mut chars = rest.chars();
    let first = chars.next()?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }
    let end = rest
        .find(|ch: char| !(ch == '_' || ch.is_ascii_alphanumeric()))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn include_define_and_macro_paths_are_classified() {
        assert_eq!(
            classify("#include <stdio.h>"),
            Dir::Include(Some("stdio.h".into()))
        );
        assert_eq!(
            classify("# include \"box.h\" // note"),
            Dir::Include(Some("box.h".into()))
        );
        assert_eq!(classify("#include FOO"), Dir::Include(None));
        assert!(hidden_include(&classify("#include MACRO")));
        match classify("#define SHOW(x) ((x)+1)") {
            Dir::Define(Some(def)) => {
                assert_eq!(def.name, "SHOW");
                assert!(def.function_like);
            }
            other => panic!("{other:?}"),
        }
        match classify("#define LIMIT 3") {
            Dir::Define(Some(def)) => {
                assert_eq!(def.name, "LIMIT");
                assert!(!def.function_like);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            classify("#define NAME (x)"),
            Dir::Define(Some(MacroDef {
                name: "NAME".into(),
                function_like: false,
            }))
        );
        assert_eq!(classify("#undef SHOW"), Dir::Undef("SHOW".into()));
        assert_eq!(classify("#ifdef EXTRA"), Dir::Open);
        assert_eq!(classify("#else"), Dir::Branch);
        assert_eq!(classify("#endif"), Dir::Close);
        assert_eq!(classify("#pragma once"), Dir::Other);
    }

    #[test]
    fn comments_between_a_directive_and_its_operand_stay() {
        assert_eq!(
            classify("#include /* note */ <extra.h>"),
            Dir::Include(Some("extra.h".into()))
        );
        assert_eq!(
            classify("#include_next /* note */ \"next.h\""),
            Dir::Include(Some("next.h".into()))
        );
        match classify("#define FOO /* comment\nint leaked(void);\nstill */ 1") {
            Dir::Define(Some(def)) => {
                assert_eq!(def.name, "FOO");
                assert!(!def.function_like);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(define_brace("#define BEGIN { // open"), Some(true));
        assert_eq!(define_brace("#define END }"), Some(false));
        assert_eq!(define_brace("#define SHOW(x) { x }"), None);
        assert_eq!(define_brace("#define LIMIT 3"), None);
    }
}
