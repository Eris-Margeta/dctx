//! Raw imports (use trees, file modules, extern crates) and raw call references.
//! Implements PARSERS-v1 "use/reexports" and "raw call references" with C05 labels:
//! path calls are `Name`, method calls stay `Unresolved`; macro bodies are never read.
//! Uses syn nodes and the visitor's scope; called from the syntax visitor.
use super::spans::{between, expr_start, node_span, source_span, text};
use super::syntax::Extractor;
use super::syntax_parts::{ident, public};
use crate::domain::text::compact;
use crate::{
    Confidence, ImportKind, Precision, RawImport, RawReference, ReferenceKind, UnresolvedReason,
};
use proc_macro2::Span;
use syn::{Expr, UseTree};

/// Longest kept method receiver text; longer receivers keep only the method name.
const MAX_RECEIVER_BYTES: usize = 200;

/// `a::b::c` with a leading `::` when present; generic arguments are dropped.
pub(super) fn path_name(path: &syn::Path) -> String {
    let mut name = String::from(if path.leading_colon.is_some() { "::" } else { "" });
    for (index, segment) in path.segments.iter().enumerate() {
        if index > 0 {
            name.push_str("::");
        }
        name.push_str(&ident(&segment.ident));
    }
    name
}

fn join(prefix: &str, name: &str) -> String {
    match (prefix, name) {
        ("", _) => name.into(),
        ("::", _) => format!("::{name}"),
        (_, "self") => prefix.into(),
        _ => format!("{prefix}::{name}"),
    }
}

impl Extractor<'_> {
    fn import(&mut self, kind: ImportKind, path: String, alias: Option<String>, public: bool, span: Span) {
        let import = RawImport {
            kind,
            path,
            alias,
            public,
            scope: self.scope.name.clone(),
            span: source_span(span),
            precision: Precision::Syntax,
        };
        self.sink.import(import);
    }

    fn reference(&mut self, kind: ReferenceKind, target: String, confidence: Confidence, span: Span) {
        let reference = RawReference {
            kind,
            from: self.scope.owner,
            target,
            scope: self.scope.name.clone(),
            in_test: self.scope.test,
            span: source_span(span),
            confidence,
        };
        self.sink.reference(reference);
    }
}

/// Flattens a use tree iteratively; each leaf spans from its first path segment.
pub(super) fn uses(ex: &mut Extractor<'_>, node: &syn::ItemUse) {
    let exported = public(&node.vis);
    let root = if node.leading_colon.is_some() { "::" } else { "" };
    let mut pending = vec![(&node.tree, root.to_owned(), None::<Span>)];
    while let Some((tree, prefix, start)) = pending.pop() {
        let leaf = |end: Span| start.and_then(|s| s.join(end)).unwrap_or(end);
        match tree {
            UseTree::Path(path) => {
                let start = start.or(Some(path.ident.span()));
                pending.push((&*path.tree, join(&prefix, &ident(&path.ident)), start));
            }
            UseTree::Name(name) => {
                let span = leaf(name.ident.span());
                ex.import(ImportKind::Use, join(&prefix, &ident(&name.ident)), None, exported, span);
            }
            UseTree::Rename(rename) => {
                let span = leaf(rename.rename.span());
                let path = join(&prefix, &ident(&rename.ident));
                let alias = Some(rename.rename.to_string());
                ex.import(ImportKind::Use, path, alias, exported, span);
            }
            UseTree::Glob(glob) => {
                let span = leaf(glob.star_token.spans[0]);
                ex.import(ImportKind::Use, join(&prefix, "*"), None, exported, span);
            }
            UseTree::Group(group) => {
                let items = group.items.iter().rev();
                pending.extend(items.map(|item| (item, prefix.clone(), None)));
            }
        }
    }
}

pub(super) fn module_file(ex: &mut Extractor<'_>, node: &syn::ItemMod) {
    let end = node.semi.as_ref().map_or(node.ident.span(), |semi| semi.spans[0]);
    let span = between(&[&node.vis, &node.unsafety, &node.mod_token], end);
    ex.import(ImportKind::ModFile, ident(&node.ident), None, public(&node.vis), span);
}

pub(super) fn extern_crate(ex: &mut Extractor<'_>, node: &syn::ItemExternCrate) {
    let span = between(&[&node.vis, &node.extern_token], node.semi_token.spans[0]);
    let alias = node.rename.as_ref().map(|(_, alias)| alias.to_string());
    ex.import(ImportKind::ExternCrate, ident(&node.ident), alias, public(&node.vis), span);
}

pub(super) fn call(ex: &mut Extractor<'_>, node: &syn::ExprCall) {
    let Expr::Path(callee) = &*node.func else {
        return;
    };
    let (target, confidence) = if callee.qself.is_some() {
        let spelled = compact(text(ex.source, node_span(callee)));
        (spelled, Confidence::Unresolved(UnresolvedReason::QualifiedSelf))
    } else {
        (path_name(&callee.path), Confidence::Name)
    };
    let start = expr_start(&node.func);
    let span = start.join(node.paren_token.span.close()).unwrap_or(start);
    ex.reference(ReferenceKind::Call, target, confidence, span);
}

pub(super) fn method_call(ex: &mut Extractor<'_>, node: &syn::ExprMethodCall) {
    let start = expr_start(&node.receiver);
    let dot = node.dot_token.spans[0];
    let receiver = start
        .join(dot)
        .map(|span| span.byte_range().start..dot.byte_range().start)
        .and_then(|range| ex.source.get(range))
        .unwrap_or_default();
    let method = ident(&node.method);
    let (target, reason) = if receiver.len() > MAX_RECEIVER_BYTES || receiver.trim().is_empty() {
        (method, UnresolvedReason::Truncated)
    } else {
        (format!("{}.{method}", compact(receiver)), UnresolvedReason::MethodReceiver)
    };
    let span = start.join(node.paren_token.span.close()).unwrap_or(start);
    ex.reference(ReferenceKind::MethodCall, target, Confidence::Unresolved(reason), span);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn use_prefixes_join_like_the_oracle() {
        assert_eq!(join("", "a"), "a");
        assert_eq!(join("::", "a"), "::a");
        assert_eq!(join("crate::a", "self"), "crate::a");
        assert_eq!(join("crate::a", "*"), "crate::a::*");
        let path: syn::Path = syn::parse_str("::std::vec::Vec::<u8>").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(path_name(&path), "::std::vec::Vec");
    }
}
