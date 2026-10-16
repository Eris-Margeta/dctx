//! Markdown heading sections and unresolved inline documentation links.
//! Implements SPEC-MAP-MODE sections 5.1–5.4 and 6.2.
//! Uses block and inline Markdown grammars; emits model nodes and raw links.
use super::facts::{Facts, lines, sentence, text};
use super::rust_items::named_children;
use crate::{ParseError, body_fingerprint};
use dctx_map_model::{EdgeKind, Node, NodeKind, StableId, Summary, SummarySource};
use tree_sitter::Node as Syntax;

struct Heading<'a> {
    syntax: Syntax<'a>,
    level: usize,
    title: String,
    owner: usize,
}

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
            .map(|inline| sentence(text(*inline, source)))
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
            // Heading text belongs to its own title, not to the preceding section.
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
    use crate::{Parser, TreeSitterParser};

    #[test]
    fn slugs_are_unicode_readable_and_valid_id_components() {
        assert_eq!(slug("5.2 Stable IDs!"), "52-stable-ids");
        assert_eq!(slug("Čuvaj Život"), "čuvaj-život");
        assert_eq!(slug("~ #"), "section");
    }

    #[test]
    fn fences_and_inline_code_do_not_make_headings() {
        let facts = TreeSitterParser
            .parse(
                "spec.md",
                b"# Real\n\n```md\n# Fake\n`fake`\n```\n\n`Thing` and [source](src/lib.rs).\n",
            )
            .unwrap();
        assert_eq!(facts.nodes.len(), 2);
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
