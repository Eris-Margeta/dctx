// SPDX-License-Identifier: Apache-2.0
//! Leaf blocks: paragraphs, ATX and setext headings, fenced, indented and HTML blocks.
//! Implements `CommonMark` 0.31 §4.1–4.6 and §4.8 for the block scanner.
//! Extends `Scanner` with leaf rules; uses line, marker and HTML helpers.
use super::{Leaf, Paragraph, RawHeading, Scanner, Segment};
use crate::lexer::html::{self, HtmlEnd};
use crate::lexer::lines::{Cursor, Line, is_blank};
use crate::lexer::markers::{atx, fence_close, fence_open, list_marker, setext, thematic_break};
use crate::{CodeBlock, CodeBlockKind, HeadingStyle, Span};

impl Scanner<'_> {
    /// Feeds the line to an open code or HTML block; true when it was consumed.
    pub(super) fn continue_leaf(&mut self, line: &Line<'_>, cur: Cursor) -> bool {
        let text = line.text;
        let (indent, at) = cur.indent(text);
        match &mut self.leaf {
            Leaf::Fence {
                fence,
                len,
                span,
                lines,
                ..
            } => {
                span.end = line.end;
                lines[1] = line.number;
                if indent <= 3 && fence_close(at.rest(text), *fence, *len) {
                    self.finish_leaf();
                }
                true
            }
            Leaf::Html(end) => {
                if *end == HtmlEnd::Blank {
                    if is_blank(cur.rest(text)) {
                        self.leaf = Leaf::None;
                    }
                } else if html::ends(text, *end) {
                    self.leaf = Leaf::None;
                }
                true
            }
            Leaf::Indented { span, lines } => {
                if is_blank(cur.rest(text)) {
                    return true;
                }
                if indent >= 4 {
                    span.end = line.end;
                    lines[1] = line.number;
                    return true;
                }
                self.finish_leaf();
                false
            }
            Leaf::None | Leaf::Paragraph(_) => false,
        }
    }

    pub(super) fn open_leaf(&mut self, line: &Line<'_>, cur: Cursor) {
        let text = line.text;
        let in_paragraph = matches!(self.leaf, Leaf::Paragraph(_));
        if is_blank(cur.rest(text)) {
            self.finish_leaf();
            return;
        }
        let (indent, at) = cur.indent(text);
        let rest = at.rest(text);
        let start = line.start + at.pos;
        if indent >= 4 {
            if in_paragraph {
                self.push_segment(line, at);
            } else {
                let content = cur.advance_to(text, cur.col + 4);
                self.leaf = Leaf::Indented {
                    span: Span {
                        start: line.start + content.pos,
                        end: line.end,
                    },
                    lines: [line.number; 2],
                };
            }
            return;
        }
        if let Some((level, title)) = atx(rest) {
            self.finish_leaf();
            self.out.headings.push(RawHeading {
                level,
                style: HeadingStyle::Atx,
                title: title.into(),
                span: Span {
                    start,
                    end: line.end,
                },
                line: line.number,
            });
        } else if let Some((fence, len, info)) = fence_open(rest) {
            self.finish_leaf();
            self.leaf = Leaf::Fence {
                fence,
                len,
                info: info.into(),
                span: Span {
                    start,
                    end: line.end,
                },
                lines: [line.number; 2],
            };
        } else if let (Some(level), Leaf::Paragraph(segments)) = (setext(rest), &self.leaf) {
            self.setext(level, segments.clone(), line);
        } else if thematic_break(rest) {
            self.finish_leaf();
        } else if let Some(end) = html::start(rest, in_paragraph) {
            self.finish_leaf();
            if !html::ends_on_start(rest, end) {
                self.leaf = Leaf::Html(end);
            }
        } else {
            self.push_segment(line, at);
        }
    }

    /// Turns the open paragraph into a setext heading underlined by `line`.
    fn setext(&mut self, level: u8, segments: Vec<Segment>, line: &Line<'_>) {
        let Some(first) = segments.first().copied() else {
            return;
        };
        let title = segments
            .into_iter()
            .map(|s| self.source[s.start..s.end].trim_start_matches([' ', '\t']))
            .collect::<Vec<_>>()
            .join("\n");
        self.out.headings.push(RawHeading {
            level,
            style: HeadingStyle::Setext,
            title,
            span: Span {
                start: first.start,
                end: line.end,
            },
            line: first.line,
        });
        self.leaf = Leaf::None;
    }

    pub(super) fn push_segment(&mut self, line: &Line<'_>, at: Cursor) {
        let content = at.rest(line.text).trim_end_matches([' ', '\t']);
        let start = line.start + at.pos;
        let segment = Segment {
            start,
            end: start + content.len(),
            line: line.number,
        };
        match &mut self.leaf {
            Leaf::Paragraph(segments) => segments.push(segment),
            _ => self.leaf = Leaf::Paragraph(vec![segment]),
        }
    }

    pub(super) fn finish_leaf(&mut self) {
        let owner = self.out.headings.len().checked_sub(1);
        match std::mem::take(&mut self.leaf) {
            Leaf::Paragraph(segments) => self.out.paragraphs.push(Paragraph { segments, owner }),
            Leaf::Fence {
                info, span, lines, ..
            } => self.out.code_blocks.push(CodeBlock {
                kind: CodeBlockKind::Fenced { info },
                span,
                lines,
            }),
            Leaf::Indented { span, lines } => self.out.code_blocks.push(CodeBlock {
                kind: CodeBlockKind::Indented,
                span,
                lines,
            }),
            Leaf::Html(_) | Leaf::None => {}
        }
    }
}

