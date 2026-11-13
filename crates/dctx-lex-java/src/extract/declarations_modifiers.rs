// SPDX-License-Identifier: Apache-2.0
//! Modifiers, annotations, test methods and method completion.
//! Implements SPEC-MAP-MODE 5.1 visibility and `JUnit` `@Test` method facts.
//! Depends on generic declarators for parameters and thrown types.
//! Called when a prefix is scanned or a member shape is a method or constructor.

use super::{Ann, Prefix, Scope, ScopeKind, SymbolDraft, Walker, is_modifier_word};
use crate::domain::Precision;
use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};

pub(super) struct MethodShape {
    pub(super) start: usize,
    pub(super) type_from: usize,
    pub(super) name: usize,
    pub(super) paren: usize,
    pub(super) constructor: bool,
    pub(super) params: Vec<String>,
    pub(super) bounds: Vec<(usize, usize)>,
}

struct MethodClose {
    brace: Option<usize>,
    last: usize,
}

impl MethodClose {
    fn body(&self, walker: &Walker<'_>, start: usize, sig_end: u32) -> (u32, u32) {
        if let Some(open) = self.brace {
            let close = walker.close(open).unwrap_or(self.last);
            (walker.start_byte(open), walker.end_byte(close))
        } else {
            (walker.start_byte(start), sig_end)
        }
    }
}

impl Walker<'_> {
    pub(crate) fn is_annotation(&self, index: usize) -> bool {
        self.is_punct(index, "@") && !self.is_word(index + 1, "interface")
    }

    pub(crate) fn annotation_name(&self, index: usize) -> String {
        let mut cursor = index + 1;
        if !self.word(cursor) {
            return String::new();
        }
        let mut name = self.text(cursor).to_string();
        cursor += 1;
        while self.is_punct(cursor, ".") && self.word(cursor + 1) {
            name.push('.');
            name.push_str(self.text(cursor + 1));
            cursor += 2;
        }
        name
    }

    pub(crate) fn skip_annotation_tokens(&self, index: usize, end: usize) -> usize {
        if !self.is_punct(index, "@") {
            return index;
        }
        let mut cursor = index + 1;
        if cursor < end && self.word(cursor) {
            cursor += 1;
            while cursor + 1 < end && self.is_punct(cursor, ".") && self.word(cursor + 1) {
                cursor += 2;
            }
        }
        if cursor < end
            && self.is_punct(cursor, "(")
            && let Some(close) = self.close(cursor)
        {
            return (close + 1).min(end);
        }
        cursor.min(end)
    }

    pub(crate) fn prefix(&self, mut index: usize, end: usize) -> Prefix {
        let mut annotations = Vec::new();
        let mut public = false;
        let mut private = false;
        while index < end {
            if self.is_annotation(index) {
                let args = self
                    .is_punct(self.annotation_args(index), "(")
                    .then(|| self.annotation_args(index));
                annotations.push(Ann {
                    name: self.annotation_name(index),
                    at: index,
                    args,
                });
                let next = self.skip_annotation_tokens(index, end);
                index = if next > index { next } else { index + 1 };
                continue;
            }
            if self.is_word(index, "non")
                && self.is_punct(index + 1, "-")
                && self.is_word(index + 2, "sealed")
            {
                index += 3;
                continue;
            }
            if self.word(index) && is_modifier_word(self.text(index)) {
                public |= self.text(index) == "public";
                private |= self.text(index) == "private";
                index += 1;
                continue;
            }
            break;
        }
        Prefix {
            next: index,
            public,
            private,
            annotations,
        }
    }

    fn annotation_args(&self, index: usize) -> usize {
        let mut cursor = index + 1;
        if self.word(cursor) {
            cursor += 1;
            while self.is_punct(cursor, ".") && self.word(cursor + 1) {
                cursor += 2;
            }
        }
        cursor
    }

    pub(super) fn finish_method(
        &mut self,
        shape: MethodShape,
        end: usize,
        scope: &Scope,
        prefix: &Prefix,
    ) -> usize {
        let MethodShape {
            start,
            type_from,
            name,
            paren,
            constructor,
            params,
            bounds,
        } = shape;
        let close = self.close(paren);
        let (cursor, throws_from) = self.after_throws(close, paren, end);
        let local = self.text(name).to_string();
        let simple = if constructor {
            local.clone()
        } else {
            scope.simple.clone()
        };
        let (node_kind, precision) = method_kind(prefix);
        let span = self.method_close(cursor, end);
        let sig_end = self.start_byte(cursor.min(end));
        let body = span.body(self, start, sig_end);
        let Some(child) = self.emit(
            SymbolDraft {
                node_kind,
                symbol: SymbolKind::Method,
                local: &local,
                simple: &simple,
                scope_kind: scope.kind,
                type_params: params,
                first: start,
                last: span.last.max(start),
                sig_end,
                body,
                is_public: is_visible(scope, prefix, false),
                precision,
            },
            scope,
        ) else {
            return self.recover(start, end);
        };
        self.scan_annotation_list(prefix, &child);
        for (from, to) in bounds {
            self.type_uses(from, to, &child);
        }
        if !constructor {
            self.type_uses(type_from, name, &child);
        }
        if let Some(close) = close {
            self.use_parameters(paren, close, &child);
        }
        if let Some(from) = throws_from {
            self.type_uses(from, cursor, &child);
        }
        self.method_body(cursor, end, &child)
    }

    fn after_throws(
        &mut self,
        close: Option<usize>,
        paren: usize,
        end: usize,
    ) -> (usize, Option<usize>) {
        let after_params = close.map_or(paren + 1, |index| index + 1);
        let mut cursor = self.after_brackets(after_params, end);
        if !self.is_word(cursor, "throws") {
            return (cursor.min(end), None);
        }
        let from = (cursor + 1).min(end);
        cursor = from;
        while cursor < end && !self.signature_end(cursor) {
            let next = self.skip_one_type(cursor, end);
            cursor = if next > cursor { next } else { cursor + 1 };
            if self.is_punct(cursor, ",") {
                cursor += 1;
            }
        }
        (cursor, Some(from))
    }

    fn signature_end(&self, index: usize) -> bool {
        self.is_punct(index, "{") || self.is_punct(index, ";") || self.is_word(index, "default")
    }

    fn method_close(&self, cursor: usize, end: usize) -> MethodClose {
        if self.is_punct(cursor, "{") {
            let last = self
                .close(cursor)
                .unwrap_or(end.saturating_sub(1))
                .min(end.saturating_sub(1));
            return MethodClose {
                brace: Some(cursor),
                last,
            };
        }
        if self.is_word(cursor, "default") {
            return MethodClose {
                brace: None,
                last: self.find_semi(cursor + 1, end),
            };
        }
        if self.is_punct(cursor, ";") {
            return MethodClose {
                brace: None,
                last: cursor,
            };
        }
        MethodClose {
            brace: None,
            last: cursor.saturating_sub(1).min(end.saturating_sub(1)),
        }
    }

    fn use_parameters(&mut self, open: usize, close: usize, scope: &Scope) {
        for declarator in self.declarators(open + 1, close) {
            self.type_uses(declarator.type_from, declarator.type_to, scope);
            for annotation in declarator.annotations {
                self.refer(
                    scope.owner,
                    scope,
                    EdgeKind::UsesType,
                    annotation,
                    None,
                    declarator.name,
                    declarator.name,
                    Precision::Structural,
                );
            }
        }
    }

    fn method_body(&mut self, cursor: usize, end: usize, scope: &Scope) -> usize {
        if self.is_punct(cursor, "{") {
            return self.scan_group(cursor, end, scope);
        }
        if self.is_word(cursor, "default") {
            let stop = self.scan_initializer(cursor + 1, end, scope);
            return if self.is_punct(stop, ";") {
                stop + 1
            } else {
                stop.max(cursor + 1)
            };
        }
        (cursor + 1).min(end)
    }

    fn find_semi(&self, mut index: usize, end: usize) -> usize {
        let mut angle = 0i32;
        while index < end {
            if self.is_punct(index, "<") {
                angle += 1;
                index += 1;
                continue;
            }
            if self.is_punct(index, ">") && angle > 0 {
                angle -= 1;
                index += 1;
                continue;
            }
            if angle == 0
                && (self.is_punct(index, "(")
                    || self.is_punct(index, "{")
                    || self.is_punct(index, "["))
                && let Some(close) = self.close(index)
            {
                index = close + 1;
                continue;
            }
            if angle == 0 && self.is_punct(index, ";") {
                return index;
            }
            index += 1;
        }
        end.saturating_sub(1)
    }
}

