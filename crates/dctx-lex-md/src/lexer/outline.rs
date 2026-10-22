//! Heading tree, section spans, slugs, ordinals, summaries and placed references.
//! Implements SPEC-MAP-MODE 5.1–5.4 doc nodes exactly as dctx-map-parse computes them.
//! Uses scanned blocks and the inline scanner; produces the public `Document`.
use super::blocks::{Blocks, Paragraph};
use super::inline;
use crate::{Document, Heading, Limits, Reference, Span};
use std::collections::BTreeMap;

/// Line arithmetic over the newline offsets of one source.
struct LineIndex {
    newlines: Vec<usize>,
}

impl LineIndex {
    fn new(source: &str) -> Self {
        let newlines = source.match_indices('\n').map(|(at, _)| at).collect();
        Self { newlines }
    }

    /// `source[..end].lines().count()` without rescanning.
    fn lines_before(&self, source: &str, end: usize) -> u32 {
        let full = self.newlines.partition_point(|at| *at < end);
        let partial = end > 0 && source.as_bytes().get(end - 1) != Some(&b'\n');
        u32::try_from(full + usize::from(partial)).unwrap_or(u32::MAX)
    }
}

pub(crate) fn document(
    source: &str,
    blocks: Blocks,
    front_matter: Option<Span>,
    limits: &Limits,
) -> Document {
    let index = LineIndex::new(source);
    let starts: Vec<usize> = blocks
        .paragraphs
        .iter()
        .map(|p| p.segments.first().map_or(0, |s| s.start))
        .collect();
    let mut headings: Vec<Heading> = Vec::with_capacity(blocks.headings.len());
    let mut parents: Vec<(u8, usize)> = Vec::new();
    for (i, raw) in blocks.headings.iter().enumerate() {
        let later = &blocks.headings[i + 1..];
        let end = later
            .iter()
            .find(|h| h.level <= raw.level)
            .map_or(source.len(), |h| h.span.start);
        let next = later.first().map_or(end, |h| h.span.start);
        let first = starts.partition_point(|start| *start < raw.span.end);
        let summary = starts
            .get(first)
            .filter(|start| **start < next).map_or_else(|| raw.title.clone(), |_| sentence(&paragraph_text(source, &blocks.paragraphs[first])));
        while parents.last().is_some_and(|(level, _)| *level >= raw.level) {
            parents.pop();
        }
        headings.push(Heading {
            level: raw.level,
            style: raw.style,
            title: raw.title.clone(),
            name: slug(&raw.title),
            ordinal: None,
            parent: parents.last().map(|(_, parent)| *parent),
            span: raw.span,
            section: Span {
                start: raw.span.start,
                end,
            },
            lines: [raw.line, index.lines_before(source, end).max(1)],
            summary,
        });
        parents.push((raw.level, i));
    }
    number_duplicates(&mut headings);
    let references = blocks
        .paragraphs
        .iter()
        .flat_map(|paragraph| place(source, paragraph, limits))
        .collect();
    Document {
        headings,
        references,
        code_blocks: blocks.code_blocks,
        front_matter,
        lines: index.lines_before(source, source.len()).max(1),
    }
}

fn number_duplicates(headings: &mut [Heading]) {
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for heading in headings.iter() {
        *counts.entry(heading.name.clone()).or_default() += 1;
    }
    let mut seen: BTreeMap<String, u32> = BTreeMap::new();
    for heading in headings.iter_mut() {
        if counts.get(&heading.name).is_some_and(|count| *count > 1) {
            let next = seen.entry(heading.name.clone()).or_default();
            *next = next.saturating_add(1);
            heading.ordinal = Some(*next);
        }
    }
}

fn paragraph_text(source: &str, paragraph: &Paragraph) -> String {
    paragraph
        .segments
        .iter()
        .map(|s| &source[s.start..s.end])
        .collect::<Vec<_>>()
        .join("\n")
}

/// Inline references of one paragraph, mapped back to source bytes and lines.
fn place(source: &str, paragraph: &Paragraph, limits: &Limits) -> Vec<Reference> {
    let text = paragraph_text(source, paragraph);
    let mut offsets = Vec::with_capacity(paragraph.segments.len());
    let mut at = 0;
    for segment in &paragraph.segments {
        offsets.push(at);
        at += segment.end - segment.start + 1;
    }
    let locate = |offset: usize, exclusive: bool| {
        let i = if exclusive {
            offsets
                .partition_point(|start| *start < offset)
                .saturating_sub(1)
        } else {
            offsets
                .partition_point(|start| *start <= offset)
                .saturating_sub(1)
        };
        let segment = &paragraph.segments[i];
        (segment.start + (offset - offsets[i]), segment.line)
    };
    inline::scan(&text, limits.max_brackets)
        .into_iter()
        .map(|found| {
            let (start, first) = locate(found.start, false);
            let (end, last) = locate(found.end, true);
            Reference {
                owner: paragraph.owner,
                kind: found.kind,
                name: found.name,
                span: Span { start, end },
                lines: [first, last],
            }
        })
        .collect()
}

/// Whitespace-normalized text cut after its first sentence, as dctx-map-parse does.
pub(crate) fn sentence(value: &str) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    match value.find(". ") {
        Some(end) => value[..=end].into(),
        None => value,
    }
}

/// Lowercase, alphanumeric slug with `-` for whitespace; `section` when empty.
pub(crate) fn slug(title: &str) -> String {
    let mut slug = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || matches!(c, '-' | '_') {
            slug.push(c);
        } else if c.is_whitespace() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "section".into()
    } else {
        slug.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_match_the_map_parse_rules() {
        assert_eq!(slug("5.2 Stable IDs!"), "52-stable-ids");
        assert_eq!(slug("Čuvaj Život"), "čuvaj-život");
        assert_eq!(slug("~ #"), "section");
        assert_eq!(slug("a  -  b"), "a--b");
    }

    #[test]
    fn sentences_stop_at_the_first_period_space() {
        assert_eq!(sentence("First  line.\nSecond."), "First line.");
        assert_eq!(sentence("v1.2 is out"), "v1.2 is out");
    }

    #[test]
    fn line_counts_agree_with_str_lines() {
        for source in ["", "a", "a\n", "a\nb", "\n\n", "a\r\nb\r\n"] {
            let index = LineIndex::new(source);
            for end in 0..=source.len() {
                assert_eq!(
                    index.lines_before(source, end) as usize,
                    source[..end].lines().count(),
                    "{source:?} at {end}"
                );
            }
        }
    }
}
