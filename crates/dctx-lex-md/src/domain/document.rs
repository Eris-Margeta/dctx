// SPDX-License-Identifier: Apache-2.0
//! Extracted Markdown facts: headings with their tree, raw references and code blocks.
//! Mirrors the doc-node and raw-reference shape of dctx-map-parse (SPEC-MAP-MODE 5.1–5.4).
//! Pure data; byte spans index the original UTF-8 source, lines are one-based inclusive.

/// Half-open byte range `start..end` into the source, always on char boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// First byte.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
}

impl Span {
    /// The spanned source text, or `""` for an invalid range.
    ///
    /// Returns `""` when either endpoint is outside the source, splits a UTF-8
    /// character, or when `start > end`. No allocation or validation error is returned.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        source.get(self.start..self.end).unwrap_or_default()
    }
}

/// How a heading was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadingStyle {
    /// `#` to `######` prefix.
    Atx,
    /// Paragraph underlined with `=` or `-`.
    Setext,
}

/// One heading and the section it opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// Level 1–6 (setext headings are 1 or 2).
    pub level: u8,
    /// ATX or setext.
    pub style: HeadingStyle,
    /// Heading text without markers, closing sequence or edge whitespace.
    pub title: String,
    /// Slug of the title; the node name in a doc stable id.
    pub name: String,
    /// One-based ordinal when several headings share `name`, as in stable ids.
    pub ordinal: Option<u32>,
    /// Index of the enclosing heading; `None` means the file module.
    pub parent: Option<usize>,
    /// The heading block itself, through the end of its last line.
    pub span: Span,
    /// From the heading start to the next heading of the same or a higher level.
    pub section: Span,
    /// Heading start line and last section line, computed like dctx-map-parse.
    pub lines: [u32; 2],
    /// First sentence of the first paragraph before the next heading, else the title.
    pub summary: String,
}

impl Heading {
    /// Section range after the heading block; its fingerprint identifies the section body.
    ///
    /// Uses `span.end..section.end`. Extracted headings always produce a valid
    /// range; [`Span::text`] safely handles inconsistent caller-constructed headings.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn body(&self) -> Span {
        Span {
            start: self.span.end,
            end: self.section.end,
        }
    }
}

/// What a raw reference was written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    /// Inline code span, a possible symbol name.
    CodeSpan,
    /// Link or image destination that looks like a repository path.
    PathLink,
    /// Destination with a URL scheme, such as `https:` or `mailto:`.
    ExternalLink,
    /// Destination that is only a `#fragment` of this document.
    AnchorLink,
}

/// An unresolved reference written in a paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// Heading whose section holds the reference; `None` before the first heading,
    /// which dctx-map-parse does not report.
    pub owner: Option<usize>,
    /// Code span or link class.
    pub kind: ReferenceKind,
    /// Whitespace-normalized code, or the raw destination without `<` `>`.
    pub name: String,
    /// The code span with its backticks, or the destination.
    pub span: Span,
    /// One-based inclusive lines of the reference site.
    pub lines: [u32; 2],
}

/// Kind of code block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeBlockKind {
    /// Backtick or tilde fence with its trimmed info string.
    Fenced {
        /// Info string after the opening fence; may be empty.
        info: String,
    },
    /// Lines indented by four or more columns.
    Indented,
}

/// A code block; its content is never read as Markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlock {
    /// Fenced or indented.
    pub kind: CodeBlockKind,
    /// From the opening fence or first code line through the last line.
    pub span: Span,
    /// One-based inclusive lines.
    pub lines: [u32; 2],
}

/// Everything the extractor found in one Markdown file, in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Headings in source order; `parent` links form the tree.
    pub headings: Vec<Heading>,
    /// Raw references in source order; repeated sites stay separate.
    pub references: Vec<Reference>,
    /// Fenced and indented code blocks in source order.
    pub code_blocks: Vec<CodeBlock>,
    /// Skipped `---` or `+++` front matter at the start of the file.
    pub front_matter: Option<Span>,
    /// Line count of the file, at least one, as for the module node.
    pub lines: u32,
}

impl Document {
    /// Stable-id name of heading `index`: the slug plus `~ordinal` for duplicates.
    ///
    /// Returns `None` for any index outside [`Document::headings`]. Repeated titles
    /// receive ordinals in source order, starting at one.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn stable_name(&self, index: usize) -> Option<String> {
        let heading = self.headings.get(index)?;
        Some(match heading.ordinal {
            Some(ordinal) => format!("{}~{ordinal}", heading.name),
            None => heading.name.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_slice_safely_and_bodies_follow_the_heading_block() {
        let span = Span { start: 2, end: 5 };
        assert_eq!(span.text("# Title"), "Tit");
        assert_eq!(Span { start: 9, end: 12 }.text("short"), "");
        let heading = Heading {
            level: 1,
            style: HeadingStyle::Atx,
            title: "A".into(),
            name: "a".into(),
            ordinal: Some(2),
            parent: None,
            span: Span { start: 0, end: 4 },
            section: Span { start: 0, end: 9 },
            lines: [1, 2],
            summary: "A".into(),
        };
        assert_eq!(heading.body(), Span { start: 4, end: 9 });
        let document = Document {
            headings: vec![heading],
            references: Vec::new(),
            code_blocks: Vec::new(),
            front_matter: None,
            lines: 2,
        };
        assert_eq!(document.stable_name(0).as_deref(), Some("a~2"));
        assert_eq!(document.stable_name(1), None);
    }
}
