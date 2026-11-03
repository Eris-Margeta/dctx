// SPDX-License-Identifier: Apache-2.0
//! Small readings of syn nodes: identifiers, visibility, doc and test attributes.
//! Implements the PARSERS-v1 Rust row "doc comments, test attributes".
//! Uses syn attributes and the domain text rules; used by the syntax visitor and refs.
use super::syntax_refs::path_name;
use crate::domain::text::{first_doc_line, is_test_path, unraw};
use proc_macro2::Span;
use syn::{Attribute, Expr, ExprLit, Fields, Ident, Lit, Meta, Visibility};

pub(super) fn ident(ident: &Ident) -> String {
    unraw(&ident.to_string()).to_owned()
}

pub(super) fn public(vis: &Visibility) -> bool {
    !matches!(vis, Visibility::Inherited)
}

/// First non-empty line of `#[doc = "..."]` attributes, which `///` and `//!` become.
pub(super) fn doc(attrs: &[Attribute]) -> Option<String> {
    let docs: Vec<String> = attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            Meta::NameValue(pair) => match &pair.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(text),
                    ..
                }) => Some(text.value()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    first_doc_line(docs.iter().map(String::as_str))
}

pub(super) fn is_test(attrs: &[Attribute]) -> bool {
    attrs
        .iter()
        .any(|attr| is_test_path(&path_name(attr.path())))
}

/// Closing token of a struct body: the `;` when present, else its braces or parens.
pub(super) fn fields_end(fields: &Fields, semi: Option<Span>) -> Option<Span> {
    semi.or(match fields {
        Fields::Named(named) => Some(named.brace_token.span.close()),
        Fields::Unnamed(unnamed) => Some(unnamed.paren_token.span.close()),
        Fields::Unit => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_tests_and_visibility_are_read_from_attributes() {
        let item: syn::ItemFn = syn::parse_str("/// \n/// First.\n/// Second.\n#[tokio::test]\npub(crate) async fn r#check() {}")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc(&item.attrs).as_deref(), Some("First."));
        assert!(is_test(&item.attrs));
        assert!(public(&item.vis));
        assert_eq!(ident(&item.sig.ident), "check");
        let plain: syn::ItemFn = syn::parse_str("#[inline] fn f() {}").unwrap_or_else(|e| panic!("{e}"));
        assert!(!is_test(&plain.attrs));
        assert_eq!(doc(&plain.attrs), None);
        assert!(!public(&plain.vis));
    }
}
