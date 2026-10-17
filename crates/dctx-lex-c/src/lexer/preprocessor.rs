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
    let (keyword, rest) = split_keyword(text);
    let rest = strip_comment(rest);
    match keyword {
        "include" | "include_next" => Dir::Include(include_path(rest)),
        "define" => Dir::Define(macro_def(rest)),
        "undef" => ident_at(rest)
            .map(|name| Dir::Undef(name.to_string()))
            .unwrap_or(Dir::Other),
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

fn strip_comment(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut quote = 0u8;
    while index < bytes.len() {
        let byte = bytes[index];
        if quote != 0 {
            if byte == quote {
                quote = 0;
            } else if byte == b'\\' {
                index += 1;
            }
            index += 1;
            continue;
        }
        if byte == b'"' || byte == b'\'' {
            quote = byte;
            index += 1;
            continue;
        }
        if byte == b'/' && matches!(bytes.get(index + 1), Some(b'/' | b'*')) {
            return text[..index].trim_end();
        }
        index += 1;
    }
    text.trim()
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
}
