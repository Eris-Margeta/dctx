// SPDX-License-Identifier: Apache-2.0
//! Rust facts from dctx-lex-rust, plus the optional tree-sitter walker.
//! Implements SPEC-MAP-MODE sections 5.1, 5.3 and 6.2 and the PARSERS-v1 Rust row.
//! syn facts become model nodes; a partial extraction is refused, not labelled complete.
//! The query index uses that refusal to mask and retry. Tree-sitter is the feature oracle.
use super::facts::{Facts, file_precision, sentence};
use crate::{Language, ParseError, ParsedFile, body_fingerprint};
use dctx_lex_rust::{
    Completion, Declaration, DeclarationKind, Extraction, ImportKind, Limits, Refusal,
};
use dctx_map_model::{
    Completion as FileCompletion, Coverage, EdgeKind, Node, NodeKind, StableId, Summary,
    SummarySource, SymbolKind,
};

#[cfg(feature = "syntax-tree-sitter")]
use super::rust_items::{Scope, item, named_children};
#[cfg(feature = "syntax-tree-sitter")]
use super::rust_references::references;

/// Maps a complete syn extraction. Partial files become [`ParseError::Syntax`].
pub(super) fn parse_pure(path: &str, source: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError> {
    let extracted =
        dctx_lex_rust::extract(bytes, &Limits::default()).map_err(|refusal| match refusal {
            Refusal::TooLarge { .. } => ParseError::TooLarge,
            Refusal::Worker => ParseError::Grammar(Refusal::Worker.to_string()),
            other => syntax_error(path, &other),
        })?;
    if let Completion::Partial(refusal) = &extracted.completion {
        return Err(syntax_error(path, refusal));
    }
    let parser = format!(
        "{}:{}",
        dctx_lex_rust::PARSER_ID,
        dctx_lex_rust::PARSER_REVISION
    );
    let precision = file_precision(Coverage::Syntax, FileCompletion::Complete, &parser)?;
    let mut facts = Facts::new(path, source, Language::Rust, precision)?;
    if let Some(doc) = &extracted.module_doc {
        facts.file.nodes[0].summary.text = sentence(doc);
        facts.file.nodes[0].summary.source = SummarySource::Doc;
    }
    let nodes = declare(&extracted, source, &mut facts)?;
    let mut pending = references_of(&extracted, source, &nodes);
    pending.sort_by_key(|site| (site.at, site.rank, site.seq));
    for site in pending {
        facts.reference(
            site.src,
            site.kind,
            site.name,
            &site.scope,
            site.alias,
            site.lines,
        );
    }
    facts.finish()
}

fn syntax_error(path: &str, refusal: &Refusal) -> ParseError {
    let line = match refusal {
        Refusal::Syntax { line } | Refusal::Lex { line } | Refusal::TooDeep { line, .. } => {
            (*line).max(1)
        }
        _ => 1,
    };
    ParseError::Syntax {
        path: path.into(),
        line,
    }
}

struct Placed {
    node: usize,
    scope: String,
}

fn declare(
    extracted: &Extraction,
    source: &str,
    facts: &mut Facts,
) -> Result<Vec<Placed>, ParseError> {
    let mut placed: Vec<Placed> = Vec::with_capacity(extracted.declarations.len());
    for declaration in &extracted.declarations {
        let parent_scope = declaration
            .parent
            .and_then(|index| placed.get(index))
            .map(|parent| parent.scope.clone())
            .unwrap_or_default();
        let scope = child_scope(declaration, &parent_scope);
        let kind = if declaration.test {
            NodeKind::Test
        } else {
            NodeKind::Symbol
        };
        let item = source
            .get(declaration.span.start..declaration.span.end)
            .unwrap_or("");
        let signature = signature_of(item);
        let summary = match &declaration.doc {
            Some(doc) => Summary {
                text: sentence(doc),
                source: SummarySource::Doc,
            },
            None => Summary {
                text: signature.clone(),
                source: SummarySource::Signature,
            },
        };
        let node = facts.node(
            Node {
                id: StableId::new(kind, &facts.file.path, Some(&declaration.name), None)?,
                kind,
                symbol_kind: symbol_kind(declaration.kind),
                path: facts.file.path.clone(),
                lines: span_lines(declaration),
                name: declaration.name.clone(),
                signature,
                fingerprint: fingerprint(declaration.kind, &declaration.name, item),
                public: declaration.public,
                summary,
            },
            declaration
                .parent
                .and_then(|index| placed.get(index))
                .map_or(0, |p| p.node),
        );
        placed.push(Placed { node, scope });
    }
    Ok(placed)
}

fn child_scope(declaration: &Declaration, parent: &str) -> String {
    match &declaration.implementation {
        Some(implementation) => qualify(parent, impl_scope(&implementation.self_type)),
        None => declaration.name.clone(),
    }
}

/// Scope of items inside an impl: `Vec<T>` becomes `Vec`. Matches dctx-lex-rust.
fn impl_scope(self_type: &str) -> &str {
    if self_type.starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == ':') {
        self_type.split('<').next().unwrap_or(self_type)
    } else {
        self_type
    }
}

