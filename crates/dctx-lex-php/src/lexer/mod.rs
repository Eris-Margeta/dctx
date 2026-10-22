//! PHP token vocabulary, source coordinates and bounded delimiter pairing.
//! Implements PARSERS-v1 lexical limits and partial diagnostics (decision 077).
//! The scanner keeps template HTML and literals opaque for the extractor.
mod scan;
mod strings;

use crate::domain::{Diagnostic, DiagnosticReason, LexError, Limits};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Ident,
    Variable,
    Number,
    String,
    DynamicString,
    Punct,
    Boundary,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
}

pub(crate) struct Lexed {
    pub tokens: Vec<Token>,
    pub partner: Vec<Option<usize>>,
    pub comments: Vec<(usize, usize)>,
    pub diagnostics: Vec<Diagnostic>,
    pub lines: Vec<usize>,
}

impl Lexed {
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets and line counts fit u32 because PhpLexer::extract enforces the 8 MiB input ceiling.")]
    pub fn line(&self, byte: usize) -> u32 {
        self.lines.partition_point(|&start| start <= byte) as u32
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets and line counts fit u32 because PhpLexer::extract enforces the 8 MiB input ceiling.")]
    pub fn problem(&mut self, byte: usize, reason: DiagnosticReason) {
        if self.diagnostics.len() < 64 {
            self.diagnostics.push(Diagnostic {
                byte: byte as u32,
                line: self.line(byte),
                reason,
            });
        }
    }
}

pub(crate) fn lex(source: &str, limits: &Limits) -> Result<Lexed, LexError> {
    let mut result = Lexed {
        tokens: Vec::new(),
        partner: Vec::new(),
        comments: Vec::new(),
        diagnostics: Vec::new(),
        lines: vec![0],
    };
    result.lines.extend(
        source
            .bytes()
            .enumerate()
            .filter(|(_, b)| *b == b'\n')
            .map(|(i, _)| i + 1),
    );
    scan::scan(source, &mut result, limits)?;
    result.partner.resize(result.tokens.len(), None);
    let mut stack: Vec<usize> = Vec::new();
    for index in 0..result.tokens.len() {
        let token = result.tokens[index];
        if token.kind != Kind::Punct {
            continue;
        }
        let text = &source[token.start..token.end];
        match text {
            "(" | "[" | "{" | "#[" => {
                if stack.len() >= limits.max_depth as usize {
                    return Err(LexError::TooDeep {
                        line: result.line(token.start),
                        limit: limits.max_depth,
                    });
                }
                stack.push(index);
            }
            ")" | "]" | "}" => {
                let wanted = match text {
                    ")" => "(",
                    "]" => "[",
                    _ => "{",
                };
                let found = stack.iter().rposition(|&open| {
                    let t = result.tokens[open];
                    let s = &source[t.start..t.end];
                    s == wanted || (wanted == "[" && s == "#[")
                });
                if let Some(position) = found {
                    while stack.len() > position + 1 {
                        if let Some(orphan) = stack.pop() {
                            result.problem(
                                result.tokens[orphan].start,
                                DiagnosticReason::UnbalancedBracket,
                            );
                        }
                    }
                    if let Some(open) = stack.pop() {
                        result.partner[open] = Some(index);
                        result.partner[index] = Some(open);
                    }
                } else {
                    result.problem(token.start, DiagnosticReason::UnbalancedBracket);
                }
            }
            _ => {}
        }
    }
    for open in stack {
        result.problem(
            result.tokens[open].start,
            DiagnosticReason::UnbalancedBracket,
        );
    }
    result.diagnostics.sort_by_key(|d| d.byte);
    Ok(result)
}
