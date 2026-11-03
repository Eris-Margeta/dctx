// SPDX-License-Identifier: Apache-2.0
//! Markdown sections from dctx-lex-md, plus the optional tree-sitter walker.
//! Implements SPEC-MAP-MODE sections 5.1–5.4 and 6.2 and the PARSERS-v1 Markdown row.
//! Headings become doc nodes; inline code and link destinations stay unresolved.
//! Fenced and indented code never create headings. Tree-sitter is the feature oracle.
use super::facts::{Facts, file_precision};
use crate::{Language, ParseError, ParsedFile, body_fingerprint};
use dctx_lex_md::LexError;
use dctx_map_model::{
    Completion, Coverage, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource,
};

#[cfg(feature = "syntax-tree-sitter")]
use super::facts::{lines, text};
#[cfg(feature = "syntax-tree-sitter")]
use super::rust_items::named_children;
#[cfg(feature = "syntax-tree-sitter")]
use tree_sitter::Node as Syntax;

/// Maps a Markdown document onto doc nodes and documentation references.
pub(super) fn parse_pure(path: &str, source: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError> {
    let document = dctx_lex_md::extract(bytes).map_err(|error| map_error(path, error))?;
    let precision = file_precision(Coverage::Structural, Completion::Complete, "dctx-lex-md:1")?;
    let mut facts = Facts::new(path, source, Language::Markdown, precision)?;
    let mut owners = Vec::with_capacity(document.headings.len());
    for heading in &document.headings {
        let parent = heading
            .parent
            .and_then(|index| owners.get(index).copied())
            .unwrap_or(0);
        let body = heading.body().text(source);
        let owner = facts.node(
            Node {
                id: StableId::new(NodeKind::Doc, &facts.file.path, Some(&heading.name), None)?,
                kind: NodeKind::Doc,
                symbol_kind: None,
                path: facts.file.path.clone(),
                lines: heading.lines,
                name: heading.name.clone(),
                signature: heading.title.clone(),
                fingerprint: body_fingerprint(body),
                public: false,
                summary: Summary {
                    text: heading.summary.clone(),
                    source: SummarySource::Heading,
                },
            },
            parent,
        );
        owners.push(owner);
    }
    for reference in &document.references {
        let Some(owner) = reference.owner.and_then(|index| owners.get(index).copied()) else {
            continue;
        };
        facts.reference(
            owner,
            EdgeKind::Documents,
            reference.name.clone(),
            "",
            None,
            reference.lines,
        );
    }
    facts.finish()
}

fn map_error(path: &str, error: LexError) -> ParseError {
    match error {
        LexError::TooLarge { .. } => ParseError::TooLarge,
        LexError::TooDeep { line, .. } => ParseError::Syntax {
            path: path.into(),
            line: line.max(1),
        },
        LexError::InvalidUtf8 { .. } => ParseError::Syntax {
            path: path.into(),
            line: 1,
        },
    }
}

#[cfg(feature = "syntax-tree-sitter")]
struct Heading<'a> {
    syntax: Syntax<'a>,
    level: usize,
    title: String,
    owner: usize,
}

