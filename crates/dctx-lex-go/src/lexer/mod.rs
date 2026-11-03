// SPDX-License-Identifier: Apache-2.0
//! Token stream for Go source, including automatic semicolon insertion.
//! Implements PARSERS-v1 lexing: comments, raw and interpreted strings, runes
//! and bracket nesting bounded by `Limits`. The structural walker consumes this.
mod scan;

use crate::domain::{Diagnostic, DiagnosticReason, LexError, Limits};

/// Token category. Keywords stay `Ident` and are compared by text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Identifier or keyword.
    Ident,
    /// Numeric literal, including an imaginary `i` suffix.
    Number,
    /// Interpreted string literal, quotes included.
    Str,
    /// Raw string literal, backticks included.
    Raw,
    /// Rune literal, quotes included.
    Rune,
    /// Unterminated interpreted or raw string.
    BadStr,
    /// Unterminated rune literal.
    BadRune,
    /// Operator or delimiter other than semicolon.
    Punct,
    /// Explicit or inserted semicolon.
    Semi,
}

/// One significant token. An inserted semicolon has an empty span (`start == end`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    /// Category.
    pub kind: Kind,
    /// First byte.
    pub start: u32,
    /// One past the last byte. Equal to `start` for an inserted semicolon.
    pub end: u32,
}

/// Lexer output: tokens, comment spans, bracket partners and problems.
pub(crate) struct Lexed {
    /// Significant tokens, with inserted semicolons included.
    pub tokens: Vec<Token>,
    /// Comment byte spans, in source order. Newlines are excluded for `//`.
    pub comments: Vec<(u32, u32)>,
    /// Matching bracket index, or [`NONE`].
    pub partner: Vec<u32>,
    /// Lexical problems, in source order.
    pub diagnostics: Vec<Diagnostic>,
    /// Byte offset of the first byte of each line. Index 0 is byte 0.
    pub line_starts: Vec<u32>,
}

/// Partner value of a bracket without a match.
pub(crate) const NONE: u32 = u32::MAX;

impl Lexed {
    /// One-based line of a byte offset. Rows advance on `\n` only.
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    pub fn line(&self, byte: u32) -> u32 {
        self.line_starts.partition_point(|&start| start <= byte) as u32
    }
}

/// Whether `word` is a Go keyword. `true`, `false` and `nil` are identifiers.
pub(crate) fn keyword(word: &str) -> bool {
    matches!(
        word,
        "break"
            | "case"
            | "chan"
            | "const"
            | "continue"
            | "default"
            | "defer"
            | "else"
            | "fallthrough"
            | "for"
            | "func"
            | "go"
            | "goto"
            | "if"
            | "import"
            | "interface"
            | "map"
            | "package"
            | "range"
            | "return"
            | "select"
            | "struct"
            | "switch"
            | "type"
            | "var"
    )
}

/// Lexes `source` and pairs its brackets.
#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
#[allow(clippy::similar_names, reason = "The scanner and its lexed output have established role names used consistently across lexer adapters.")]
pub(crate) fn lex(source: &str, limits: &Limits) -> Result<Lexed, LexError> {
    let mut line_starts = vec![0];
    line_starts.extend(
        source
            .bytes()
            .enumerate()
            .filter(|(_, byte)| *byte == b'\n')
            .map(|(index, _)| index as u32 + 1),
    );
    let mut lexer = scan::Lexer::new(source);
    lexer.run();
    let mut lexed = Lexed {
        tokens: lexer.tokens,
        comments: lexer.comments,
        partner: Vec::new(),
        diagnostics: Vec::new(),
        line_starts,
    };
    let mut problems = lexer.problems;
    lexed.partner = pair(source, &lexed.tokens, &mut problems, limits.max_depth)?;
    problems.sort_by_key(|problem| problem.0);
    lexed.diagnostics = problems
        .into_iter()
        .map(|(byte, reason)| Diagnostic {
            line: lexed.line(byte),
            byte,
            reason,
        })
        .collect();
    Ok(lexed)
}

/// Problems found while lexing, as `(byte, reason)`.
pub(crate) type Problems = Vec<(u32, DiagnosticReason)>;

#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
fn pair(
    source: &str,
    tokens: &[Token],
    problems: &mut Problems,
    max_depth: u32,
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
                stack.push(index);
                if stack.len() as u32 > max_depth {
                    return Err(LexError::TooDeep {
                        line: line_at(source, token.start),
                        limit: max_depth,
                    });
                }
            }
            ")" | "]" | "}" => {
                close_bracket(source, tokens, index, text, &mut stack, &mut partner, problems);
            }
            _ => {}
        }
    }
    for open in stack {
        problems.push((tokens[open].start, DiagnosticReason::UnbalancedBracket));
    }
    Ok(partner)
}

#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
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

#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
fn line_at(source: &str, byte: u32) -> u32 {
    let end = (byte as usize).min(source.len());
    source[..end].bytes().filter(|&byte| byte == b'\n').count() as u32 + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partners_recover_a_mismatched_closer_and_depth_refuses() {
        let source = "func f() {\n\treturn len(s\n}\n";
        let lexed = lex(source, &Limits::default()).unwrap();
        let brace = lexed
            .tokens
            .iter()
            .position(|token| slice(source, token.start, token.end) == "{")
            .unwrap();
        assert_ne!(lexed.partner[brace], NONE);
        assert!(
            lexed
                .diagnostics
                .iter()
                .any(|d| d.reason == DiagnosticReason::UnbalancedBracket)
        );
        let limits = Limits {
            max_depth: 2,
            ..Limits::default()
        };
        assert!(matches!(
            lex("{{{}}}", &limits),
            Err(LexError::TooDeep { limit: 2, .. })
        ));
        assert!(lex("{{}}", &limits).is_ok());
    }
}