fn qualify(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.into()
    } else {
        format!("{scope}::{name}")
    }
}

fn symbol_kind(kind: DeclarationKind) -> Option<SymbolKind> {
    Some(match kind {
        DeclarationKind::Module => return None,
        DeclarationKind::Struct | DeclarationKind::Union => SymbolKind::Struct,
        DeclarationKind::Enum => SymbolKind::Enum,
        DeclarationKind::Trait | DeclarationKind::TraitAlias => SymbolKind::Trait,
        DeclarationKind::Impl => SymbolKind::Impl,
        DeclarationKind::Function => SymbolKind::Function,
        DeclarationKind::Method => SymbolKind::Method,
        DeclarationKind::Const | DeclarationKind::Static => SymbolKind::Const,
        DeclarationKind::TypeAlias => SymbolKind::Type,
        DeclarationKind::Macro => SymbolKind::Macro,
    })
}

fn signature_of(item: &str) -> String {
    item.split('{')
        .next()
        .unwrap_or(item)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn fingerprint(kind: DeclarationKind, name: &str, item: &str) -> String {
    if matches!(
        kind,
        DeclarationKind::Function
            | DeclarationKind::Method
            | DeclarationKind::Impl
            | DeclarationKind::Trait
            | DeclarationKind::Enum
            | DeclarationKind::Union
            | DeclarationKind::Module
            | DeclarationKind::Macro
    ) && let Some(body) = item.find('{')
    {
        return body_fingerprint(&item[body..]);
    }
    let local = name.rsplit("::").next().unwrap_or(name);
    match item.find(local) {
        Some(at) => {
            let mut text = String::new();
            text.push_str(&item[..at]);
            text.push(' ');
            text.push_str(&item[at + local.len()..]);
            body_fingerprint(&text)
        }
        None => body_fingerprint(item),
    }
}

fn span_lines(declaration: &Declaration) -> [u32; 2] {
    let start = declaration.span.lines[0].max(1);
    [start, declaration.span.lines[1].max(start)]
}

struct Site {
    at: usize,
    rank: u8,
    seq: u32,
    src: usize,
    kind: EdgeKind,
    name: String,
    scope: String,
    alias: Option<String>,
    lines: [u32; 2],
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep the source-ordered reference dispatch together with its shared scope and declaration state."
)]
fn references_of(extracted: &Extraction, source: &str, placed: &[Placed]) -> Vec<Site> {
    let mut sites = Vec::new();
    let mut seq = 0_u32;
    for import in &extracted.imports {
        let lines = [import.span.lines[0].max(1), import.span.lines[1].max(1)];
        remember(
            &mut sites,
            &mut seq,
            Site {
                at: import.span.start,
                rank: 0,
                seq: 0,
                src: 0,
                kind: EdgeKind::Imports,
                name: import.path.clone(),
                scope: import.scope.clone(),
                alias: import.alias.clone(),
                lines,
            },
        );
        if import.public && import.kind == ImportKind::Use {
            remember(
                &mut sites,
                &mut seq,
                Site {
                    at: import.span.start,
                    rank: 1,
                    seq: 0,
                    src: 0,
                    kind: EdgeKind::Reexports,
                    name: import.path.clone(),
                    scope: import.scope.clone(),
                    alias: import.alias.clone(),
                    lines,
                },
            );
        }
    }
    for (index, declaration) in extracted.declarations.iter().enumerate() {
        let Some(placed) = placed.get(index) else {
            continue;
        };
        let lines = span_lines(declaration);
        if let Some(trait_path) = declaration
            .implementation
            .as_ref()
            .and_then(|implementation| implementation.trait_path.clone())
        {
            remember(
                &mut sites,
                &mut seq,
                Site {
                    at: declaration.span.start,
                    rank: 2,
                    seq: 0,
                    src: placed.node,
                    kind: EdgeKind::Implements,
                    name: trait_path,
                    scope: placed.scope.clone(),
                    alias: None,
                    lines,
                },
            );
        }
        if let Some(name) = used_type(declaration, source) {
            remember(
                &mut sites,
                &mut seq,
                Site {
                    at: declaration.span.start,
                    rank: 3,
                    seq: 0,
                    src: placed.node,
                    kind: EdgeKind::UsesType,
                    name,
                    scope: placed.scope.clone(),
                    alias: None,
                    lines,
                },
            );
        }
    }
    for reference in &extracted.references {
        let src = reference
            .from
            .and_then(|index| placed.get(index))
            .map_or(0, |item| item.node);
        let lines = [
            reference.span.lines[0].max(1),
            reference.span.lines[1].max(1),
        ];
        remember(
            &mut sites,
            &mut seq,
            Site {
                at: reference.span.start,
                rank: 4,
                seq: 0,
                src,
                kind: EdgeKind::Calls,
                name: reference.target.clone(),
                scope: reference.scope.clone(),
                alias: None,
                lines,
            },
        );
        if reference.in_test {
            remember(
                &mut sites,
                &mut seq,
                Site {
                    at: reference.span.start,
                    rank: 5,
                    seq: 0,
                    src,
                    kind: EdgeKind::Tests,
                    name: reference.target.clone(),
                    scope: reference.scope.clone(),
                    alias: None,
                    lines,
                },
            );
        }
    }
    sites
}

