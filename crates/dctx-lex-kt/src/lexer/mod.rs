// SPDX-License-Identifier: Apache-2.0
//! Token stream for Kotlin source, including templates and newline statement ends.
//! Implements PARSERS-v1 lexing: comments, strings and brackets are bounded by `Limits`.
//! The structural walker consumes this. `>` is always one character.
mod scan;

use crate::domain::{Diagnostic, DiagnosticReason, LexError, Limits};

/// Token category. Keywords stay [`Kind::Ident`] and are compared by text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Identifier or keyword.
    Ident,
    /// Numeric literal.
    Num,
    /// Quoted string piece. Template holes are separate tokens.
    Str,
    /// Raw-string piece, quotes not required on every piece.
    Raw,
    /// Character literal.
    Char,
    /// Unterminated quoted string.
    BadStr,
    /// Unterminated raw string.
    BadRaw,
    /// Unterminated character literal.
    BadChar,
    /// Operator or delimiter other than semicolon.
    Punct,
    /// Explicit or inserted statement end.
    Semi,
}

/// One significant token. An inserted semicolon has an empty span.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    /// Category.
    pub kind: Kind,
    /// Inclusive byte start.
    pub start: u32,
    /// Exclusive byte end. Equal to `start` for an inserted semicolon.
    pub end: u32,
    /// `1 + index` of an adjacent preceding `KDoc` comment, or zero.
    pub doc: u32,
}

/// A comment span. `KDoc` is `/** ... */` and not the empty `/**/`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Comment {
    /// Inclusive byte start.
    pub start: u32,
    /// Exclusive byte end.
    pub end: u32,
    /// Whether the comment is `KDoc`.
    pub javadoc: bool,
}

/// Lexed tokens, comments, bracket partners and problems.
pub(crate) struct Lexed {
    /// Significant tokens in source order.
    pub tokens: Vec<Token>,
    /// Comments in source order.
    pub comments: Vec<Comment>,
    /// Partner token index, or [`NONE`] when the bracket is unmatched.
    pub partner: Vec<u32>,
    /// Lexical problems in source order.
    pub diagnostics: Vec<Diagnostic>,
    /// Byte offset of each line, starting at zero.
    pub line_starts: Vec<u32>,
}

/// Partner value of a bracket without a match.
pub(crate) const NONE: u32 = u32::MAX;

/// Problems found while lexing, as `(byte, reason)`.
pub(crate) type Problems = Vec<(u32, DiagnosticReason)>;

impl Lexed {
    /// One-based line of a byte offset. Rows advance on `\n` only.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
    )]
    pub fn line(&self, byte: u32) -> u32 {
        self.line_starts.partition_point(|&start| start <= byte) as u32
    }
}

/// Hard keywords. Soft keywords (`import`, `constructor`, modifiers) stay identifiers.
pub(crate) fn keyword(word: &str) -> bool {
    KEYWORDS.binary_search(&word).is_ok()
}

const KEYWORDS: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "when",
    "while",
];

/// Lexes `source` and pairs `()`, `[]` and `{}`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
)]
pub(crate) fn lex(source: &str, limits: &Limits) -> Result<Lexed, LexError> {
    let mut line_starts = vec![0];
    for (index, &byte) in source.as_bytes().iter().enumerate() {
        if byte == b'\n' || (byte == b'\r' && source.as_bytes().get(index + 1) != Some(&b'\n')) {
            line_starts.push(index as u32 + 1);
        }
    }
    let mut lexer = scan::Lexer::new(source);
    lexer
        .run(limits.max_depth)
        .map_err(|byte| LexError::TooDeep {
            line: line_of(&line_starts, byte),
            limit: limits.max_depth,
        })?;
    let mut problems = lexer.problems;
    let tokens = lexer.tokens;
    let partner = pair(
        source,
        &tokens,
        &mut problems,
        limits.max_depth,
        &line_starts,
    )?;
    problems.sort_by_key(|problem| problem.0);
    let diagnostics = problems
        .into_iter()
        .map(|(byte, reason)| Diagnostic {
            line: line_of(&line_starts, byte),
            byte,
            reason,
        })
        .collect();
    Ok(Lexed {
        tokens,
        comments: lexer.comments,
        partner,
        diagnostics,
        line_starts,
    })
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
)]
fn line_of(line_starts: &[u32], byte: u32) -> u32 {
    line_starts.partition_point(|&start| start <= byte) as u32
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
)]
fn pair(
    source: &str,
    tokens: &[Token],
    problems: &mut Problems,
    max_depth: u32,
    line_starts: &[u32],
) -> Result<Vec<u32>, LexError> {
    let mut partner = vec![NONE; tokens.len()];
    let mut stack = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != Kind::Punct {
            continue;
        }
        let text = slice(source, token.start, token.end);
        match text {
            "(" | "[" | "{" => {
                if stack.len() as u32 >= max_depth {
                    return Err(LexError::TooDeep {
                        line: line_of(line_starts, token.start),
                        limit: max_depth,
                    });
                }
                stack.push(index);
            }
            ")" | "]" | "}" => {
                close_bracket(
                    source,
                    tokens,
                    index,
                    text,
                    &mut stack,
                    &mut partner,
                    problems,
                );
            }
            _ => {}
        }
    }
    for open in stack {
        problems.push((tokens[open].start, DiagnosticReason::UnbalancedBracket));
    }
    Ok(partner)
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
)]
fn close_bracket(
    source: &str,
    tokens: &[Token],
    index: usize,
    text: &str,
    stack: &mut Vec<usize>,
    partner: &mut [u32],
    problems: &mut Problems,
) {
    let wanted = match text {
        ")" => "(",
        "]" => "[",
        _ => "{",
    };
    let found = stack.iter().rposition(|&open| {
        let token = &tokens[open];
        slice(source, token.start, token.end) == wanted
    });
    let Some(position) = found else {
        problems.push((tokens[index].start, DiagnosticReason::UnbalancedBracket));
        return;
    };
    while stack.len() > position + 1 {
        if let Some(orphan) = stack.pop() {
            problems.push((tokens[orphan].start, DiagnosticReason::UnbalancedBracket));
        }
    }
    if let Some(open) = stack.pop() {
        partner[open] = index as u32;
        partner[index] = open as u32;
    }
}

