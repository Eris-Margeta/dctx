// SPDX-License-Identifier: Apache-2.0
//! Markdown lexer pipeline: front matter, block scan, then outline with references.
//! Implements the extraction steps behind `extract_with` (decision 077, ARCHITECTURE-PROPOSAL §3).
//! Pure functions over a validated UTF-8 source; no IO and no panics on any input.
mod blocks;
mod html;
mod inline;
mod lines;
mod markers;
mod outline;

use crate::{Document, LexError, Limits, Span};

pub(crate) fn document(source: &str, limits: &Limits) -> Result<Document, LexError> {
    let front_matter = lines::front_matter(source).map(|(start, end)| Span { start, end });
    let bom = if source.starts_with('\u{feff}') { 3 } else { 0 };
    let from = front_matter.map_or(bom, |span| span.end);
    let first_line = u32::try_from(source[..from].matches('\n').count() + 1).unwrap_or(u32::MAX);
    let mut scanner = blocks::Scanner::new(source, limits);
    for line in lines::Lines::new(source, from, first_line) {
        scanner.line(&line)?;
    }
    Ok(outline::document(
        source,
        scanner.finish(),
        front_matter,
        limits,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_is_skipped_and_numbering_continues() {
        let doc = document("---\ntitle: x\n---\n# Real\nBody.\n", &Limits::default()).unwrap();
        assert_eq!(doc.front_matter, Some(Span { start: 0, end: 17 }));
        assert_eq!(doc.headings.len(), 1);
        assert_eq!(doc.headings[0].lines, [4, 5]);
        assert_eq!(doc.headings[0].summary, "Body.");
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_first_heading() {
        let doc = document("\u{feff}# Title\n", &Limits::default()).unwrap();
        assert_eq!(doc.headings[0].span, Span { start: 3, end: 11 });
    }

    #[test]
    fn without_front_matter_a_dash_line_is_a_break() {
        let doc = document("---\ntitle: x\n", &Limits::default()).unwrap();
        assert_eq!(doc.front_matter, None);
        assert_eq!(doc.headings, []);
    }
}
