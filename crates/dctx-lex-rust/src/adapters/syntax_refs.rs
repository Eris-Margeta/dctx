// SPDX-License-Identifier: Apache-2.0
//! Raw imports (use trees, file modules, extern crates) and raw call references.
//! Implements PARSERS-v1 "use/reexports" and "raw call references" with C05 labels:
//! path calls are `Name`, method calls stay `Unresolved`. Macro arguments are read as
//! comma-separated expressions, else walked as tokens (so is a `macro_rules!` body).
//! Uses syn nodes and the visitor's scope; called from the syntax visitor.
use super::spans::{between, expr_start, node_span, source_span, text};
use super::syntax::Extractor;
use super::syntax_parts::{ident, public};
use crate::domain::text::compact;
use crate::{
    Confidence, ImportKind, Precision, RawImport, RawReference, ReferenceKind, UnresolvedReason,
};
use proc_macro2::{Delimiter, Span, TokenStream, TokenTree};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::Visit;
use syn::{Expr, UseTree};

/// Words that introduce a declared name, which is not a call even when a group follows it.
const DECLARERS: &[&str] = &["enum", "fn", "mod", "struct", "trait", "type", "union"];

/// Keywords that may stand before a parenthesised group without being a call.
const NOT_CALLEES: &[&str] = &[
    "as", "async", "await", "box", "break", "const", "continue", "dyn", "else", "enum", "extern",
    "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "static", "struct", "trait", "type", "union", "unsafe", "use", "where", "while",
    "yield",
];

/// Longest kept method receiver text; longer receivers keep only the method name.
const MAX_RECEIVER_BYTES: usize = 200;

