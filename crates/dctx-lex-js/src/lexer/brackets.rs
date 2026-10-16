//! Bracket partners over the token stream, with conservative recovery.
//! Implements the PARSERS-v1 bounded-depth rule and reports unbalanced brackets.
//! Used by `lexer::lex`; the walker reads partners to skip and scope groups.
use super::{Kind, NONE, Problems, Token};
use crate::domain::DiagnosticReason;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Paren,
    Square,
    Curly,
    Jsx,
    Template,
}

fn opens(source: &str, token: &Token) -> Option<Shape> {
    match token.kind {
        Kind::JsxOpen => Some(Shape::Jsx),
        Kind::TemplateHead | Kind::TemplateMiddle => Some(Shape::Template),
        Kind::Punct => match &source[token.start as usize..token.end as usize] {
            "(" => Some(Shape::Paren),
            "[" => Some(Shape::Square),
            "{" => Some(Shape::Curly),
            _ => None,
        },
        _ => None,
    }
}

fn closes(source: &str, token: &Token) -> Option<Shape> {
    match token.kind {
        Kind::JsxClose => Some(Shape::Jsx),
        Kind::TemplateMiddle | Kind::TemplateTail => Some(Shape::Template),
        Kind::Punct => match &source[token.start as usize..token.end as usize] {
            ")" => Some(Shape::Paren),
            "]" => Some(Shape::Square),
            "}" => Some(Shape::Curly),
            _ => None,
        },
        _ => None,
    }
}

/// Returns partners, or the byte where nesting exceeded `max_depth`.
pub(super) fn pair(
    source: &str,
    tokens: &[Token],
    problems: &mut Problems,
    max_depth: u32,
) -> Result<Vec<u32>, u32> {
    let mut partner = vec![NONE; tokens.len()];
    let mut stack: Vec<(usize, Shape)> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if let Some(shape) = closes(source, token) {
            match stack.iter().rposition(|open| open.1 == shape) {
                Some(depth) => {
                    for (open, _) in stack.drain(depth + 1..) {
                        problems.push((tokens[open].start, DiagnosticReason::UnbalancedBracket));
                    }
                    if let Some((open, _)) = stack.pop() {
                        partner[open] = index as u32;
                        partner[index] = open as u32;
                    }
                }
                None => problems.push((token.start, DiagnosticReason::UnbalancedBracket)),
            }
        }
        if let Some(shape) = opens(source, token) {
            if stack.len() >= max_depth as usize {
                return Err(token.start);
            }
            stack.push((index, shape));
        }
    }
    for (open, _) in stack {
        problems.push((tokens[open].start, DiagnosticReason::UnbalancedBracket));
    }
    Ok(partner)
}

#[cfg(test)]
mod tests {
    use super::super::lex;
    use super::*;
    use crate::domain::{Dialect, Limits};

    #[test]
    fn mismatched_closers_recover_to_the_nearest_matching_opener() {
        let lexed = lex("f({ a: [1 }); g()", Dialect::JavaScript, &Limits::default()).unwrap();
        assert_eq!(lexed.partner[2], 7);
        assert_eq!(lexed.partner[5], NONE);
        assert_eq!(lexed.partner[1], 8);
        assert_eq!(lexed.diagnostics.len(), 1);
        assert_eq!(lexed.partner[11], 12);
    }

    #[test]
    fn template_middles_both_close_and_open() {
        let lexed = lex("`a${x}b${y}c`", Dialect::JavaScript, &Limits::default()).unwrap();
        assert_eq!(lexed.partner[0], 2);
        assert_eq!(lexed.partner[2], 4);
    }
}
