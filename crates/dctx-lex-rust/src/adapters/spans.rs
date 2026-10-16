//! Byte spans, lines and source text of syn nodes via proc-macro2 span locations.
//! Implements PARSERS-v1 "Each fact has a byte span [start,end) inside the input".
//! Spans are built from a node's small head and its closing token, never by printing a
//! whole body, so nested items and chains stay linear. Used by the syn visitor.
use crate::SourceSpan;
use proc_macro2::Span;
use quote::ToTokens;
use syn::Expr;

pub(super) fn source_span(span: Span) -> SourceSpan {
    let range = span.byte_range();
    let line = |line: usize| u32::try_from(line).unwrap_or(u32::MAX);
    let start = line(span.start().line);
    SourceSpan {
        start: range.start,
        end: range.end.max(range.start),
        lines: [start, line(span.end().line).max(start)],
    }
}

/// Span of the first token printed by any of `parts`, in order.
pub(super) fn first(parts: &[&dyn ToTokens]) -> Option<Span> {
    parts.iter().find_map(|part| {
        part.to_token_stream()
            .into_iter()
            .next()
            .map(|tree| tree.span())
    })
}

/// From the first token of `head` (outer attributes excluded by construction) to `end`.
pub(super) fn between(head: &[&dyn ToTokens], end: Span) -> Span {
    first(head).and_then(|start| start.join(end)).unwrap_or(end)
}

/// Whole-node span; prints the node, so only for small nodes such as types and paths.
pub(super) fn node_span(node: &dyn ToTokens) -> Span {
    let mut trees = node.to_token_stream().into_iter();
    let Some(first) = trees.next().map(|t| t.span()) else {
        return Span::call_site();
    };
    let last = trees.last().map_or(first, |t| t.span());
    first.join(last).unwrap_or(first)
}

/// First token of an expression by leftmost descent, without printing operands.
pub(super) fn expr_start(mut expr: &Expr) -> Span {
    loop {
        expr = match expr {
            Expr::MethodCall(e) => &*e.receiver,
            Expr::Call(e) => &*e.func,
            Expr::Field(e) => &*e.base,
            Expr::Index(e) => &*e.expr,
            Expr::Try(e) => &*e.expr,
            Expr::Await(e) => &*e.base,
            Expr::Cast(e) => &*e.expr,
            Expr::Binary(e) => &*e.left,
            Expr::Path(e) if e.attrs.is_empty() => {
                if let Some(qself) = &e.qself {
                    return qself.lt_token.spans[0];
                }
                if let Some(colon) = &e.path.leading_colon {
                    return colon.spans[0];
                }
                return e
                    .path
                    .segments
                    .first()
                    .map_or_else(|| node_span(expr), |s| s.ident.span());
            }
            Expr::Lit(e) if e.attrs.is_empty() => return e.lit.span(),
            Expr::Paren(e) if e.attrs.is_empty() => return e.paren_token.span.open(),
            Expr::Tuple(e) if e.attrs.is_empty() => return e.paren_token.span.open(),
            Expr::Array(e) if e.attrs.is_empty() => return e.bracket_token.span.open(),
            _ => return first(&[expr]).unwrap_or_else(Span::call_site),
        };
    }
}

pub(super) fn text(source: &str, span: Span) -> &str {
    source.get(span.byte_range()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse<T: syn::parse::Parse>(source: &str) -> T {
        let tokens: proc_macro2::TokenStream = source.parse().unwrap_or_default();
        syn::parse2(tokens).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn head_to_end_spans_exclude_attributes() {
        let source = "/// Doc.\n#[inline]\npub fn run() -> u8 { 3 }";
        let item: syn::ItemFn = parse(source);
        let span = between(&[&item.vis, &item.sig], item.block.brace_token.span.close());
        let span = source_span(span);
        assert_eq!(&source[span.start..span.end], "pub fn run() -> u8 { 3 }");
        assert_eq!(span.lines, [3, 3]);
        assert_eq!(text(source, node_span(&item.sig.output)), "-> u8");
    }

    #[test]
    fn expression_starts_follow_the_leftmost_operand() {
        let source = "(a).b::<T>()[0].c? + 1";
        let expr: Expr = parse(source);
        assert_eq!(source_span(expr_start(&expr)).start, 0);
        let source = "::x::y(1)";
        let expr: Expr = parse(source);
        assert_eq!(source_span(expr_start(&expr)).start, 0);
    }
}
