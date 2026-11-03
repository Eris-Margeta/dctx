// SPDX-License-Identifier: Apache-2.0
//! Token stream for Java source, including text blocks, literals and comments.
//! Implements PARSERS-v1 lexing: comments and literals are opaque, brackets are paired,
//! and nesting past the caller limit is a typed refusal. The structural walker consumes it.
mod scan;

use crate::domain::{Diagnostic, DiagnosticReason, LexError, Limits};

/// Token category. Keywords stay [`Kind::Word`] and are compared by text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Identifier or keyword.
    Word,
    /// Quoted string or text block.
    Str,
    /// Character literal.
    Char,
    /// Numeric literal.
    Num,
    /// Operator or separator. `>` is always one character so `>>` closes two type arguments.
    Punct,
}

/// One significant token. `doc` is `1 + index` of an adjacent preceding Javadoc comment.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    /// Category.
    pub kind: Kind,
    /// Inclusive byte start.
    pub start: u32,
    /// Exclusive byte end.
    pub end: u32,
    /// Javadoc immediately before this token, or zero.
    pub doc: u32,
}

/// A comment span. Javadoc is `/** ... */` and not the empty `/**/`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Comment {
    /// Inclusive byte start.
    pub start: u32,
    /// Exclusive byte end.
    pub end: u32,
    /// Whether the comment is Javadoc.
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

impl Lexed {
    /// One-based line of a byte offset. Rows advance on `\n` only.
    pub fn line(&self, byte: u32) -> u32 {
        line_of(&self.line_starts, byte)
    }
}

#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
pub(crate) fn line_of(line_starts: &[u32], byte: u32) -> u32 {
    line_starts.partition_point(|&start| start <= byte) as u32
}

/// Lexes `source` and pairs `()`, `[]` and `{}`.
#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
pub(crate) fn lex(source: &str, limits: &Limits) -> Result<Lexed, LexError> {
    let mut line_starts = vec![0];
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            line_starts.push(index as u32 + 1);
        }
    }
    let (tokens, comments, mut problems) = scan::scan(source);
    let partner = pair(source, &tokens, &mut problems, limits.max_depth, &line_starts)?;
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
        comments,
        partner,
        diagnostics,
        line_starts,
    })
}

#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
fn pair(
    source: &str,
    tokens: &[Token],
    problems: &mut Vec<(u32, DiagnosticReason)>,
    max_depth: u32,
    line_starts: &[u32],
) -> Result<Vec<u32>, LexError> {
    let mut partner = vec![NONE; tokens.len()];
    let mut stack: Vec<(usize, u8)> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != Kind::Punct {
            continue;
        }
        let text = source
            .get(token.start as usize..token.end as usize)
            .unwrap_or("");
        let Some(code) = bracket_code(text) else {
            continue;
        };
        if code < 3 {
            if u32::try_from(stack.len()).unwrap_or(u32::MAX) >= max_depth {
                return Err(LexError::TooDeep {
                    line: line_of(line_starts, token.start),
                    limit: max_depth,
                });
            }
            stack.push((index, code));
            continue;
        }
        match stack.pop() {
            Some((open, kind)) if kind + 3 == code => {
                partner[open] = index as u32;
                partner[index] = open as u32;
            }
            Some(unmatched) => {
                stack.push(unmatched);
                problems.push((token.start, DiagnosticReason::UnbalancedBracket));
            }
            None => problems.push((token.start, DiagnosticReason::UnbalancedBracket)),
        }
    }
    for (open, _) in stack {
        problems.push((tokens[open].start, DiagnosticReason::UnbalancedBracket));
    }
    Ok(partner)
}

fn bracket_code(text: &str) -> Option<u8> {
    match text {
        "(" => Some(0),
        "[" => Some(1),
        "{" => Some(2),
        ")" => Some(3),
        "]" => Some(4),
        "}" => Some(5),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(source: &str) -> Vec<String> {
        let lexed = lex(source, &Limits::default()).expect("lex");
        lexed
            .tokens
            .iter()
            .map(|token| source[token.start as usize..token.end as usize].to_string())
            .collect()
    }

    #[test]
    fn literals_hide_declaration_shaped_text() {
        let source = "class A { String s = \"class B {}\"; String t = \"\"\"\nclass C {}\n\"\"\"; char c = '\"'; }";
        let lexed = lex(source, &Limits::default()).expect("lex");
        assert_eq!(lexed.diagnostics, []);
        let owned = texts(source);
        assert!(owned.iter().any(|text| text.contains("class B")));
        assert!(owned.iter().any(|text| text.starts_with("\"\"\"")));
        assert!(owned.contains(&"'\"'".to_string()));
        assert!(!owned.iter().any(|text| text == "B" || text == "C"));
        let generics: Vec<_> = owned.into_iter().filter(|text| text == ">").collect();
        let nested = texts("class A<B<C>> {}");
        assert_eq!(nested.iter().filter(|text| text.as_str() == ">").count(), 2);
        assert_eq!(generics, [] as [String; 0]);
    }

    #[test]
    fn partners_lines_and_depth_are_typed() {
        let lexed = lex("class A {\n void m() { }\n}", &Limits::default()).expect("lex");
        assert_eq!(lexed.line(lexed.tokens.last().expect("token").start), 3);
        assert_eq!(lexed.diagnostics, []);
        let open = lex("void m() {", &Limits::default()).expect("lex");
        assert!(open
            .diagnostics
            .iter()
            .any(|d| d.reason == DiagnosticReason::UnbalancedBracket));
        let limits = Limits {
            max_depth: 2,
            ..Limits::default()
        };
        assert!(matches!(
            lex("{{{", &limits),
            Err(LexError::TooDeep { limit: 2, .. })
        ));
        assert!(lex("{{}}", &limits).is_ok());
    }
}
