// SPDX-License-Identifier: Apache-2.0
//! Type members: fields, compact constructors and member-shape classification.
//! Implements SPEC-MAP-MODE 5.1 field and compact-constructor facts inside a Java type body.
//! Depends on modifier visibility and the method finisher; sees through generic arguments.
//! Called from class and enum bodies.

use super::declarations_modifiers::{MethodShape, is_visible};
use super::{Prefix, Scope, SymbolDraft, Walker, is_modifier_word};
use crate::domain::Precision;
use dctx_map_model::{NodeKind, SymbolKind};

enum Shape {
    Method { name: usize, paren: usize },
    Field { name: usize },
    Ctor { name: usize, paren: usize },
    Compact { name: usize, brace: usize },
    Unknown,
}

struct FieldAt {
    start: usize,
    type_from: usize,
    type_to: usize,
    name: usize,
}

impl Walker<'_> {
    pub(super) fn member(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        if index >= end || !self.ok() {
            return end;
        }
        if self.is_punct(index, ";") {
            return index + 1;
        }
        let prefix = self.prefix(index, end);
        let at = prefix.next;
        if at >= end {
            return end;
        }
        if self.is_punct(at, "{") {
            return self.scan_group(at, end, scope);
        }
        if let Some(kind) = self.decl_kind(at)
            && self.word(Self::name_at(at, kind))
        {
            return self.type_decl(index, &prefix, kind, end, scope);
        }
        let mut cursor = at;
        let mut own_params = Vec::new();
        let mut bounds = Vec::new();
        if self.is_punct(cursor, "<") {
            let (next, names, found) = self.type_params(cursor, end);
            own_params = names;
            bounds = found;
            cursor = next;
        }
        if !self.ok() {
            return end;
        }
        match self.shape(cursor, end, &scope.simple) {
            Shape::Method { name, paren } => self.finish_method(
                MethodShape {
                    start: index,
                    type_from: cursor,
                    name,
                    paren,
                    constructor: false,
                    params: own_params,
                    bounds,
                },
                end,
                scope,
                &prefix,
            ),
            Shape::Ctor { name, paren } => self.finish_method(
                MethodShape {
                    start: index,
                    type_from: name,
                    name,
                    paren,
                    constructor: true,
                    params: own_params,
                    bounds,
                },
                end,
                scope,
                &prefix,
            ),
            Shape::Field { name } => self.finish_field(
                &FieldAt {
                    start: index,
                    type_from: cursor,
                    type_to: name,
                    name,
                },
                end,
                scope,
                &prefix,
                &own_params,
            ),
            Shape::Compact { name, brace } => {
                self.finish_compact(index, name, brace, end, scope, &prefix)
            }
            Shape::Unknown => self.recover(index, end),
        }
    }

    fn finish_field(
        &mut self,
        at: &FieldAt,
        end: usize,
        scope: &Scope,
        prefix: &Prefix,
        extra: &[String],
    ) -> usize {
        let mut cursor = self.one_field(at, at.name, end, scope, prefix, extra);
        while self.is_punct(cursor, ",") && self.ok() {
            cursor += 1;
            cursor = self.skip_declarator_mods(cursor, end);
            if !self.word(cursor) {
                break;
            }
            let next_name = cursor;
            cursor = self.one_field(at, next_name, end, scope, prefix, extra);
        }
        let done = if self.is_punct(cursor, ";") {
            cursor + 1
        } else {
            cursor
        };
        if done > at.start { done } else { at.start + 1 }
    }

    fn skip_declarator_mods(&self, mut cursor: usize, end: usize) -> usize {
        while cursor < end
            && (self.is_annotation(cursor)
                || (self.word(cursor) && is_modifier_word(self.text(cursor))))
        {
            if self.is_annotation(cursor) {
                let next = self.skip_annotation_tokens(cursor, end);
                cursor = if next > cursor { next } else { cursor + 1 };
            } else {
                cursor += 1;
            }
        }
        cursor
    }

    fn one_field(
        &mut self,
        at: &FieldAt,
        name: usize,
        end: usize,
        scope: &Scope,
        prefix: &Prefix,
        extra: &[String],
    ) -> usize {
        let after = self.after_brackets(name + 1, end);
        let last_sig = after.saturating_sub(1).max(name);
        let local = self.text(name).to_string();
        let sig_end = self.end_byte(last_sig);
        let child = self.emit(
            SymbolDraft {
                node_kind: NodeKind::Symbol,
                symbol: SymbolKind::Const,
                local: &local,
                simple: scope.simple.as_str(),
                scope_kind: scope.kind,
                type_params: Vec::new(),
                first: at.start,
                last: last_sig,
                sig_end,
                body: (self.start_byte(at.start), sig_end),
                is_public: is_visible(scope, prefix, false),
                precision: Precision::Structural,
            },
            scope,
        );
        let mut cursor = after;
        if let Some(child) = child {
            self.type_uses_skip(at.type_from, at.type_to, &child, extra);
            self.scan_annotation_list(prefix, &child);
            if self.is_punct(cursor, "=") {
                let eq = cursor;
                cursor = self.scan_initializer(eq + 1, end, &child);
                let text = self
                    .src_span(self.end_byte(eq), self.start_byte(cursor))
                    .to_string();
                self.draft.set_fingerprint(child.owner, &text);
            }
        } else if self.is_punct(cursor, "=") {
            cursor = self.skip_balance(cursor + 1, end);
        }
        cursor
    }

    fn skip_balance(&self, mut index: usize, end: usize) -> usize {
        while index < end {
            if self.is_punct(index, ",") || self.is_punct(index, ";") {
                return index;
            }
            if (self.is_punct(index, "(") || self.is_punct(index, "[") || self.is_punct(index, "{"))
                && let Some(close) = self.close(index)
            {
                index = close + 1;
                continue;
            }
            index += 1;
        }
        index
    }

    fn finish_compact(
        &mut self,
        start: usize,
        name: usize,
        brace: usize,
        end: usize,
        scope: &Scope,
        prefix: &Prefix,
    ) -> usize {
        let close = self.close(brace).unwrap_or(end.saturating_sub(1));
        let local = self.text(name).to_string();
        if let Some(child) = self.emit(
            SymbolDraft {
                node_kind: NodeKind::Symbol,
                symbol: SymbolKind::Method,
                local: &local,
                simple: scope.simple.as_str(),
                scope_kind: scope.kind,
                type_params: Vec::new(),
                first: start,
                last: close.min(end.saturating_sub(1)),
                sig_end: self.start_byte(brace),
                body: (self.start_byte(brace), self.end_byte(close)),
                is_public: is_visible(scope, prefix, false),
                precision: Precision::Structural,
            },
            scope,
        ) {
            self.scan_annotation_list(prefix, &child);
            self.scan_block(brace + 1, close.min(end), &child);
        }
        (close + 1).min(end)
    }

    fn after_brackets(&self, mut index: usize, end: usize) -> usize {
        while index < end && self.is_punct(index, "[") {
            match self.close(index) {
                Some(close) => index = close + 1,
                None => break,
            }
        }
        index
    }

    fn shape(&mut self, start: usize, end: usize, ctor: &str) -> Shape {
        let mut cursor = start;
        let mut angle = 0i32;
        let mut top = Vec::new();
        let mut prev_dot = false;
        while cursor < end {
            if angle == 0
                && let Some(shape) = self.shape_stop(cursor, &top, ctor)
            {
                return shape;
            }
            if self.is_punct(cursor, "<") {
                angle += 1;
                if angle > i32::try_from(self.max_depth).unwrap_or(i32::MAX) {
                    self.refuse_depth(cursor);
                    return Shape::Unknown;
                }
                prev_dot = false;
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, ">") && angle > 0 {
                angle -= 1;
                prev_dot = false;
                cursor += 1;
                continue;
            }
            if angle > 0 {
                cursor = self.skip_nested(cursor, end);
                continue;
            }
            if self.is_punct(cursor, "[") {
                match self.close(cursor) {
                    Some(close) if close < end => {
                        cursor = close + 1;
                        prev_dot = false;
                        continue;
                    }
                    _ => return Shape::Unknown,
                }
            }
            if self.is_punct(cursor, ".") {
                prev_dot = true;
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, "...") {
                prev_dot = false;
                cursor += 1;
                continue;
            }
            if self.is_annotation(cursor) {
                let next = self.skip_annotation_tokens(cursor, end);
                cursor = if next > cursor { next } else { cursor + 1 };
                prev_dot = false;
                continue;
            }
            if self.word(cursor) {
                if matches!(self.text(cursor), "extends" | "super" | "throws") {
                    prev_dot = false;
                    cursor += 1;
                    continue;
                }
                if prev_dot {
                    if let Some(last) = top.last_mut() {
                        *last = cursor;
                    } else {
                        top.push(cursor);
                    }
                } else {
                    top.push(cursor);
                }
                prev_dot = false;
                cursor += 1;
                continue;
            }
            prev_dot = false;
            cursor += 1;
        }
        Shape::Unknown
    }

    fn shape_stop(&self, index: usize, top: &[usize], ctor: &str) -> Option<Shape> {
        if self.is_punct(index, "(") {
            let name = *top.last()?;
            if top.len() == 1 && !ctor.is_empty() && self.text(name) == ctor {
                return Some(Shape::Ctor { name, paren: index });
            }
            if top.len() >= 2 {
                return Some(Shape::Method { name, paren: index });
            }
            return Some(Shape::Unknown);
        }
        if self.is_punct(index, "{") {
            if top.len() == 1 && !ctor.is_empty() && self.text(top[0]) == ctor {
                return Some(Shape::Compact {
                    name: top[0],
                    brace: index,
                });
            }
            return Some(Shape::Unknown);
        }
        if self.is_punct(index, ";") || self.is_punct(index, "=") || self.is_punct(index, ",") {
            if top.len() >= 2 {
                return Some(Shape::Field {
                    name: top[top.len() - 1],
                });
            }
            return Some(Shape::Unknown);
        }
        None
    }

    fn skip_nested(&self, index: usize, end: usize) -> usize {
        if (self.is_punct(index, "(") || self.is_punct(index, "[") || self.is_punct(index, "{"))
            && let Some(close) = self.close(index)
        {
            return (close + 1).min(end);
        }
        index + 1
    }
}
