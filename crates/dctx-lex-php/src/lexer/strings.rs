//! Opaque PHP quoted strings, interpolation, heredoc and nowdoc boundaries.
//! Implements decision 077: literal contents never become declarations or calls.
//! Used by the byte scanner; nesting refusals use the shared limits.
use super::Kind;
use crate::domain::{DiagnosticReason, LexError, Limits};

pub(super) struct Literal {
    pub end: usize,
    pub kind: Kind,
    pub problem: Option<DiagnosticReason>,
}

pub(super) fn quoted(
    src: &str,
    start: usize,
    limits: &Limits,
    line: u32,
) -> Result<Literal, LexError> {
    let bytes = src.as_bytes();
    let mut frames = vec![(bytes[start], false)];
    let mut i = start + 1;
    let mut dynamic = bytes[start] == b'`';
    while i < bytes.len() {
        let Some(&(delimiter, expression)) = frames.last() else {
            break;
        };
        let b = bytes[i];
        if b == b'\\' && !expression {
            i = (i + 2).min(bytes.len());
            continue;
        }
        if expression {
            match b {
                b'\'' | b'"' | b'`' => frames.push((b, false)),
                b'{' => frames.push((b'}', true)),
                b'}' => {
                    frames.pop();
                }
                _ => {}
            }
        } else if b == delimiter {
            frames.pop();
            if frames.is_empty() {
                return Ok(Literal {
                    end: i + 1,
                    kind: if dynamic {
                        Kind::DynamicString
                    } else {
                        Kind::String
                    },
                    problem: None,
                });
            }
        } else if delimiter != b'\'' {
            if b == b'$' {
                dynamic = true;
            }
            if (b == b'{' && bytes.get(i + 1) == Some(&b'$'))
                || (b == b'$' && bytes.get(i + 1) == Some(&b'{'))
            {
                dynamic = true;
                frames.push((b'}', true));
                i += 1;
            }
        }
        if frames.len().saturating_sub(1) > limits.max_depth as usize {
            return Err(LexError::TooDeep {
                line,
                limit: limits.max_depth,
            });
        }
        i += 1;
    }
    Ok(Literal {
        end: bytes.len(),
        kind: Kind::DynamicString,
        problem: Some(DiagnosticReason::UnterminatedString),
    })
}

pub(super) fn heredoc(src: &str, start: usize) -> Literal {
    let bytes = src.as_bytes();
    let mut i = start + 3;
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    let quote = bytes.get(i).copied().filter(|b| matches!(b, b'\'' | b'"'));
    if quote.is_some() {
        i += 1;
    }
    let label_start = i;
    while bytes
        .get(i)
        .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        i += 1;
    }
    let label = &src[label_start..i];
    if label.is_empty() || bytes[label_start].is_ascii_digit() {
        return broken(bytes.len());
    }
    if let Some(q) = quote {
        if bytes.get(i) != Some(&q) {
            return broken(bytes.len());
        }
        i += 1;
    }
    while matches!(bytes.get(i), Some(b' ' | b'\t' | b'\r')) {
        i += 1;
    }
    if bytes.get(i) != Some(&b'\n') {
        return broken(bytes.len());
    }
    i += 1;
    while i < bytes.len() {
        let line_end = src[i..].find('\n').map_or(bytes.len(), |n| i + n);
        let mut first = i;
        while first < line_end && matches!(bytes[first], b' ' | b'\t') {
            first += 1;
        }
        if src[first..].starts_with(label) {
            let end = first + label.len();
            if !bytes
                .get(end)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b >= 128)
            {
                return Literal {
                    end,
                    kind: Kind::DynamicString,
                    problem: None,
                };
            }
        }
        i = line_end.saturating_add(1);
    }
    broken(bytes.len())
}

fn broken(end: usize) -> Literal {
    Literal {
        end,
        kind: Kind::DynamicString,
        problem: Some(DiagnosticReason::UnterminatedHeredoc),
    }
}
