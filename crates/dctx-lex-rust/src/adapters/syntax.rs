// SPDX-License-Identifier: Apache-2.0
//! syn visitor that records declarations in pre-order with their lexical scopes.
//! Implements the PARSERS-v1 Rust row: items, modules, functions/methods, impls with trait
//! and type, consts/statics, type aliases, `macro_rules`, tests and doc first lines.
//! Uses proc-macro2/syn, the span helpers and `syntax_refs`; run on the parse worker.
use super::spans::{between, first, node_span, source_span, text};
use super::nesting;
use super::syntax_parts::{doc, fields_end, ident, is_test, public};
use crate::domain::sink::Sink;
use crate::domain::text::{compact, impl_scope, qualify};
use crate::{
    Coverage, Declaration, DeclarationKind as Kind, Extraction, Implementation, Limits, Precision,
    Refusal,
};
use proc_macro2::{Span, TokenStream};
use std::str::FromStr;
use syn::visit::{self, Visit};
use syn::Attribute;

/// Lexical context of the node being visited.
#[derive(Clone, Default)]
pub(super) struct Frame {
    pub name: String,
    pub owner: Option<usize>,
    pub test: bool,
    methods: bool,
    public_items: bool,
}

pub(super) struct Extractor<'s> {
    pub source: &'s str,
    pub sink: Sink,
    pub scope: Frame,
}

pub(super) fn extract(source: &str, limits: &Limits) -> Result<Extraction, Refusal> {
    let line = |line: usize| u32::try_from(line).unwrap_or(u32::MAX);
    let tokens = TokenStream::from_str(source).map_err(|e| Refusal::Lex {
        line: line(e.span().start().line),
    })?;
    nesting::check(&tokens, limits.max_nesting())?;
    let file: syn::File = syn::parse2(tokens).map_err(|e| Refusal::Syntax {
        line: line(e.span().start().line),
    })?;
    let mut extractor = Extractor {
        source,
        sink: Sink::new(limits),
        scope: Frame::default(),
    };
    extractor.visit_file(&file);
    Ok(extractor
        .sink
        .finish(Coverage::Syntax, None, doc(&file.attrs)))
}

impl Extractor<'_> {
    fn declare(
        &mut self,
        kind: Kind,
        local: &str,
        attrs: &[Attribute],
        public: bool,
        span: Span,
        implementation: Option<Implementation>,
    ) -> Frame {
        let methods = self.scope.methods;
        let kind = if kind == Kind::Function && methods {
            Kind::Method
        } else {
            kind
        };
        let public = public || (methods && self.scope.public_items);
        let test = matches!(kind, Kind::Function | Kind::Method) && is_test(attrs);
        let name = qualify(&self.scope.name, local);
        let child = match &implementation {
            Some(i) => qualify(&self.scope.name, impl_scope(&i.self_type)),
            None => name.clone(),
        };
        let owner = self.sink.declare(Declaration {
            kind,
            name,
            parent: self.scope.owner,
            span: source_span(span),
            public,
            test,
            doc: doc(attrs),
            implementation,
            precision: Precision::Syntax,
        });
        Frame {
            name: child,
            owner,
            test,
            methods: matches!(kind, Kind::Impl | Kind::Trait),
            public_items: kind == Kind::Trait && public,
        }
    }

    fn within(&mut self, frame: Frame, visit: impl FnOnce(&mut Self)) {
        let outer = std::mem::replace(&mut self.scope, frame);
        visit(self);
        self.scope = outer;
    }
}

/// Declares a node and visits its children inside the new scope.
macro_rules! scoped {
    ($self:ident, $node:ident, $walk:expr, $kind:expr, $name:expr, $public:expr, $span:expr) => {{
        let frame = $self.declare($kind, &$name, &$node.attrs, $public, $span, None);
        $self.within(frame, |this| $walk(this, $node));
    }};
}

