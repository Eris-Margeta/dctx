//! Swift token vocabulary and delimiter pairing for PARSERS-v1.
//! Depends on the bounded scanner and domain diagnostics.
//! Used by the structural declaration and reference walkers.
mod scan;
mod strings;

use crate::domain::{Diagnostic, DiagnosticReason, LexError, Limits};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Ident,
    Number,
    Str,
    Punct,
    Directive,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    pub kind: Kind,
    pub start: u32,
    pub end: u32,
}

pub(crate) struct Lexed {
    pub tokens: Vec<Token>,
    pub comments: Vec<(u32, u32)>,
    pub partner: Vec<Option<usize>>,
    pub diagnostics: Vec<Diagnostic>,
    pub line_starts: Vec<u32>,
}

impl Lexed {
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and angle-bracket runs fit the target integer because SwiftLexer::extract enforces the 8 MiB input ceiling.")]
    pub fn line(&self, byte: u32) -> u32 {
        self.line_starts.partition_point(|&start| start <= byte) as u32
    }
}

#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and angle-bracket runs fit the target integer because SwiftLexer::extract enforces the 8 MiB input ceiling.")]
pub(crate) fn lex(source: &str, limits: &Limits) -> Result<Lexed, LexError> {
    let mut scanner = scan::Scanner::new(source, limits);
    scanner.region(false, 0)?;
    let mut output = Lexed {
        tokens: scanner.tokens,
        comments: scanner.comments,
        partner: Vec::new(),
        diagnostics: Vec::new(),
        line_starts: vec![0],
    };
    output.line_starts.extend(
        source
            .bytes()
            .enumerate()
            .filter(|(_, b)| *b == b'\n')
            .map(|(i, _)| i as u32 + 1),
    );
    output.partner.resize(output.tokens.len(), None);
    let mut stack = Vec::new();
    let mut conditions = Vec::new();
    for (i, token) in output.tokens.iter().enumerate() {
        let text = &source[token.start as usize..token.end as usize];
        if token.kind == Kind::Directive {
            match text {
                "#if" => {
                    conditions.push((token.start, false));
                    if conditions.len() > limits.max_depth as usize {
                        return Err(LexError::TooDeep {
                            line: output.line(token.start),
                            limit: limits.max_depth,
                        });
                    }
                }
                "#elseif" | "#else" => {
                    if let Some((_, saw_else)) = conditions.last_mut() {
                        if *saw_else {
                            record(&mut scanner.problems, token.start);
                        }
                        *saw_else = text == "#else";
                    } else {
                        record(&mut scanner.problems, token.start);
                    }
                }
                "#endif" if conditions.pop().is_none() => {
                    record(&mut scanner.problems, token.start);
                }
                _ => {}
            }
        }
        if token.kind != Kind::Punct {
            continue;
        }
        if matches!(text, "(" | "[" | "{") {
            stack.push((i, text));
            if stack.len() > limits.max_depth as usize {
                return Err(LexError::TooDeep {
                    line: output.line(token.start),
                    limit: limits.max_depth,
                });
            }
        } else if matches!(text, ")" | "]" | "}") {
            if let Some(&(open, spelling)) = stack.last()
                && matches!((spelling, text), ("(", ")") | ("[", "]") | ("{", "}"))
            {
                stack.pop();
                output.partner[i] = Some(open);
                output.partner[open] = Some(i);
            } else {
                record(&mut scanner.problems, token.start);
            }
        }
    }
    for (i, _) in stack {
        record(&mut scanner.problems, output.tokens[i].start);
    }
    for (byte, _) in conditions {
        record(&mut scanner.problems, byte);
    }
    scanner.problems.sort_by_key(|p| p.0);
    output.diagnostics = scanner
        .problems
        .into_iter()
        .take(crate::LexedFile::MAX_DIAGNOSTICS)
        .map(|(byte, reason)| Diagnostic {
            byte,
            line: output.line(byte),
            reason,
        })
        .collect();
    Ok(output)
}

fn record(problems: &mut Vec<(u32, DiagnosticReason)>, byte: u32) {
    if problems.len() < crate::LexedFile::MAX_DIAGNOSTICS {
        problems.push((byte, DiagnosticReason::UnbalancedBracket));
    }
}
