// SPDX-License-Identifier: Apache-2.0
//! Byte scanner with explicit PHP/HTML modes and opaque comments and literals.
//! Implements PHP lexical boundaries for decision 077 without executing source.
//! Produces tokens consumed by delimiter pairing and structural extraction.
use super::{Kind, Lexed, Token, strings};
use crate::domain::{DiagnosticReason, LexError, Limits};

pub(super) fn scan(src: &str, out: &mut Lexed, limits: &Limits) -> Result<(), LexError> {
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut php = false;
    while i < bytes.len() {
        if !php {
            if src.get(i..i + 3) == Some("<?=") {
                php = true;
                i += 3;
            } else if src
                .get(i..i + 5)
                .is_some_and(|s| s.eq_ignore_ascii_case("<?php"))
                && bytes.get(i + 5).is_none_or(u8::is_ascii_whitespace)
            {
                php = true;
                i += 5;
            } else {
                i += src[i..].chars().next().map_or(1, char::len_utf8);
            }
            continue;
        }
        let start = i;
        let rest = &src[i..];
        let b = bytes[i];
        let kind = if rest.starts_with("?>") {
            php = false;
            i += 2;
            Kind::Boundary
        } else if b.is_ascii_whitespace() {
            i += 1;
            continue;
        } else if rest.starts_with("//") || (b == b'#' && !rest.starts_with("#[")) {
            i += if b == b'#' { 1 } else { 2 };
            while i < bytes.len() && bytes[i] != b'\n' && !src[i..].starts_with("?>") {
                i += src[i..].chars().next().map_or(1, char::len_utf8);
            }
            out.comments.push((start, i));
            continue;
        } else if rest.starts_with("/*") {
            // Only `//` and `#` comments end at `?>`. A block comment ends at `*/`.
            if let Some(end) = src[i + 2..].find("*/") {
                i += end + 4;
            } else {
                i = bytes.len();
                out.problem(start, DiagnosticReason::UnterminatedComment);
            }
            out.comments.push((start, i));
            continue;
        } else if matches!(b, b'\'' | b'"' | b'`') || rest.starts_with("<<<") {
            let literal = if rest.starts_with("<<<") {
                strings::heredoc(src, i)
            } else {
                strings::quoted(src, i, limits, out.line(i))?
            };
            i = literal.end;
            if let Some(problem) = literal.problem {
                out.problem(start, problem);
            }
            literal.kind
        } else if b == b'$' && bytes.get(i + 1).is_some_and(|b| ident_start(*b)) {
            i += 2;
            while bytes.get(i).is_some_and(|b| ident_continue(*b)) {
                i += 1;
            }
            Kind::Variable
        } else if ident_start(b) {
            i += 1;
            while bytes.get(i).is_some_and(|b| ident_continue(*b)) {
                i += 1;
            }
            Kind::Ident
        } else if b.is_ascii_digit() {
            i += 1;
            while bytes
                .get(i)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'.' || *b == b'_')
            {
                i += 1;
            }
            Kind::Number
        } else {
            let length = [
                "?->", "...", "#[", "::", "->", "=>", "??", "&&", "||", "==", "!=", "<=", ">=",
                "++", "--", "**",
            ]
            .iter()
            .find(|op| rest.starts_with(**op))
            .map_or(1, |op| op.len());
            i += length;
            Kind::Punct
        };
        out.tokens.push(Token {
            kind,
            start,
            end: i,
        });
    }
    Ok(())
}

fn ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 128
}
fn ident_continue(b: u8) -> bool {
    ident_start(b) || b.is_ascii_digit()
}