impl<'ast> Visit<'ast> for Extractor<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let Some((brace, _)) = &node.content else {
            return super::syntax_refs::module_file(self, node);
        };
        let span = between(&[&node.vis, &node.unsafety, &node.mod_token], brace.span.close());
        let walk = visit::visit_item_mod;
        scoped!(self, node, walk, Kind::Module, ident(&node.ident), public(&node.vis), span);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let span = between(&[&node.vis, &node.sig], node.block.brace_token.span.close());
        let walk = visit::visit_item_fn;
        scoped!(self, node, walk, Kind::Function, ident(&node.sig.ident), public(&node.vis), span);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let span = between(&[&node.vis, &node.sig], node.block.brace_token.span.close());
        let walk = visit::visit_impl_item_fn;
        scoped!(self, node, walk, Kind::Function, ident(&node.sig.ident), public(&node.vis), span);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        let end = node.default.as_ref().map(|b| b.brace_token.span.close());
        let end = end.or(node.semi_token.as_ref().map(|s| s.spans[0]));
        let span = between(&[&node.sig], end.unwrap_or_else(Span::call_site));
        let walk = visit::visit_trait_item_fn;
        scoped!(self, node, walk, Kind::Function, ident(&node.sig.ident), false, span);
    }

    fn visit_foreign_item_fn(&mut self, node: &'ast syn::ForeignItemFn) {
        let span = between(&[&node.vis, &node.sig], node.semi_token.spans[0]);
        let walk = visit::visit_foreign_item_fn;
        scoped!(self, node, walk, Kind::Function, ident(&node.sig.ident), public(&node.vis), span);
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        let semi = node.semi_token.as_ref().map(|s| s.spans[0]);
        let end = fields_end(&node.fields, semi).unwrap_or(node.ident.span());
        let span = between(&[&node.vis, &node.struct_token], end);
        let walk = visit::visit_item_struct;
        scoped!(self, node, walk, Kind::Struct, ident(&node.ident), public(&node.vis), span);
    }

    fn visit_item_union(&mut self, node: &'ast syn::ItemUnion) {
        let span = between(&[&node.vis, &node.union_token], node.fields.brace_token.span.close());
        let walk = visit::visit_item_union;
        scoped!(self, node, walk, Kind::Union, ident(&node.ident), public(&node.vis), span);
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        let span = between(&[&node.vis, &node.enum_token], node.brace_token.span.close());
        let walk = visit::visit_item_enum;
        scoped!(self, node, walk, Kind::Enum, ident(&node.ident), public(&node.vis), span);
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        let head: [&dyn quote::ToTokens; 4] =
            [&node.vis, &node.unsafety, &node.modifiers.auto_token, &node.trait_token];
        let span = between(&head, node.brace_token.span.close());
        let walk = visit::visit_item_trait;
        scoped!(self, node, walk, Kind::Trait, ident(&node.ident), public(&node.vis), span);
    }

    fn visit_item_trait_alias(&mut self, node: &'ast syn::ItemTraitAlias) {
        let span = between(&[&node.vis, &node.trait_token], node.semi_token.spans[0]);
        let walk = visit::visit_item_trait_alias;
        scoped!(self, node, walk, Kind::TraitAlias, ident(&node.ident), public(&node.vis), span);
    }

    fn visit_item_type(&mut self, node: &'ast syn::ItemType) {
        let span = between(&[&node.vis, &node.type_token], node.semi_token.spans[0]);
        let walk = visit::visit_item_type;
        scoped!(self, node, walk, Kind::TypeAlias, ident(&node.ident), public(&node.vis), span);
    }

    fn visit_impl_item_type(&mut self, node: &'ast syn::ImplItemType) {
        let span = between(&[&node.vis, &node.type_token], node.semi_token.spans[0]);
        let walk = visit::visit_impl_item_type;
        scoped!(self, node, walk, Kind::TypeAlias, ident(&node.ident), public(&node.vis), span);
    }

    fn visit_trait_item_type(&mut self, node: &'ast syn::TraitItemType) {
        let span = between(&[&node.type_token], node.semi_token.spans[0]);
        let walk = visit::visit_trait_item_type;
        scoped!(self, node, walk, Kind::TypeAlias, ident(&node.ident), false, span);
    }

    fn visit_foreign_item_type(&mut self, node: &'ast syn::ForeignItemType) {
        let span = between(&[&node.vis, &node.type_token], node.semi_token.spans[0]);
        let walk = visit::visit_foreign_item_type;
        scoped!(self, node, walk, Kind::TypeAlias, ident(&node.ident), public(&node.vis), span);
    }

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        let span = between(&[&node.vis, &node.const_token], node.semi_token.spans[0]);
        let walk = visit::visit_item_const;
        scoped!(self, node, walk, Kind::Const, ident(&node.ident), public(&node.vis), span);
    }

    fn visit_impl_item_const(&mut self, node: &'ast syn::ImplItemConst) {
        let span = between(&[&node.vis, &node.const_token], node.semi_token.spans[0]);
        let walk = visit::visit_impl_item_const;
        scoped!(self, node, walk, Kind::Const, ident(&node.ident), public(&node.vis), span);
    }

    fn visit_trait_item_const(&mut self, node: &'ast syn::TraitItemConst) {
        let span = between(&[&node.const_token], node.semi_token.spans[0]);
        let walk = visit::visit_trait_item_const;
        scoped!(self, node, walk, Kind::Const, ident(&node.ident), false, span);
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        let span = between(&[&node.vis, &node.static_token], node.semi_token.spans[0]);
        let walk = visit::visit_item_static;
        scoped!(self, node, walk, Kind::Static, ident(&node.ident), public(&node.vis), span);
    }

    fn visit_foreign_item_static(&mut self, node: &'ast syn::ForeignItemStatic) {
        let span = between(&[&node.vis, &node.static_token], node.semi_token.spans[0]);
        let walk = visit::visit_foreign_item_static;
        scoped!(self, node, walk, Kind::Static, ident(&node.ident), public(&node.vis), span);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let self_type = compact(text(self.source, node_span(&*node.self_ty)));
        let trait_path = node
            .trait_
            .as_ref()
            .map(|(path, _)| compact(text(self.source, node_span(path))));
        let local = match &trait_path {
            Some(t) => format!("{self_type}::impl<{t}>"),
            None => format!("{self_type}::impl"),
        };
        let head: [&dyn quote::ToTokens; 3] =
            [&node.modifiers.defaultness, &node.unsafety, &node.impl_token];
        let span = between(&head, node.brace_token.span.close());
        let implementation = Implementation {
            self_type,
            trait_path,
        };
        let frame = self.declare(Kind::Impl, &local, &node.attrs, false, span, Some(implementation));
        self.within(frame, |this| visit::visit_item_impl(this, node));
    }

    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        let Some(name) = node.ident.as_ref().filter(|_| node.mac.path.is_ident("macro_rules"))
        else {
            return;
        };
        let end = match &node.mac.delimiter {
            syn::MacroDelimiter::Brace(brace) => brace.span.close(),
            syn::MacroDelimiter::Paren(paren) => paren.span.close(),
            syn::MacroDelimiter::Bracket(bracket) => bracket.span.close(),
        };
        let end = node.semi_token.as_ref().map_or(end, |semi| semi.spans[0]);
        let span = first(&[&node.mac.path]).map_or(end, |start| start.join(end).unwrap_or(end));
        let frame = self.declare(Kind::Macro, &ident(name), &node.attrs, false, span, None);
        let body = node.mac.tokens.clone();
        self.within(frame, |this| super::syntax_refs::token_calls(this, body));
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        super::syntax_refs::macro_args(self, node);
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        super::syntax_refs::uses(self, node);
    }

    fn visit_item_extern_crate(&mut self, node: &'ast syn::ItemExternCrate) {
        super::syntax_refs::extern_crate(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        super::syntax_refs::call(self, node);
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        super::syntax_refs::method_call(self, node);
        visit::visit_expr_method_call(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Completion, Confidence, ImportKind, UnresolvedReason};

    const SOURCE: &str = "//! Module doc.\nmod file;\npub use crate::a::{self, b::{C as D}, *};\npub trait Work { const N: u8; type Out; fn run(&self) -> u8; }\nimpl<T> Work for Vec<T> {\n    const N: u8 = 1;\n    type Out = u8;\n    fn run(&self) -> u8 { helper::<T>(); self.len().min(3); 3 }\n}\nunion U { a: u8 }\nextern \"C\" { fn ext(); static EXT: u8; }\nmacro_rules! make { () => { fn phantom() {} } }\nstatic S: u8 = 0;\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn checks() { super::run(); }\n}\n";

    fn facts() -> Extraction {
        extract(SOURCE, &Limits::default()).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn declarations_are_named_scoped_and_labelled() {
        let facts = facts();
        let found: Vec<(Kind, &str, bool)> = facts
            .declarations
            .iter()
            .map(|d| (d.kind, d.name.as_str(), d.public))
            .collect();
        assert_eq!(
            found,
            [
                (Kind::Trait, "Work", true),
                (Kind::Const, "Work::N", true),
                (Kind::TypeAlias, "Work::Out", true),
                (Kind::Method, "Work::run", true),
                (Kind::Impl, "Vec<T>::impl<Work>", false),
                (Kind::Const, "Vec::N", false),
                (Kind::TypeAlias, "Vec::Out", false),
                (Kind::Method, "Vec::run", false),
                (Kind::Union, "U", false),
                (Kind::Function, "ext", false),
                (Kind::Static, "EXT", false),
                (Kind::Macro, "make", false),
                (Kind::Static, "S", false),
                (Kind::Module, "tests", false),
                (Kind::Function, "tests::checks", false),
            ]
        );
        let checks = &facts.declarations[14];
        assert!(checks.test);
        assert_eq!(checks.parent, Some(13));
        assert_eq!(checks.span.lines, [17, 17]);
        assert_eq!(facts.module_doc.as_deref(), Some("Module doc."));
        assert_eq!(facts.completion, Completion::Complete);
        let imp = &facts.declarations[4];
        let self_type = imp.implementation.as_ref().map(|i| i.self_type.as_str());
        assert_eq!(self_type, Some("Vec<T>"));
        assert_eq!(imp.span.lines, [5, 9]);
    }

    #[test]
    fn imports_and_calls_keep_raw_spellings_and_labels() {
        let facts = facts();
        let imports: Vec<(ImportKind, &str, Option<&str>, bool)> = facts
            .imports
            .iter()
            .map(|i| (i.kind, i.path.as_str(), i.alias.as_deref(), i.public))
            .collect();
        assert_eq!(
            imports,
            [
                (ImportKind::ModFile, "file", None, false),
                (ImportKind::Use, "crate::a", None, true),
                (ImportKind::Use, "crate::a::b::C", Some("D"), true),
                (ImportKind::Use, "crate::a::*", None, true),
            ]
        );
        let calls: Vec<(&str, Confidence, bool)> = facts
            .references
            .iter()
            .map(|r| (r.target.as_str(), r.confidence, r.in_test))
            .collect();
        let receiver = Confidence::Unresolved(UnresolvedReason::MethodReceiver);
        assert_eq!(
            calls,
            [
                ("helper", Confidence::Name, false),
                ("self.len().min", receiver, false),
                ("self.len", receiver, false),
                ("super::run", Confidence::Name, true),
            ]
        );
        assert_eq!(facts.references[0].from, Some(7));
    }
}