fn slice(source: &str, start: u32, end: u32) -> &str {
    source.get(start as usize..end as usize).unwrap_or("")
}

impl scan::Lexer<'_> {
    fn scan_number(&mut self) {
        let marked = self.b.get(self.pos) == Some(&b'0')
            && matches!(self.b.get(self.pos + 1), Some(b'x' | b'X' | b'b' | b'B'));
        let hex = matches!(self.b.get(self.pos + 1), Some(b'x' | b'X'));
        if marked {
            self.pos += 2;
            self.scan_digits(hex);
        } else {
            self.scan_digits(false);
            if self.b.get(self.pos) == Some(&b'.')
                && self.b.get(self.pos + 1).is_some_and(u8::is_ascii_digit)
            {
                self.pos += 1;
                self.scan_digits(false);
            }
            if matches!(self.b.get(self.pos), Some(b'e' | b'E')) {
                self.scan_exponent();
            }
        }
        if matches!(self.b.get(self.pos), Some(b'f' | b'F' | b'L' | b'u' | b'U')) {
            self.pos += 1;
        }
    }

    fn scan_digits(&mut self, hex: bool) {
        while self.pos < self.b.len() {
            let byte = self.b[self.pos];
            let digit = if hex {
                byte.is_ascii_hexdigit()
            } else {
                byte.is_ascii_digit()
            };
            if digit || byte == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn scan_exponent(&mut self) {
        let next = self.pos + 1;
        let sign = matches!(self.b.get(next), Some(b'+' | b'-'));
        let digit_at = if sign { next + 1 } else { next };
        if self.b.get(digit_at).is_some_and(u8::is_ascii_digit) {
            self.pos = digit_at;
            self.scan_digits(false);
        }
    }

    fn on_newline(&mut self) {
        if self.insert && self.groups == 0 && !self.next_suppresses() {
            self.push_semi(self.pos);
        }
        self.insert = false;
        if self.b[self.pos] == b'\r' && self.b.get(self.pos + 1) == Some(&b'\n') {
            self.pos += 1;
        }
        self.pos += 1;
    }

    fn next_suppresses(&self) -> bool {
        let mut index = self.pos + 1;
        while index < self.b.len() {
            let byte = self.b[index];
            if matches!(byte, b' ' | b'\t' | b'\r' | b'\n') {
                index += 1;
                continue;
            }
            if byte == b'/' && self.b.get(index + 1) == Some(&b'/') {
                index += 2;
                while index < self.b.len() && !matches!(self.b[index], b'\r' | b'\n') {
                    index += 1;
                }
                continue;
            }
            if byte == b'/' && self.b.get(index + 1) == Some(&b'*') {
                index = self.skip_block_ahead(index + 2);
                continue;
            }
            return suppresses(self.src, index);
        }
        false
    }

    fn skip_block_ahead(&self, mut index: usize) -> usize {
        let mut depth = 1u32;
        while index < self.b.len() && depth > 0 {
            if self.b[index] == b'/' && self.b.get(index + 1) == Some(&b'*') {
                depth += 1;
                index += 2;
                continue;
            }
            if self.b[index] == b'*' && self.b.get(index + 1) == Some(&b'/') {
                depth -= 1;
                index += 2;
                continue;
            }
            index += 1;
        }
        index
    }

    fn shebang(&mut self) {
        if self.b.starts_with(b"#!") {
            while self.pos < self.b.len() && !matches!(self.b[self.pos], b'\r' | b'\n') {
                self.pos += 1;
            }
        }
    }

    fn skip_space(&mut self) {
        while self.pos < self.b.len() && matches!(self.b[self.pos], b' ' | b'\t') {
            self.pos += 1;
        }
    }

    fn skip_gap(&mut self) {
        loop {
            self.skip_space();
            if self.pos < self.b.len() && matches!(self.b[self.pos], b'\r' | b'\n') {
                self.pos += 1;
                continue;
            }
            break;
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
    )]
    fn push(&mut self, kind: Kind, start: usize) {
        let end = self.pos;
        let doc = self.doc_for(start as u32);
        self.track_group(kind, start, end);
        self.tokens.push(Token {
            kind,
            start: start as u32,
            end: end as u32,
            doc,
        });
        self.insert = ends_stmt(kind, self.snippet(start, end));
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
    )]
    fn push_semi(&mut self, at: usize) {
        let at = at as u32;
        self.tokens.push(Token {
            kind: Kind::Semi,
            start: at,
            end: at,
            doc: 0,
        });
        self.insert = false;
    }

    fn track_group(&mut self, kind: Kind, start: usize, end: usize) {
        if kind != Kind::Punct {
            return;
        }
        match self.snippet(start, end) {
            "(" | "[" => self.groups += 1,
            ")" | "]" => self.groups = self.groups.saturating_sub(1),
            _ => {}
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
    )]
    fn doc_for(&self, start: u32) -> u32 {
        let Some((index, comment)) = self.comments.iter().enumerate().next_back() else {
            return 0;
        };
        if !comment.javadoc || comment.end > start {
            return 0;
        }
        let prev = self.tokens.last().map_or(0, |token| token.end);
        if comment.start < prev || !gap_ok(self.src, comment.end, start) {
            return 0;
        }
        index as u32 + 1
    }

    fn snippet(&self, start: usize, end: usize) -> &str {
        self.src.get(start..end).unwrap_or("")
    }

    fn peek(&self, byte: u8) -> bool {
        self.b.get(self.pos) == Some(&byte)
    }

    fn comment_at(&self, index: usize) -> bool {
        matches!(self.b.get(index + 1), Some(b'/' | b'*'))
    }

    fn char_at(&self, index: usize) -> char {
        self.src
            .get(index..)
            .and_then(|rest| rest.chars().next())
            .unwrap_or('\u{FFFD}')
    }

    fn bump(&mut self) {
        if self.pos >= self.b.len() {
            return;
        }
        let width = self.char_at(self.pos).len_utf8().max(1);
        self.pos += width;
    }
}