fn remember(sites: &mut Vec<Site>, seq: &mut u32, mut site: Site) {
    site.seq = *seq;
    *seq = seq.saturating_add(1);
    sites.push(site);
}

/// Impl self type, or the right-hand side of a type alias, when it is a path.
fn used_type(declaration: &Declaration, source: &str) -> Option<String> {
    if let Some(implementation) = &declaration.implementation {
        return path_type(impl_scope(&implementation.self_type));
    }
    if declaration.kind != DeclarationKind::TypeAlias {
        return None;
    }
    let text = source.get(declaration.span.start..declaration.span.end)?;
    let (_, rhs) = text.split_once('=')?;
    let rhs: String = rhs
        .trim()
        .trim_end_matches(';')
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    path_type(impl_scope(&rhs))
}

fn path_type(name: &str) -> Option<String> {
    if name.is_empty() || name == "Self" || primitive(name) {
        return None;
    }
    name.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
        .then(|| name.to_owned())
}

/// Tree-sitter records `type_identifier`, not primitive types such as `u32`.
fn primitive(name: &str) -> bool {
    matches!(
        name,
        "u8" | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "f32"
            | "f64"
            | "bool"
            | "char"
            | "str"
    )
}

#[cfg(feature = "syntax-tree-sitter")]
pub(super) fn parse(source: &str, facts: &mut Facts) -> Result<(), ParseError> {
    let tree = super::tree(source, tree_sitter_rust::LANGUAGE.into())?;
    let root = tree.root_node();
    if root.has_error() {
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if node.is_error() || node.is_missing() {
                return Err(ParseError::Syntax {
                    path: facts.file.path.clone(),
                    line: node.start_position().row as u32 + 1,
                });
            }
            pending.extend(
                named_children(node)
                    .into_iter()
                    .rev()
                    .filter(|n| n.has_error()),
            );
        }
        return Err(ParseError::Syntax {
            path: facts.file.path.clone(),
            line: 1,
        });
    }
    let mut pending = vec![(root, Scope::default())];
    while let Some((node, scope)) = pending.pop() {
        let context = item(node, source, &scope, facts)?.unwrap_or(scope);
        references(node, source, &context, facts);
        if matches!(
            node.kind(),
            "macro_definition" | "macro_invocation" | "use_declaration"
        ) {
            continue;
        }
        pending.extend(
            named_children(node)
                .into_iter()
                .rev()
                .map(|child| (child, context.clone())),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
