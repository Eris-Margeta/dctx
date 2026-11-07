// SPDX-License-Identifier: Apache-2.0
//! Nesting-cost bound checked on the token tree before syn's recursive descent runs.
//! Implements PARSERS-v1 "no unbounded recursion". Cost of a token = tokens since the last
//! point where syn returns to a loop (`;`, `=>`, a `,` outside `<...>`/`|...|`, a finished
//! brace block) plus the cost of each enclosing group. It bounds parse, visit and drop depth.
//! Iterative over proc-macro2 groups; used by the syn adapter.
use crate::Refusal;
use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree, token_stream::IntoIter};
use std::iter::Peekable;

struct Frame {
    tokens: Peekable<IntoIter>,
    base: u32,
    open: u32,
    angles: u32,
    pipes_open: bool,
    joint: Option<char>,
}

impl Frame {
    fn new(stream: TokenStream, base: u32) -> Self {
        Self {
            tokens: stream.into_iter().peekable(),
            base,
            open: 0,
            angles: 0,
            pipes_open: false,
            joint: None,
        }
    }

    fn reset(&mut self) {
        self.open = 0;
        self.angles = 0;
        self.pipes_open = false;
    }

    fn next_is(&mut self, c: char) -> bool {
        matches!(self.tokens.peek(), Some(TokenTree::Punct(p)) if p.as_char() == c)
    }

    /// A brace group ends a block unless an `else`, a cast or an operator continues it.
    fn block_ends(&mut self) -> bool {
        match self.tokens.peek() {
            None => true,
            Some(TokenTree::Ident(ident)) => ident != "else" && ident != "as",
            Some(TokenTree::Punct(p)) => matches!(p.as_char(), '#' | ';'),
            _ => false,
        }
    }
}

pub(super) fn check(stream: &TokenStream, limit: u32) -> Result<(), Refusal> {
    let mut stack = vec![Frame::new(stream.clone(), 0)];
    while let Some(frame) = stack.last_mut() {
        let Some(tree) = frame.tokens.next() else {
            stack.pop();
            continue;
        };
        frame.open = frame.open.saturating_add(1);
        let cost = frame.base.saturating_add(frame.open);
        if cost > limit {
            let line = u32::try_from(tree.span().start().line).unwrap_or(u32::MAX);
            return Err(Refusal::TooDeep { line, limit });
        }
        let joint = frame.joint.take();
        match &tree {
            TokenTree::Group(group) => {
                let child = Frame::new(group.stream(), cost);
                if group.delimiter() == Delimiter::Brace && frame.block_ends() {
                    frame.reset();
                }
                stack.push(child);
            }
            TokenTree::Punct(punct) => {
                let c = punct.as_char();
                let glued = punct.spacing() == Spacing::Joint;
                match c {
                    ';' => frame.reset(),
                    ',' if frame.angles == 0 && !frame.pipes_open => frame.open = 0,
                    '>' if joint == Some('=') => frame.reset(),
                    '>' if joint == Some('-') || (glued && frame.next_is('=')) => {}
                    '>' => frame.angles = frame.angles.saturating_sub(1),
                    '<' if !(glued && frame.next_is('=')) => {
                        frame.angles = frame.angles.saturating_add(1);
                    }
                    '|' => frame.pipes_open = !frame.pipes_open,
                    _ => {}
                }
                if glued {
                    frame.joint = Some(c);
                }
            }
            TokenTree::Ident(_) | TokenTree::Literal(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn cost_ok(source: &str, limit: u32) -> bool {
        let stream = TokenStream::from_str(source).unwrap_or_default();
        check(&stream, limit).is_ok()
    }

    #[test]
    fn statements_items_and_arms_reset_the_cost() {
        let many_items = "fn a() {} ".repeat(200);
        assert!(cost_ok(&many_items, 16));
        let statements = format!("fn a() {{ {} }}", "x = 1; ".repeat(200));
        assert!(cost_ok(&statements, 16));
        let arms = format!("match x {{ {} }}", "A => 1, ".repeat(200));
        assert!(cost_ok(&arms, 16));
        let list = format!("[{}]", "1, ".repeat(200));
        assert!(cost_ok(&list, 16));
    }

    #[test]
    fn chains_and_nesting_accumulate() {
        assert!(!cost_ok(&"-".repeat(40), 16));
        assert!(!cost_ok(&"a.b()".repeat(20), 16));
        assert!(!cost_ok(
            &format!("{}x{}", "(".repeat(20), ")".repeat(20)),
            16
        ));
        assert!(!cost_ok(&"|a,| ".repeat(20), 16));
        assert!(!cost_ok(&format!("Vec<{}T>", "A, Vec<".repeat(10)), 16));
        assert!(!cost_ok(&"if a {} else ".repeat(10), 16));
        assert!(cost_ok("fn a() -> u8 { 3 }", 16));
    }

    #[test]
    fn refusal_reports_the_line() {
        let source = format!("fn a() {{}}\n{}", "-".repeat(40));
        let stream = TokenStream::from_str(&source).unwrap_or_default();
        assert_eq!(
            check(&stream, 16),
            Err(Refusal::TooDeep { line: 2, limit: 16 })
        );
    }
}