fn ends_stmt(kind: Kind, text: &str) -> bool {
    match kind {
        Kind::Ident => !text.starts_with('`') && scan::PREFIX.binary_search(&text).is_err(),
        Kind::Num
        | Kind::Str
        | Kind::Raw
        | Kind::Char
        | Kind::BadStr
        | Kind::BadRaw
        | Kind::BadChar => true,
        Kind::Punct => matches!(text, ")" | "]" | "}" | "!!"),
        Kind::Semi => false,
    }
}

fn suppresses(src: &str, index: usize) -> bool {
    let rest = src.get(index..).unwrap_or("");
    let Some(byte) = rest.as_bytes().first().copied() else {
        return false;
    };
    if matches!(
        byte,
        b'.' | b'{'
            | b':'
            | b'='
            | b','
            | b'+'
            | b'-'
            | b'*'
            | b'/'
            | b'%'
            | b'&'
            | b'|'
            | b')'
            | b']'
            | b'}'
            | b'?'
            | b'<'
            | b'>'
            | b'!'
    ) {
        return true;
    }
    matches!(
        ident_at(rest),
        "else" | "catch" | "finally" | "by" | "where" | "is" | "as" | "in"
    )
}

fn ident_at(rest: &str) -> &str {
    let end = rest
        .char_indices()
        .take_while(|(index, ch)| {
            (*index == 0 && scan::starts_ident(*ch)) || (*index > 0 && scan::continues_ident(*ch))
        })
        .last()
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    rest.get(..end).unwrap_or("")
}

fn gap_ok(src: &str, from: u32, to: u32) -> bool {
    if from > to {
        return false;
    }
    let Some(gap) = src.get(from as usize..to as usize) else {
        return false;
    };
    gap.bytes()
        .enumerate()
        .filter(|&(index, byte)| {
            byte == b'\n' || (byte == b'\r' && gap.as_bytes().get(index + 1) != Some(&b'\n'))
        })
        .count()
        <= 1
        && gap
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

#[cfg(test)]
mod tests;