/// Whether an unmatched line starts a block, so it cannot continue a paragraph lazily.
pub(super) fn interrupts(text: &str, cur: Cursor) -> bool {
    let (indent, at) = cur.indent(text);
    let rest = at.rest(text);
    indent < 4
        && (rest.starts_with('>')
            || atx(rest).is_some()
            || fence_open(rest).is_some()
            || thematic_break(rest)
            || list_marker(rest).is_some_and(|m| m.interrupts)
            || html::start(rest, true).is_some())
}

#[cfg(test)]
mod tests {
    use super::super::{Blocks, Scanner, Segment};
    use crate::lexer::lines::Lines;
    use crate::{CodeBlockKind, Limits};

    fn scan(source: &str) -> Blocks {
        let limits = Limits::default();
        let mut scanner = Scanner::new(source, &limits);
        for line in Lines::new(source, 0, 1) {
            scanner.line(&line).unwrap();
        }
        scanner.finish()
    }

    fn titles(source: &str) -> Vec<(u8, String)> {
        let blocks = scan(source);
        blocks
            .headings
            .into_iter()
            .map(|h| (h.level, h.title))
            .collect()
    }

    #[test]
    fn code_blocks_hide_headings() {
        let blocks = scan("# A\n```md\n# no\n```\n    # no\n\n~~~\n# no\n");
        assert_eq!(blocks.headings.len(), 1);
        assert_eq!(blocks.code_blocks.len(), 3);
        assert_eq!(blocks.code_blocks[0].lines, [2, 4]);
        assert_eq!(blocks.code_blocks[1].kind, CodeBlockKind::Indented);
        assert_eq!(blocks.code_blocks[2].lines, [7, 8]);
    }

    #[test]
    fn setext_needs_an_unlazy_paragraph() {
        assert_eq!(titles("Foo\nbar\n---"), [(2, "Foo\nbar".into())]);
        assert_eq!(titles("> foo\n---"), []);
        assert_eq!(titles("> foo\n==="), []);
        assert_eq!(titles("- foo\n---"), []);
        assert_eq!(titles("---\nFoo"), []);
        assert_eq!(titles("    Foo\n    ---"), []);
    }

    #[test]
    fn html_blocks_hide_markdown_until_they_end() {
        assert_eq!(titles("<div>\n# no\n</div>"), []);
        assert_eq!(titles("<div>\n\n# yes"), [(1, "yes".into())]);
        assert_eq!(titles("<!--\n# no\n-->"), []);
        assert_eq!(titles("<!-- x -->\n# yes"), [(1, "yes".into())]);
    }

    #[test]
    fn paragraphs_record_their_owner_and_segments() {
        let blocks = scan("intro\n# A\ntext  \n  more\n");
        assert_eq!(blocks.paragraphs[0].owner, None);
        assert_eq!(blocks.paragraphs[1].owner, Some(0));
        let segment = |start, end, line| Segment { start, end, line };
        assert_eq!(
            blocks.paragraphs[1].segments,
            [segment(10, 14, 3), segment(19, 23, 4)]
        );
    }
}