#[cfg(feature = "syntax-tree-sitter")]
pub(super) fn parse(source: &str, facts: &mut Facts) -> Result<(), ParseError> {
    let tree = super::tree(source, tree_sitter_md::LANGUAGE.into())?;
    let mut headings = Vec::new();
    let mut inlines = Vec::new();
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        match node.kind() {
            "atx_heading" | "setext_heading" => {
                let (level, title) = heading(node, source);
                headings.push(Heading {
                    syntax: node,
                    level,
                    title,
                    owner: 0,
                });
            }
            "inline" => inlines.push(node),
            "fenced_code_block" | "indented_code_block" | "html_block" => continue,
            _ => {}
        }
        pending.extend(named_children(node).into_iter().rev());
    }
    let mut parents: Vec<(usize, usize)> = Vec::new();
    for index in 0..headings.len() {
        let current = &headings[index];
        let end = headings[index + 1..]
            .iter()
            .find(|h| h.level <= current.level)
            .map_or(source.len(), |h| h.syntax.start_byte());
        let body_start = current.syntax.end_byte();
        let next_heading = headings
            .get(index + 1)
            .map_or(end, |h| h.syntax.start_byte());
        let body = &source[body_start..end];
        let summary = inlines
            .iter()
            .find(|inline| inline.start_byte() >= body_start && inline.start_byte() < next_heading)
            .map(|inline| super::facts::sentence(text(*inline, source)))
            .unwrap_or_else(|| current.title.clone());
        let name = slug(&current.title);
        while parents
            .last()
            .is_some_and(|(level, _)| *level >= current.level)
        {
            parents.pop();
        }
        let parent = parents.last().map_or(0, |(_, owner)| *owner);
        let owner = facts.node(
            Node {
                id: StableId::new(NodeKind::Doc, &facts.file.path, Some(&name), None)?,
                kind: NodeKind::Doc,
                symbol_kind: None,
                path: facts.file.path.clone(),
                lines: [
                    lines(current.syntax)[0],
                    source[..end].lines().count().max(1) as u32,
                ],
                name,
                signature: current.title.clone(),
                fingerprint: body_fingerprint(body),
                public: false,
                summary: Summary {
                    text: summary,
                    source: SummarySource::Heading,
                },
            },
            parent,
        );
        parents.push((current.level, owner));
        headings[index].owner = owner;
    }
    for inline in inlines {
        if let Some(heading) = headings
            .iter()
            .rev()
            .find(|h| h.syntax.end_byte() <= inline.start_byte())
        {
            if headings
                .iter()
                .any(|h| h.syntax.byte_range().contains(&inline.start_byte()))
            {
                continue;
            }
            links(inline, source, heading.owner, facts)?;
        }
    }
    Ok(())
}

#[cfg(feature = "syntax-tree-sitter")]
fn heading(node: Syntax<'_>, source: &str) -> (usize, String) {
    let children = named_children(node);
    let level = children
        .iter()
        .find_map(|child| match child.kind() {
            "atx_h1_marker" | "setext_h1_underline" => Some(1),
            "atx_h2_marker" | "setext_h2_underline" => Some(2),
            "atx_h3_marker" => Some(3),
            "atx_h4_marker" => Some(4),
            "atx_h5_marker" => Some(5),
            "atx_h6_marker" => Some(6),
            _ => None,
        })
        .unwrap_or(1);
    let title = children
        .iter()
        .find(|n| matches!(n.kind(), "inline" | "paragraph"))
        .map(|n| text(*n, source).trim().to_owned())
        .unwrap_or_default();
    (level, title)
}

#[cfg(feature = "syntax-tree-sitter")]
fn slug(title: &str) -> String {
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

#[cfg(feature = "syntax-tree-sitter")]
fn links(
    inline: Syntax<'_>,
    source: &str,
    owner: usize,
    facts: &mut Facts,
) -> Result<(), ParseError> {
    let content = text(inline, source);
    let tree = super::tree(content, tree_sitter_md::INLINE_LANGUAGE.into())?;
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        if matches!(node.kind(), "code_span" | "link_destination") {
            let name = if node.kind() == "code_span" {
                text(node, content)
                    .trim_matches('`')
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                text(node, content).trim_matches(['<', '>']).into()
            };
            let local = lines(node);
            let offset = inline.start_position().row as u32;
            facts.reference(
                owner,
                EdgeKind::Documents,
                name,
                "",
                None,
                [local[0] + offset, local[1] + offset],
            );
        } else {
            pending.extend(named_children(node).into_iter().rev());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Parser, PureParser};

    #[cfg(feature = "syntax-tree-sitter")]
    #[test]
    fn slugs_are_unicode_readable_and_valid_id_components() {
        assert_eq!(slug("5.2 Stable IDs!"), "52-stable-ids");
        assert_eq!(slug("Čuvaj Život"), "čuvaj-život");
        assert_eq!(slug("~ #"), "section");
    }

    #[test]
    fn fences_and_inline_code_do_not_make_headings() {
        let facts = PureParser
            .parse(
                "spec.md",
                b"# Real\n\n```md\n# Fake\n`fake`\n```\n\n`Thing` and [source](src/lib.rs).\n",
            )
            .unwrap();
        assert_eq!(facts.nodes.len(), 2);
        assert_eq!(facts.precision.coverage(), Coverage::Structural);
        assert_eq!(
            facts
                .references
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            ["Thing", "src/lib.rs"]
        );
    }
}