pub(super) fn is_visible(scope: &Scope, prefix: &Prefix, enum_const: bool) -> bool {
    if enum_const || prefix.public {
        return true;
    }
    if prefix.private {
        return false;
    }
    matches!(scope.kind, ScopeKind::Interface)
}

fn is_test_name(name: &str) -> bool {
    name == "Test" || name.ends_with(".Test")
}

fn method_kind(prefix: &Prefix) -> (NodeKind, Precision) {
    if prefix.annotations.iter().any(|ann| is_test_name(&ann.name)) {
        (NodeKind::Test, Precision::Convention)
    } else {
        (NodeKind::Symbol, Precision::Structural)
    }
}

#[cfg(test)]
mod tests {
    use crate::{JavaLexer, Precision};

    #[test]
    fn junit_annotation_marks_a_test_and_a_tests_edge() {
        let source = "class A { @Test void saves() { repo.save(1); } }";
        let facts = JavaLexer::default()
            .extract("A.java", source.as_bytes())
            .expect("extract");
        let test = facts
            .nodes
            .iter()
            .find(|node| node.fact.name == "A::saves")
            .expect("saves");
        assert_eq!(test.fact.kind.as_str(), "test");
        assert_eq!(
            test.fact
                .symbol_kind
                .map(dctx_map_model::SymbolKind::as_str),
            Some("method")
        );
        assert_eq!(test.precision, Precision::Convention);
        let calls: Vec<_> = facts
            .references
            .iter()
            .filter(|reference| reference.fact.kind.as_str() == "calls")
            .map(|reference| (reference.fact.name.as_str(), reference.precision))
            .collect();
        let tests: Vec<_> = facts
            .references
            .iter()
            .filter(|reference| reference.fact.kind.as_str() == "tests")
            .map(|reference| (reference.fact.name.as_str(), reference.precision))
            .collect();
        assert_eq!(calls, [("repo.save", Precision::Structural)]);
        assert_eq!(tests, [("repo.save", Precision::Convention)]);
    }
}
