// SPDX-License-Identifier: Apache-2.0
//! Type grammar: modifiers, attributes, generics, base lists and type references.
//! Implements SPEC-MAP-MODE 5.3 `uses-type` and `implements` for C# declarations.
//! Members and type declarations call these helpers while they walk a span.
use super::{DeclKind, Prefix, Scope, ScopeKind, Walker};
use crate::lexer::modifier;
use dctx_map_model::EdgeKind;

const TARGETS: &[&str] = &[
    "assembly", "module", "return", "param", "field", "method", "type", "property", "event",
    "typevar",
];

impl Walker<'_> {
    pub(crate) fn decl_kind(&self, index: usize) -> Option<DeclKind> {
        Some(match self.text(index) {
            "class" => DeclKind::Class,
            "struct" => DeclKind::Struct,
            "interface" => DeclKind::Interface,
            "enum" => DeclKind::Enum,
            "record" => DeclKind::Record,
            _ => return None,
        })
    }

    pub(crate) fn prefix(&self, mut index: usize, end: usize) -> Prefix {
        let mut public = false;
        let mut private = false;
        let mut attrs = Vec::new();
        while index < end {
            if self.is(index, "[") {
                if let Some(close) = self.partner_within(index, end) {
                    attrs.extend(self.attr_names(index, close));
                    index = close + 1;
                } else {
                    index += 1;
                }
                continue;
            }
            if self.word(index) && modifier(self.text(index)) {
                public |= self.is(index, "public");
                private |= self.is(index, "private");
                index += 1;
                continue;
            }
            break;
        }
        Prefix {
            next: index,
            public,
            private,
            attrs,
        }
    }

    fn attr_names(&self, open: usize, close: usize) -> Vec<(usize, usize)> {
        let mut names = Vec::new();
        let mut index = open + 1;
        while index < close {
            if self.word(index) && self.is(index + 1, ":") && TARGETS.contains(&self.text(index)) {
                index += 2;
            }
            if let Some(end) = self.dotted_end(index, close) {
                names.push((index, end));
                index = end;
                if self.is(index, "(") {
                    index = self.after(index, close + 1).min(close);
                }
                if self.is(index, ",") {
                    index += 1;
                }
            } else {
                index += 1;
            }
        }
        names
    }

    pub(crate) fn attr_uses(&mut self, prefix: &Prefix, scope: &Scope) {
        for &(start, end) in &prefix.attrs {
            let last = end.saturating_sub(1);
            let name = self.span_text(start, last);
            self.refer(
                scope.owner,
                &scope.name,
                EdgeKind::UsesType,
                name,
                None,
                start,
                last,
            );
        }
    }

    /// Exclusive end of a dotted or `::` name, if `index` begins one.
    pub(crate) fn dotted_end(&self, index: usize, end: usize) -> Option<usize> {
        if !self.word(index) {
            return None;
        }
        let mut cursor = index + 1;
        while cursor < end
            && (self.is(cursor, ".") || self.is(cursor, "::"))
            && self.word(cursor + 1)
        {
            cursor += 2;
        }
        Some(cursor)
    }

    pub(crate) fn parse_type_end(&self, index: usize, end: usize) -> Option<usize> {
        if index >= end {
            return None;
        }
        let mut cursor = if self.is(index, "(") {
            self.partner_within(index, end)? + 1
        } else if self.type_word(index) {
            let mut cursor = self.dotted_end(index, end)?;
            if self.is(cursor, "<") {
                cursor = self.close_angle(cursor, end)? + 1;
            }
            cursor
        } else {
            return None;
        };
        loop {
            if self.is(cursor, "?") {
                cursor += 1;
            } else if self.is(cursor, "[") {
                cursor = self.partner_within(cursor, end)? + 1;
            } else if self.is(cursor, "*") {
                cursor += 1;
            } else {
                break;
            }
        }
        Some(cursor)
    }

    pub(crate) fn close_angle(&self, open: usize, end: usize) -> Option<usize> {
        let mut depth = 0i32;
        let mut index = open;
        while index < end {
            if self.is(index, "<") {
                depth += 1;
            } else if self.is(index, ">") {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            } else if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                index = self.after(index, end);
                continue;
            } else if self.is(index, ";") {
                return None;
            }
            index += 1;
        }
        None
    }

    pub(crate) fn type_param_names(&self, open: usize, close: usize) -> Vec<String> {
        let mut names = Vec::new();
        let mut index = open + 1;
        while index < close {
            if self.can_name(index) {
                names.push(self.text(index).to_string());
            }
            index += 1;
        }
        names
    }

    pub(crate) fn uses_from(
        &mut self,
        mut index: usize,
        end: usize,
        scope: &Scope,
        skip: &[String],
    ) {
        while index < end {
            if !self.type_word(index) || constraint(self.text(index)) {
                index += 1;
                continue;
            }
            let Some(name_end) = self.dotted_end(index, end) else {
                index += 1;
                continue;
            };
            let last = name_end.saturating_sub(1);
            let name = self.span_text(index, last);
            let skipped = !name.contains('.') && skip.iter().any(|item| item == &name);
            if !skipped && !name.is_empty() {
                self.refer(
                    scope.owner,
                    &scope.name,
                    EdgeKind::UsesType,
                    name,
                    None,
                    index,
                    last,
                );
            }
            index = name_end;
            if self.is(index, "<")
                && let Some(close) = self.close_angle(index, end)
            {
                self.uses_from(index + 1, close, scope, skip);
                index = close + 1;
            }
        }
    }

    pub(crate) fn param_types(&mut self, open: usize, scope: &Scope, skip: &[String]) {
        let Some(close) = self.partner_within(open, self.toks.len()) else {
            return;
        };
        for (start, end) in self.split_commas(open + 1, close) {
            let cursor = self.skip_param_prefix(start, end);
            if let Some(after) = self.parse_type_end(cursor, end) {
                self.uses_from(cursor, after, scope, skip);
            }
        }
    }

    pub(crate) fn skip_param_prefix(&self, mut index: usize, end: usize) -> usize {
        while index < end && (self.is(index, "[") || self.param_mod(index)) {
            index = if self.is(index, "[") {
                self.after(index, end)
            } else {
                index + 1
            };
        }
        index
    }

    fn param_mod(&self, index: usize) -> bool {
        self.word(index)
            && (modifier(self.text(index))
                || matches!(
                    self.text(index),
                    "params" | "ref" | "out" | "in" | "this" | "scoped"
                ))
    }

    pub(crate) fn split_commas(&self, start: usize, end: usize) -> Vec<(usize, usize)> {
        let mut parts = Vec::new();
        let mut index = start;
        let mut part = start;
        let mut angle = 0i32;
        while index < end {
            if self.is(index, "<") {
                angle += 1;
            } else if self.is(index, ">") {
                angle -= 1;
            } else if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                index = self.after(index, end);
                continue;
            } else if self.is(index, ",") && angle == 0 {
                parts.push((part, index));
                part = index + 1;
            }
            index += 1;
        }
        if part < end {
            parts.push((part, end));
        }
        parts
    }

    pub(crate) fn base_list(
        &mut self,
        start: usize,
        end: usize,
        scope: &Scope,
        kind: DeclKind,
        record_struct: bool,
    ) {
        let parts = self.split_commas(start, end);
        for (ordinal, (from, to)) in parts.iter().copied().enumerate() {
            let Some(name_end) = self.dotted_end(from, to) else {
                continue;
            };
            let last = name_end.saturating_sub(1);
            let name = self.span_text(from, last);
            if name.is_empty() {
                continue;
            }
            let edge = if implements(kind, record_struct, ordinal, &name) {
                EdgeKind::Implements
            } else {
                EdgeKind::UsesType
            };
            self.refer(scope.owner, &scope.name, edge, name, None, from, last);
            if self.is(name_end, "<")
                && let Some(close) = self.close_angle(name_end, to)
            {
                self.uses_from(name_end + 1, close, scope, &[]);
            }
        }
    }
}

pub(crate) fn visible(parent: &Scope, prefix: &Prefix, enum_const: bool) -> bool {
    if prefix.private {
        return false;
    }
    prefix.public || enum_const || parent.kind == ScopeKind::Interface
}

fn implements(kind: DeclKind, record_struct: bool, ordinal: usize, name: &str) -> bool {
    match kind {
        DeclKind::Interface | DeclKind::Struct => true,
        DeclKind::Record if record_struct => true,
        DeclKind::Enum => !integral(name),
        DeclKind::Class | DeclKind::Record => ordinal > 0,
    }
}

fn integral(name: &str) -> bool {
    matches!(
        name,
        "byte" | "sbyte" | "short" | "ushort" | "int" | "uint" | "long" | "ulong"
    )
}

fn constraint(word: &str) -> bool {
    matches!(
        word,
        "class" | "struct" | "unmanaged" | "notnull" | "default" | "new" | "where"
    )
}