/// `a::b::c` with a leading `::` when present; generic arguments are dropped.
pub(super) fn path_name(path: &syn::Path) -> String {
    let mut name = String::from(if path.leading_colon.is_some() {
        "::"
    } else {
        ""
    });
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
    fn import(
        &mut self,
        kind: ImportKind,
        path: String,
        alias: Option<String>,
        public: bool,
        span: Span,
    ) {
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

    fn reference(
        &mut self,
        kind: ReferenceKind,
        target: String,
        confidence: Confidence,
        span: Span,
    ) {
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
    let root = if node.leading_colon.is_some() {
        "::"
    } else {
        ""
    };
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
                ex.import(
                    ImportKind::Use,
                    join(&prefix, &ident(&name.ident)),
                    None,
                    exported,
                    span,
                );
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
    let end = node
        .semi
        .as_ref()
        .map_or(node.ident.span(), |semi| semi.spans[0]);
    let span = between(&[&node.vis, &node.unsafety, &node.mod_token], end);
    ex.import(
        ImportKind::ModFile,
        ident(&node.ident),
        None,
        public(&node.vis),
        span,
    );
}

pub(super) fn extern_crate(ex: &mut Extractor<'_>, node: &syn::ItemExternCrate) {
    let span = between(&[&node.vis, &node.extern_token], node.semi_token.spans[0]);
    let alias = node.rename.as_ref().map(|(_, alias)| alias.to_string());
    ex.import(
        ImportKind::ExternCrate,
        ident(&node.ident),
        alias,
        public(&node.vis),
        span,
    );
}

pub(super) fn call(ex: &mut Extractor<'_>, node: &syn::ExprCall) {
    let Expr::Path(callee) = &*node.func else {
        return;
    };
    let (target, confidence) = if callee.qself.is_some() {
        let spelled = compact(text(ex.source, node_span(callee)));
        (
            spelled,
            Confidence::Unresolved(UnresolvedReason::QualifiedSelf),
        )
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
        (
            format!("{}.{method}", compact(receiver)),
            UnresolvedReason::MethodReceiver,
        )
    };
    let span = start.join(node.paren_token.span.close()).unwrap_or(start);
    ex.reference(
        ReferenceKind::MethodCall,
        target,
        Confidence::Unresolved(reason),
        span,
    );
}

/// Reads the arguments of a macro call such as `assert!`, `vec!` or `println!`.
/// Arguments that parse as comma-separated expressions are visited as code; any other
/// argument shape (`matches!` patterns, `quote!` bodies) is walked token by token.
pub(super) fn macro_args(ex: &mut Extractor<'_>, node: &syn::Macro) {
    let parse = Punctuated::<Expr, syn::Token![,]>::parse_terminated;
    match parse.parse2(node.tokens.clone()) {
        Ok(args) => args.iter().for_each(|arg| Visit::visit_expr(ex, arg)),
        Err(_) => token_calls(ex, node.tokens.clone()),
    }
}

/// True when `trees[at..]` starts with the path separator `::`.
fn colons(trees: &[TokenTree], at: usize) -> bool {
    let punct = |index: usize| match trees.get(index) {
        Some(TokenTree::Punct(p)) if p.as_char() == ':' => Some(p.spacing()),
        _ => None,
    };
    punct(at) == Some(proc_macro2::Spacing::Joint) && punct(at + 1).is_some()
}

/// Records calls found in raw macro tokens: `a::b(..)` is a path call, `.m(..)` a method call
/// with an unknown receiver. Declarations (`fn f(..)`), `$` metavariables and idents after
/// `::` that start no path are skipped, so only text that is written as a call counts.
pub(super) fn token_calls(ex: &mut Extractor<'_>, tokens: TokenStream) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut at = 0;
    while at < trees.len() {
        let step = match &trees[at] {
            TokenTree::Group(group) => {
                token_calls(ex, group.stream());
                1
            }
            TokenTree::Punct(p) if p.as_char() == '$' => {
                1 + usize::from(matches!(trees.get(at + 1), Some(TokenTree::Ident(_))))
            }
            TokenTree::Ident(first) => token_call(ex, &trees, at, first),
            _ => 1,
        };
        at += step;
    }
}

/// Handles the identifier at `trees[at]`; returns how many tokens to advance.
fn token_call(
    ex: &mut Extractor<'_>,
    trees: &[TokenTree],
    at: usize,
    first: &proc_macro2::Ident,
) -> usize {
    let before = at.checked_sub(1).and_then(|i| trees.get(i));
    let after_colons = at >= 2 && colons(trees, at - 2);
    let range = at >= 2
        && matches!(&trees[at - 2], TokenTree::Punct(p) if p.as_char() == '.' && p.spacing() == proc_macro2::Spacing::Joint);
    let method = !range && matches!(before, Some(TokenTree::Punct(p)) if p.as_char() == '.');
    let declared =
        matches!(before, Some(TokenTree::Ident(w)) if DECLARERS.contains(&w.to_string().as_str()));
    let (mut name, mut end) = (ident(first), at);
    while colons(trees, end + 1) {
        match trees.get(end + 3) {
            Some(TokenTree::Ident(next)) => {
                (name, end) = (format!("{name}::{}", ident(next)), end + 3)
            }
            _ => break,
        }
    }
    let called = match trees.get(end + 1) {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
            Some(group.span_close())
        }
        _ => None,
    };
    let keyword = NOT_CALLEES.contains(&name.as_str());
    if let (Some(close), false) = (
        called,
        after_colons || declared || keyword || (method && end > at),
    ) {
        let span = first.span().join(close).unwrap_or(close);
        if method {
            ex.reference(
                ReferenceKind::MethodCall,
                name,
                Confidence::Unresolved(UnresolvedReason::Truncated),
                span,
            );
        } else {
            ex.reference(ReferenceKind::Call, name, Confidence::Name, span);
        }
    }
    end - at + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_arguments_and_macro_rules_bodies_emit_calls() {
        let source = "pub fn in_assert() { assert!(W::target() == 1); }\npub fn in_eq() { assert_eq!(W::target(), 1); }\npub fn in_vec() -> Vec<u32> { vec![W::target()] }\npub fn in_print() { println!(\"{}\", W::target()); }\npub fn in_matches(x: u8) -> bool { matches!(x, 1 | 2 if W::target() > 0) }\nmacro_rules! mk { ($v:expr) => { W::target(); $v.go(); fn phantom() {} Vec::<u8>::new(); x..y(1) }; }\nmacro_rules! lit { () => { 1 + 2 }; }\n";
        let facts = super::super::syntax::extract(source, &crate::Limits::default())
            .unwrap_or_else(|e| panic!("{e}"));
        let found: Vec<(Option<&str>, &str)> = facts
            .references
            .iter()
            .map(|r| {
                (
                    r.from.map(|i| facts.declarations[i].name.as_str()),
                    r.target.as_str(),
                )
            })
            .collect();
        let want = [
            (Some("in_assert"), "W::target"),
            (Some("in_eq"), "W::target"),
            (Some("in_vec"), "W::target"),
            (Some("in_print"), "W::target"),
            (Some("in_matches"), "W::target"),
            (Some("mk"), "W::target"),
            (Some("mk"), "go"),
            (Some("mk"), "y"),
        ];
        assert!(found == want, "error: {found:?}");
    }

    #[test]
    fn use_prefixes_join_like_the_oracle() {
        assert_eq!(join("", "a"), "a");
        assert_eq!(join("::", "a"), "::a");
        assert_eq!(join("crate::a", "self"), "crate::a");
        assert_eq!(join("crate::a", "*"), "crate::a::*");
        let path: syn::Path =
            syn::parse_str("::std::vec::Vec::<u8>").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(path_name(&path), "::std::vec::Vec");
    }
}
