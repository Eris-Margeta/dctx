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
    pub fn line(&self, byte: usize) -> u32 {
        self.lines.partition_point(|&start| start <= byte) as u32
    }

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
    // `?>` inside a block comment ends the PHP region. The byte scanner keeps
    // going, so retokenize the suffix in HTML mode from that closer.
    repair_comment_closers(source, &mut result, limits)?;
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

fn repair_comment_closers(
    source: &str,
    result: &mut Lexed,
    limits: &Limits,
) -> Result<(), LexError> {
    while let Some((comment_start, at)) = block_comment_closer(source, &result.comments) {
        result.tokens.retain(|token| token.end <= at);
        result.diagnostics.retain(|diagnostic| {
            let byte = diagnostic.byte as usize;
            byte < at
                && !(diagnostic.reason == DiagnosticReason::UnterminatedComment
                    && byte == comment_start)
        });
        for comment in &mut result.comments {
            if comment.0 == comment_start && comment.1 > at {
                comment.1 = at;
            }
        }
        result.comments.retain(|(start, end)| *end <= at && *start < at);
        let base = result.line(at);
        let suffix = &source[at..];
        let mut extra = Lexed {
            tokens: Vec::new(),
            partner: Vec::new(),
            comments: Vec::new(),
            diagnostics: Vec::new(),
            lines: vec![0],
        };
        extra.lines.extend(
            suffix
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(index, _)| index + 1),
        );
        match scan::scan(suffix, &mut extra, limits) {
            Ok(()) => {}
            Err(LexError::TooDeep { line, limit }) => {
                return Err(LexError::TooDeep {
                    line: base + line - 1,
                    limit,
                });
            }
            Err(error) => return Err(error),
        }
        for token in extra.tokens {
            result.tokens.push(Token {
                kind: token.kind,
                start: token.start + at,
                end: token.end + at,
            });
        }
        for (start, end) in extra.comments {
            result.comments.push((start + at, end + at));
        }
        for diagnostic in extra.diagnostics {
            result.problem(diagnostic.byte as usize + at, diagnostic.reason);
        }
    }
    Ok(())
}

fn block_comment_closer(source: &str, comments: &[(usize, usize)]) -> Option<(usize, usize)> {
    let mut found: Option<(usize, usize)> = None;
    for &(start, end) in comments {
        let Some(body) = source.get(start..end) else {
            continue;
        };
        if !body.starts_with("/*") {
            continue;
        }
        let Some(relative) = body[2..].find("?>") else {
            continue;
        };
        let at = start + 2 + relative;
        if found.is_none_or(|(_, previous)| at < previous) {
            found = Some((start, at));
        }
    }
    found
}
