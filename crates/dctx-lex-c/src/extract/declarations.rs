//! Top-level dispatch: directives, namespaces, using-declarations and typedefs.
//! Implements SPEC-MAP-MODE 5.3 for `#include` and the C++ namespace / using forms.
//! Type bodies and functions live in sibling modules. Macros are never expanded.
use super::{Item, Region, Scope, Walker};
use crate::domain::{DiagnosticReason, Precision};
use crate::lexer::{classify, define_attribute, define_brace, hidden_include, specifier, Dir, Kind};
use dctx_map_model::{EdgeKind, SymbolKind};

impl Walker<'_> {
    pub(crate) fn walk(&mut self, mut index: usize, end: usize, scope: &Scope) {
        let mut scope = scope.clone();
        while index < end && !self.stopped() {
            if scope.region == Region::Body {
                index = self.scan_step(index, end, &scope);
                continue;
            }
            let next = self.decl_one(index, end, &mut scope);
            index = if next > index { next } else { index + 1 };
        }
    }

    fn decl_one(&mut self, start: usize, end: usize, scope: &mut Scope) -> usize {
        if self.kind(start) == Some(Kind::Directive) {
            return self.directive(start, scope);
        }
        if let Some(next) = self.access(start, scope) {
            return next;
        }
        if self.is(start, ";") {
            return start + 1;
        }
        // A macro whose body is `}` must not end the scope; the partner already
        // bounded the declaration it closes. A real `}` still ends it.
        if self.is(start, "}") && self.kind(start) == Some(Kind::Punct) {
            return end;
        }
        if matches!(self.text(start), "static_assert" | "_Static_assert" | "asm" | "__asm__") {
            let stop = self.consume_semi(start, end);
            self.scan_range(start, stop, scope);
            return stop;
        }
        let at = self.skip_templates(start, end);
        if let Some(next) = self.hidden_macro_type(at, end) {
            return next;
        }
        if self.is(at, "extern") && self.kind(at + 1) == Some(Kind::String) && self.is(at + 2, "{") {
            let open = at + 2;
            let close = self.close(open).unwrap_or(end.saturating_sub(1).max(open));
            self.walk(open + 1, close.min(end), scope);
            return self.after(open).min(end);
        }
        let spec = self.skip_specs(at, end);
        if self.is(at, "namespace") || self.is(spec, "namespace") {
            let keyword = if self.is(at, "namespace") { at } else { spec };
            return self.namespace(start, keyword, end, scope);
        }
        if self.is(at, "using") || self.is(spec, "using") {
            let keyword = if self.is(at, "using") { at } else { spec };
            return self.using_decl(start, keyword, end, scope);
        }
        if self.is(at, "typedef") || self.is(spec, "typedef") {
            let keyword = if self.is(at, "typedef") { at } else { spec };
            let after = self.skip_specs(keyword + 1, end);
            if self.is_intro(after) {
                return self.type_decl(start, after, end, scope, true);
            }
            return self.typedef_alias(start, keyword, end, scope);
        }
        if self.is_intro(spec) {
            if self.saw(start, spec, "friend") {
                return self.friend_decl(start, spec, end, scope);
            }
            return self.type_decl(start, spec, end, scope, false);
        }
        self.func_or_var(start, end, scope)
    }

    pub(crate) fn directive(&mut self, index: usize, scope: &Scope) -> usize {
        let text = self.text(index).to_string();
        let at = self.start_of(index);
        match classify(&text) {
            Dir::Include(path) => {
                if let Some(path) = path {
                    self.refer(scope.owner, scope, EdgeKind::Imports, path, (index, index), Precision::Structural);
                } else if hidden_include(&Dir::Include(None)) {
                    self.problem(at, DiagnosticReason::HiddenByMacro);
                }
            }
            Dir::Define(Some(def)) => {
                self.macros.insert(def.name.clone(), def.function_like);
                match define_brace(&text) {
                    Some(open) => {
                        self.brace_macros.insert(def.name.clone(), open);
                    }
                    None => {
                        self.brace_macros.remove(&def.name);
                    }
                }
                if define_attribute(&text) {
                    self.attr_macros.insert(def.name.clone());
                } else {
                    self.attr_macros.remove(&def.name);
                }
                let item = Item {
                    symbol: Some(SymbolKind::Macro),
                    local: def.name.clone(),
                    first: index,
                    last: index,
                    sig_end: self.end_of(index),
                    body: (self.start_of(index), self.end_of(index)),
                    public: true,
                };
                let _ = self.emit(item, scope);
            }
            Dir::Define(None) => self.problem(at, DiagnosticReason::Malformed),
            Dir::Undef(name) => {
                self.macros.remove(&name);
                self.brace_macros.remove(&name);
                self.attr_macros.remove(&name);
            }
            Dir::Open => self.open_cond(at),
            Dir::Branch => {
                if self.cond == 0 {
                    self.problem(at, DiagnosticReason::UnbalancedConditional);
                }
            }
            Dir::Close => self.close_cond(at),
            Dir::Other => {}
        }
        index + 1
    }

    fn namespace(&mut self, sig: usize, keyword: usize, end: usize, scope: &Scope) -> usize {
        let name_at = self.skip_attributes(keyword + 1, end);
        if self.is(name_at, "{") {
            let close = self.close(name_at).unwrap_or(end.saturating_sub(1).max(name_at));
            let mut inner = scope.clone();
            inner.region = Region::Namespace;
            inner.public = false;
            self.walk(name_at + 1, close.min(end), &inner);
            return self.after(name_at).min(end);
        }
        if !self.can_name(name_at) {
            return self.consume_semi(sig, end);
        }
        if self.known_macro(self.text(name_at)) {
            self.problem(self.start_of(name_at), DiagnosticReason::HiddenByMacro);
            return self.consume_semi(sig, end);
        }
        let (name, after_name) = self.qualified_ident(name_at, end);
        let open_at = self.skip_attributes(after_name, end);
        if self.is(open_at, "=") {
            let stop = self.consume_semi(sig, end);
            self.emit_alias(
                sig,
                name_at,
                stop.saturating_sub(1).max(name_at),
                &name,
                scope,
                "namespace",
            );
            self.emit_types(open_at + 1, stop, usize::MAX, usize::MAX, scope);
            return stop;
        }
        if !self.is(open_at, "{") {
            return self.consume_semi(sig, end);
        }
        let open = open_at;
        let close = self.close(open).unwrap_or(end.saturating_sub(1).max(open));
        let item = Item {
            symbol: None,
            local: name.clone(),
            first: sig,
            last: close.min(self.toks.len().saturating_sub(1)),
            sig_end: self.start_of(open),
            body: (self.start_of(open), self.end_of(close)),
            public: scope.public,
        };
        if let Some(mut inner) = self.emit(item, scope) {
            inner.region = Region::Namespace;
            inner.public = scope.public;
            self.walk(open + 1, close.min(end), &inner);
        }
        (close + 1).min(end)
    }

    fn using_decl(&mut self, sig: usize, keyword: usize, end: usize, scope: &Scope) -> usize {
        let stop = self.consume_semi(sig, end);
        let last = stop.saturating_sub(1).max(keyword);
        if self.is(keyword + 1, "namespace") {
            if let Some(name) = self.qual_name(keyword + 2, stop) {
                self.refer(scope.owner, scope, EdgeKind::Imports, name, (keyword + 2, last), Precision::Structural);
            }
            return stop;
        }
        if self.can_name(keyword + 1) && self.is(keyword + 2, "=") {
            let name = self.text(keyword + 1).to_string();
            if !self.known_macro(&name) {
                self.emit_alias(sig, keyword + 1, last, &name, scope, "using");
                self.emit_types(keyword + 3, stop, usize::MAX, usize::MAX, scope);
            } else {
                self.problem(self.start_of(keyword + 1), DiagnosticReason::HiddenByMacro);
            }
            return stop;
        }
        if let Some(name) = self.qual_name(keyword + 1, stop) {
            self.refer(scope.owner, scope, EdgeKind::Imports, name, (keyword + 1, last), Precision::Structural);
        }
        stop
    }

    fn typedef_alias(&mut self, sig: usize, keyword: usize, end: usize, scope: &Scope) -> usize {
        let stop = self.consume_semi(sig, end);
        let names = self.decl_names(keyword + 1, stop);
        if names.is_empty() {
            self.note_skipped(sig, stop);
            return stop;
        }
        for (at, bare) in names {
            if self.known_macro(&bare) {
                self.problem(self.start_of(at), DiagnosticReason::HiddenByMacro);
                continue;
            }
            let item = Item {
                symbol: Some(SymbolKind::Type),
                local: bare.clone(),
                first: sig,
                last: stop.saturating_sub(1).max(sig),
                sig_end: self.start_of(stop.saturating_sub(1).max(sig)),
                body: (self.start_of(sig), self.end_of(stop.saturating_sub(1).max(sig))),
                public: self.decl_public(sig, at, scope),
            };
            let _ = self.emit(item, scope);
        }
        self.emit_types(keyword + 1, stop, usize::MAX, usize::MAX, scope);
        stop
    }

    fn friend_decl(&mut self, sig: usize, intro: usize, end: usize, scope: &Scope) -> usize {
        let stop = self.consume_semi(sig, end);
        self.emit_types(intro, stop, usize::MAX, usize::MAX, scope);
        stop
    }

    fn emit_alias(&mut self, sig: usize, _name_at: usize, last: usize, name: &str, scope: &Scope, kind: &str) {
        let item = Item {
            symbol: if kind == "using" { Some(SymbolKind::Type) } else { None },
            local: name.to_string(),
            first: sig,
            last,
            sig_end: self.end_of(last),
            body: (self.start_of(sig), self.end_of(last)),
            public: scope.public,
        };
        let _ = self.emit(item, scope);
    }

    fn access(&mut self, index: usize, scope: &mut Scope) -> Option<usize> {
        if !matches!(self.text(index), "public" | "private" | "protected") || !self.is(index + 1, ":") {
            return None;
        }
        scope.public = self.is(index, "public");
        Some(index + 2)
    }

    /// A macro standing in for `struct`/`class` hides the brace that follows.
    ///
    /// An attribute macro (`__attribute__`, `[[...]]`, empty) is not that stand-in.
    fn hidden_macro_type(&mut self, index: usize, end: usize) -> Option<usize> {
        if !(self.ident(index) && self.known_macro(self.text(index))) || self.attribute_macro(index)
        {
            return None;
        }
        let mut next = index + 1;
        if self.is(next, "(") {
            next = self.after(next);
        }
        if self.can_name(next) {
            next += 1;
        }
        if self.is(next, "<") {
            next = self.skip_angles(next, end).unwrap_or(next);
        }
        if self.is(next, ":") {
            next += 1;
            while next < end && !self.is(next, "{") && !self.is(next, ";") {
                if self.is(next, "(") || self.is(next, "[") || self.is(next, "{") {
                    next = self.after(next);
                } else {
                    next += 1;
                }
            }
        }
        if !self.is(next, "{") {
            return None;
        }
        self.problem(self.start_of(index), DiagnosticReason::HiddenByMacro);
        Some(self.consume_semi(index, end))
    }

    pub(crate) fn skip_templates(&mut self, mut index: usize, end: usize) -> usize {
        let mut guard = 0;
        while self.is(index, "template") && index < end && guard < 8 {
            guard += 1;
            let Some(next) = self.skip_angles(index + 1, end) else {
                return (index + 1).min(end);
            };
            if next <= index {
                return index + 1;
            }
            index = next;
        }
        index
    }

    pub(crate) fn skip_specs(&self, mut index: usize, end: usize) -> usize {
        let mut guard = 0;
        while index < end && guard <= end {
            guard += 1;
            if self.is(index, "[") && self.is(index + 1, "[") {
                index = self.after(index).min(end);
                continue;
            }
            if self.ident(index) && matches!(self.text(index), "alignas" | "_Alignas" | "__attribute__" | "__declspec")
            {
                index += 1;
                if self.is(index, "(") {
                    index = self.after(index);
                }
                continue;
            }
            if self.is(index, "extern") {
                index += 1;
                if self.kind(index) == Some(Kind::String) {
                    index += 1;
                }
                continue;
            }
            if self.ident(index) && (specifier(self.text(index)) || self.known_macro(self.text(index))) {
                index += 1;
                if self.is(index, "(") {
                    index = self.after(index);
                }
                continue;
            }
            break;
        }
        index
    }

    pub(crate) fn is_intro(&self, index: usize) -> bool {
        matches!(self.text(index), "struct" | "class" | "union" | "enum")
    }

    pub(crate) fn can_name(&self, index: usize) -> bool {
        self.ident(index)
            && !matches!(self.text(index), "{" | "}")
            && !crate::lexer::keyword(self.text(index))
    }

    /// `[[...]]`, `__attribute__`, `__declspec`, `alignas`, and attribute macros.
    pub(crate) fn starts_attribute(&self, index: usize) -> bool {
        (self.is(index, "[") && self.is(index + 1, "["))
            || (self.ident(index)
                && matches!(
                    self.text(index),
                    "__attribute__" | "__declspec" | "alignas" | "_Alignas"
                ))
            || self.attribute_macro(index)
    }

    pub(crate) fn skip_attributes(&self, mut index: usize, end: usize) -> usize {
        let mut guard = 0;
        while index < end && guard < end && self.starts_attribute(index) {
            guard += 1;
            if self.is(index, "[") && self.is(index + 1, "[") {
                let next = self.after(index);
                index = if next > index { next } else { index + 2 };
                continue;
            }
            index += 1;
            if self.is(index, "(") {
                let next = self.after(index);
                index = if next > index { next } else { index + 1 };
            }
        }
        index
    }

    pub(crate) fn qualified_ident(&self, start: usize, end: usize) -> (String, usize) {
        let mut name = self.text(start).to_string();
        let mut index = start + 1;
        while index < end && self.is(index, "::") {
            let mut next = index + 1;
            if self.is(next, "inline") {
                next += 1;
            }
            if !self.can_name(next) {
                break;
            }
            name.push_str("::");
            name.push_str(self.text(next));
            index = next + 1;
        }
        (name, index)
    }

    pub(crate) fn consume_semi(&self, start: usize, end: usize) -> usize {
        let mut index = start;
        let mut guard = 0;
        while index < end && guard <= end {
            guard += 1;
            if self.is(index, ";") {
                return (index + 1).min(end);
            }
            if self.is(index, "}") {
                return index;
            }
            if self.is(index, "{") || self.is(index, "(") || self.is(index, "[") {
                let next = self.after(index);
                index = if next > index { next.min(end) } else { index + 1 };
                continue;
            }
            index += 1;
        }
        end
    }

    pub(crate) fn saw(&self, start: usize, end: usize, word: &str) -> bool {
        (start..end.min(self.toks.len())).any(|index| self.is(index, word))
    }

    /// Skips a span that is not a declaration.
    ///
    /// No diagnostic: a diagnostic refuses the whole file downstream, and real
    /// sources contain macros and forms this walker does not model. Hidden names
    /// and macro-introduced types still diagnose at their own sites.
    pub(crate) fn note_skipped(&mut self, _start: usize, _end: usize) {}

    pub(crate) fn decl_public(&self, start: usize, name: usize, scope: &Scope) -> bool {
        if matches!(scope.region, Region::File | Region::Namespace) {
            scope.public && !self.saw(start, name, "static")
        } else {
            scope.public
        }
    }

    pub(crate) fn scan_range(&mut self, start: usize, end: usize, scope: &Scope) {
        let mut inner = scope.clone();
        inner.region = Region::Body;
        self.walk(start, end.min(self.toks.len()), &inner);
    }
}

#[cfg(test)]
mod tests {
    use crate::{CLexer, Completion, Precision};
    use dctx_map_model::EdgeKind;

    #[test]
    fn includes_and_macros_are_facts_and_hidden_paths_abstain() {
        let facts = CLexer::default()
            .extract("a.h", b"#include <stdio.h>\n#include NAME\n#define LIMIT 1\n")
            .unwrap();
        assert_eq!(facts.completion, Completion::Partial);
        let imports: Vec<_> = facts
            .references
            .iter()
            .filter(|r| r.fact.kind == EdgeKind::Imports)
            .map(|r| r.fact.name.as_str())
            .collect();
        assert_eq!(imports, ["stdio.h"]);
        assert!(facts.nodes.iter().any(|n| n.fact.name == "LIMIT"));
        assert!(facts.diagnostics.iter().any(|d| d.reason == crate::DiagnosticReason::HiddenByMacro));
        assert!(facts.nodes.iter().any(|n| n.precision == Precision::Structural));
    }
}
