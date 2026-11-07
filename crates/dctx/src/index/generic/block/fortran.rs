// SPDX-License-Identifier: Apache-2.0
//! Fortran: programs, modules, derived types, procedures and named interfaces.
//! Fortran is a block-family language: `program`, `module`, `function`, `subroutine`, `type` and `interface` open a block and a
//! bare `end` (also `end function name`, `endsubroutine`) closes it. `end if`, `end do`, `end select` and friends close
//! constructs the scanner never opens, so the mask blanks those lines; it also blanks `module procedure`, `select type`,
//! `type is`/`class is` and the `type(...)` of a declaration, none of which declares or opens anything.
//! `module m`, `type :: t` and named `interface i` are types; `[prefix] [type-spec] function f(` and `subroutine s(` are fns
//! named `module::f` by the scanner; `program p` is a fn so calls made by the main program are references too.
//! Keywords are case-insensitive, names keep their spelling. Fixed-form column rules are not modelled; only `!` comments.
//! Calls live in `calls_block.rs`. Heuristic by design: one declaration per line, bodies never entered.
//! Consumes authorized UTF-8 from extract; never panics on malformed or truncated input.

use super::super::{Syntax, ident_at, is_denied, mask};
use super::{Block, Tok};

/// How Fortran reads: `!` line comments, `'` and `"` strings, blocks closed by `end`.
pub(super) const SCAN: Block = Block {
    mask: mask_of,
    decl: decl_of,
    word: word_of,
    flat: false,
    nested: true,
};

/// Words after `end` that close one of the blocks the scanner opens.
const CLOSERS: &[&str] = &[
    "function",
    "subroutine",
    "module",
    "program",
    "type",
    "interface",
];

/// Words in `line` as `(start, end)` byte ranges.
fn word_ranges(line: &[u8]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut at = 0usize;
    while at < line.len() {
        if line[at].is_ascii_alphanumeric() || line[at] == b'_' {
            let start = at;
            while at < line.len() && (line[at].is_ascii_alphanumeric() || line[at] == b'_') {
                at += 1;
            }
            found.push((start, at));
        } else {
            at += 1;
        }
    }
    found
}

/// True when the word at `range` of `line` equals `keyword`, ignoring case.
fn is_word(line: &[u8], range: Option<&(usize, usize)>, keyword: &str) -> bool {
    range.is_some_and(|(start, end)| line[*start..*end].eq_ignore_ascii_case(keyword.as_bytes()))
}

/// Rewrites one masked line in place, keeping every offset, so only real block words are left.
fn tidy(line: &mut [u8]) {
    let words = word_ranges(line);
    let Some(&(start, end)) = words.first() else {
        return;
    };
    let first = String::from_utf8_lossy(&line[start..end]).to_ascii_lowercase();
    let second = words.get(1);
    let blank_all = |line: &mut [u8]| line.fill(b' ');
    match first.as_str() {
        "end" => {
            if second.is_some_and(|(s, e)| {
                !CLOSERS
                    .iter()
                    .any(|word| line[*s..*e].eq_ignore_ascii_case(word.as_bytes()))
                    && line[*s..*e].iter().all(u8::is_ascii_alphabetic)
            }) {
                blank_all(line);
            } else {
                line[end..].fill(b' ');
            }
        }
        "select" => blank_all(line),
        "module" if is_word(line, second, "procedure") => blank_all(line),
        "type" | "class"
            if is_word(line, second, "is")
                || (first == "class" && is_word(line, second, "default")) =>
        {
            blank_all(line)
        }
        "type" if line[end..].iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b'(') => {
            line[start..end].fill(b' ')
        }
        word if word.starts_with("end") => {
            if CLOSERS.contains(&&word[3..]) {
                line[start + 3..].fill(b' ');
            } else {
                blank_all(line);
            }
        }
        _ => {}
    }
    // A `module` prefix of `module function` / `module subroutine` (a submodule procedure) opens nothing by itself.
    let words = word_ranges(line);
    for pair in words.windows(2) {
        let (module, next) = (pair[0], pair[1]);
        if line[module.0..module.1].eq_ignore_ascii_case(b"module")
            && (is_word(line, Some(&next), "function") || is_word(line, Some(&next), "subroutine"))
        {
            line[module.0..module.1].fill(b' ');
        }
    }
}

