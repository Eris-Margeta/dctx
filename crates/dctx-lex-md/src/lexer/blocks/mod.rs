// SPDX-License-Identifier: Apache-2.0
//! Block scanner: containers, paragraphs, headings, code and HTML blocks, line by line.
//! Implements the `CommonMark` 0.31 block algorithm (§3.2, §4, §5) reduced to structure.
//! Uses line and marker rules plus `leaves`; the outline step consumes its `Blocks`.
mod leaves;

use super::html::HtmlEnd;
use super::lines::{Cursor, Line, is_blank};
use super::markers::{list_marker, thematic_break};
use crate::{CodeBlock, HeadingStyle, LexError, Limits, Span};
use leaves::interrupts;

/// Heading as written, before the section tree is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawHeading {
    pub level: u8,
    pub style: HeadingStyle,
    pub title: String,
    pub span: Span,
    pub line: u32,
}

/// One paragraph line after container prefixes and indentation, trimmed at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Segment {
    pub start: usize,
    pub end: usize,
    pub line: u32,
}

/// Paragraph lines and the heading written most recently before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Paragraph {
    pub segments: Vec<Segment>,
    pub owner: Option<usize>,
}

/// Block structure of one document in source order.
#[derive(Debug, Default)]
#[allow(clippy::struct_field_names, reason = "code_blocks matches the public document vocabulary and distinguishes code blocks from paragraphs and headings.")]
pub(crate) struct Blocks {
    pub headings: Vec<RawHeading>,
    pub paragraphs: Vec<Paragraph>,
    pub code_blocks: Vec<CodeBlock>,
}

#[derive(Debug, Clone, Copy)]
enum Container {
    Quote,
    /// List item whose content starts at this column.
    Item(usize),
}

#[derive(Debug, Default)]
enum Leaf {
    #[default]
    None,
    Paragraph(Vec<Segment>),
    Fence {
        fence: u8,
        len: usize,
        info: String,
        span: Span,
        lines: [u32; 2],
    },
    Indented {
        span: Span,
        lines: [u32; 2],
    },
    Html(HtmlEnd),
}

pub(crate) struct Scanner<'a> {
    source: &'a str,
    limits: &'a Limits,
    stack: Vec<Container>,
    leaf: Leaf,
    out: Blocks,
}

impl<'a> Scanner<'a> {
    pub fn new(source: &'a str, limits: &'a Limits) -> Self {
        Self {
            source,
            limits,
            stack: Vec::new(),
            leaf: Leaf::None,
            out: Blocks::default(),
        }
    }

    pub fn finish(mut self) -> Blocks {
        self.finish_leaf();
        self.out
    }

    pub fn line(&mut self, line: &Line<'_>) -> Result<(), LexError> {
        let text = line.text;
        let (matched, mut cur) = self.match_containers(text);
        if matched < self.stack.len() {
            let lazy = matches!(self.leaf, Leaf::Paragraph(_))
                && !is_blank(cur.rest(text))
                && !interrupts(text, cur);
            if lazy {
                self.push_segment(line, cur.indent(text).1);
                return Ok(());
            }
            self.finish_leaf();
            self.stack.truncate(matched);
        }
        if self.continue_leaf(line, cur) {
            return Ok(());
        }
        loop {
            let (indent, at) = cur.indent(text);
            let rest = at.rest(text);
            if indent >= 4 {
                break;
            }
            let container = if rest.starts_with('>') {
                cur = after_quote_marker(text, at);
                Container::Quote
            } else if let Some(marker) = list_marker(rest)
                .filter(|_| !thematic_break(rest))
                .filter(|m| m.interrupts || !matches!(self.leaf, Leaf::Paragraph(_)))
            {
                let after = at.skip(marker.width);
                let (spaces, content) = after.indent(text);
                let (column, next) = if marker.empty {
                    (after.col + 1, after)
                } else if spaces >= 5 {
                    (after.col + 1, after.advance_to(text, after.col + 1))
                } else {
                    (content.col, content)
                };
                cur = next;
                Container::Item(column)
            } else {
                break;
            };
            self.finish_leaf();
            if self.stack.len() >= self.limits.max_depth {
                return Err(LexError::TooDeep {
                    line: line.number,
                    max: self.limits.max_depth,
                });
            }
            self.stack.push(container);
        }
        self.open_leaf(line, cur);
        Ok(())
    }

    fn match_containers(&self, text: &str) -> (usize, Cursor) {
        let mut cur = Cursor::default();
        let mut matched = 0;
        for container in &self.stack {
            match *container {
                Container::Quote => {
                    let (indent, at) = cur.indent(text);
                    if indent > 3 || !at.rest(text).starts_with('>') {
                        break;
                    }
                    cur = after_quote_marker(text, at);
                }
                Container::Item(column) => {
                    if !is_blank(cur.rest(text)) {
                        let (_, at) = cur.indent(text);
                        if at.col < column {
                            break;
                        }
                        cur = cur.advance_to(text, column);
                    }
                }
            }
            matched += 1;
        }
        (matched, cur)
    }
}

/// Cursor after a `>` at `at` and the one optional space that follows it.
fn after_quote_marker(text: &str, at: Cursor) -> Cursor {
    let after = at.skip(1);
    if after.rest(text).starts_with(' ') {
        after.skip(1)
    } else {
        after
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lines::Lines;

    fn titles(source: &str) -> Vec<(u8, String)> {
        let limits = Limits::default();
        let mut scanner = Scanner::new(source, &limits);
        for line in Lines::new(source, 0, 1) {
            scanner.line(&line).unwrap();
        }
        let blocks = scanner.finish();
        blocks
            .headings
            .into_iter()
            .map(|h| (h.level, h.title))
            .collect()
    }

    #[test]
    fn containers_hold_headings_and_fences() {
        assert_eq!(
            titles("> # Quoted\n- ## Listed"),
            [(1, "Quoted".into()), (2, "Listed".into())]
        );
        assert_eq!(titles("- ```\n  # code\n  ```\n"), []);
        assert_eq!(titles("> ```\n> # code\n"), []);
        assert_eq!(titles("- ```\n# out\n"), [(1, "out".into())]);
        assert_eq!(titles("> > # Deep"), [(1, "Deep".into())]);
        assert_eq!(titles("1. a\n\n   # In item"), [(1, "In item".into())]);
    }

    #[test]
    fn nesting_beyond_the_limit_is_refused() {
        let limits = Limits {
            max_depth: 2,
            ..Limits::default()
        };
        let mut scanner = Scanner::new("> > > deep", &limits);
        let line = Lines::new("> > > deep", 0, 1).next().unwrap();
        assert_eq!(
            scanner.line(&line),
            Err(LexError::TooDeep { line: 1, max: 2 })
        );
        let mut scanner = Scanner::new("> > ok", &limits);
        let line = Lines::new("> > ok", 0, 1).next().unwrap();
        assert!(scanner.line(&line).is_ok());
    }
}