/// Fortran: `!` comments and strings blanked, then block-less `end` forms normalised line by line.
fn mask_of(text: &str) -> String {
    let masked = mask(
        text,
        &Syntax {
            line: &["!"],
            block: None,
            nested: false,
            quotes: b"\"'",
            triple: false,
        },
    );
    let mut bytes = masked.into_bytes();
    for line in bytes.split_mut(|byte| *byte == b'\n') {
        tidy(line);
    }
    String::from_utf8(bytes).unwrap_or_default()
}

/// Fortran blocks open at `program`, `module`, `function`, `subroutine`, `type`, `interface` and close at `end`.
fn word_of(word: &str, _next: Option<u8>) -> Tok {
    if word.eq_ignore_ascii_case("end") {
        return Tok::Close;
    }
    let opens = [
        "program",
        "module",
        "function",
        "subroutine",
        "type",
        "interface",
    ];
    if opens
        .iter()
        .any(|keyword| word.eq_ignore_ascii_case(keyword))
    {
        Tok::Open
    } else {
        Tok::Neutral
    }
}

/// The declared name when `text` (a trimmed line tail) starts with one, skipping an attribute list up to `::`.
fn name_after(text: &str) -> Option<&str> {
    let text = match text.find("::") {
        Some(cut) if text.trim_start().starts_with(',') || text[..cut].trim().is_empty() => {
            &text[cut + 2..]
        }
        _ => text,
    };
    let name = ident_at(text.trim_start())?;
    (!is_denied(name)).then_some(name)
}

/// `program p`, `module m`, `type [, attrs] [::] t`, named `interface i` and `[prefix] function f(` / `subroutine s(`.
fn decl_of(line: &str, _source: &str, _owner: Option<&str>) -> Option<(String, &'static str)> {
    let lower = line.to_ascii_lowercase();
    let first_end = lower
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(lower.len());
    let first = &lower[..first_end];
    let rest = &line[first_end..];
    match first {
        "program" => return name_after(rest).map(|name| (name.to_string(), "fn")),
        "module" if rest.starts_with([' ', '\t']) => {
            return name_after(rest).map(|name| (name.to_string(), "type"));
        }
        "type" if !rest.trim_start().starts_with('(') => {
            return name_after(rest).map(|name| (name.to_string(), "type"));
        }
        "abstract" | "interface" => {
            let after = if first == "abstract" {
                lower[first_end..]
                    .trim_start()
                    .strip_prefix("interface")
                    .map(|tail| &line[line.len() - tail.len()..])
            } else {
                Some(rest)
            };
            let name = name_after(after?)?;
            return (!matches!(
                name.to_ascii_lowercase().as_str(),
                "operator" | "assignment" | "read" | "write"
            ))
            .then(|| (name.to_string(), "type"));
        }
        _ => {}
    }
    // A procedure: the keyword is a word before any `::`, `=` or parenthesis (so `integer :: function` and `x = f(` do not count).
    let head = &lower[..lower.find(['=', ':']).unwrap_or(lower.len())];
    let mut depth = 0usize;
    let mut at = 0usize;
    let bytes = head.as_bytes();
    while at < bytes.len() {
        match bytes[at] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            byte if depth == 0 && byte.is_ascii_alphabetic() => {
                let end = bytes[at..]
                    .iter()
                    .position(|b| !(b.is_ascii_alphanumeric() || *b == b'_'))
                    .map_or(bytes.len(), |offset| at + offset);
                if matches!(&head[at..end], "function" | "subroutine") {
                    let name = ident_at(line[end..].trim_start())?;
                    return (!is_denied(name)).then(|| (name.to_string(), "fn"));
                }
                at = end;
                continue;
            }
            _ => {}
        }
        at += 1;
    }
    None
}
