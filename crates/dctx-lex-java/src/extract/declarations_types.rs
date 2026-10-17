//! Type declarations: classes, interfaces, enums, records and their heritage.
//! Implements SPEC-MAP-MODE 5.1 Java type facts, enum constants and record components.
//! Depends on generic parameter lists and the member walker.
//! Called from the compilation-unit entry and from nested type sites.

use super::declarations_generics::Declarator;
use super::{
    is_modifier_word, DeclKind, Prefix, Scope, ScopeKind, SymbolDraft, Walker,
};
use crate::domain::{DiagnosticReason, Precision};
use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};

struct Heritage {
    end: usize,
    extends: Vec<(usize, usize)>,
    implements: Vec<(usize, usize)>,
    permits: Vec<(usize, usize)>,
}

enum HeritageMode {
    None,
    Extends,
    Implements,
    Permits,
}

impl Walker<'_> {
    pub(super) fn type_decl_body(
        &mut self,
        start: usize,
        prefix: &Prefix,
        kind: DeclKind,
        end: usize,
        scope: &Scope,
    ) -> usize {
        let name_at = self.name_at(prefix.next, kind);
        if !self.word(name_at) {
            self.problem_at(prefix.next, DiagnosticReason::Malformed);
            return self.recover(start, end);
        }
        let local = self.text(name_at).to_string();
        let mut cursor = name_at + 1;
        let mut params = Vec::new();
        let mut bounds = Vec::new();
        if self.is_punct(cursor, "<") && kind != DeclKind::Enum {
            let (next, names, found) = self.type_params(cursor, end);
            params = names;
            bounds = found;
            cursor = next;
        }
        let mut components = Vec::new();
        if kind == DeclKind::Record
            && self.is_punct(cursor, "(")
            && let Some(close) = self.close(cursor)
        {
            components = self.declarators(cursor + 1, close);
            cursor = (close + 1).min(end);
        }
        let heritage = self.heritage(cursor, end);
        cursor = heritage.end;
        let (brace, last) = self.type_tail(cursor, end, start);
        let sig_end = if brace.is_some() {
            self.start_byte(cursor)
        } else {
            self.end_byte(last)
        };
        let body = brace
            .and_then(|open| self.close(open).map(|close| (self.start_byte(open), self.end_byte(close))))
            .unwrap_or_else(|| (self.start_byte(start), sig_end));
        let symbol = match kind {
            DeclKind::Interface | DeclKind::Annotation => SymbolKind::Interface,
            DeclKind::Enum => SymbolKind::Enum,
            DeclKind::Record => SymbolKind::Struct,
            DeclKind::Class => SymbolKind::Class,
        };
        let scope_kind = match kind {
            DeclKind::Interface | DeclKind::Annotation => ScopeKind::Interface,
            DeclKind::Enum => ScopeKind::Enum,
            DeclKind::Record => ScopeKind::Record,
            DeclKind::Class => ScopeKind::Class,
        };
        let Some(child) = self.emit(
            SymbolDraft {
                node_kind: NodeKind::Symbol,
                symbol,
                local: &local,
                simple: &local,
                scope_kind,
                type_params: params,
                first: start,
                last,
                sig_end,
                body,
                is_public: prefix.public,
                precision: Precision::Structural,
            },
            scope,
        ) else {
            return self.recover(start, end);
        };
        self.scan_annotation_list(prefix, &child);
        for (from, to) in &bounds {
            self.type_uses(*from, *to, &child);
        }
        self.emit_heritage(&child, kind, &heritage);
        for component in &components {
            self.emit_component(component, &child);
        }
        if let Some(open) = brace {
            let close = self.close(open).unwrap_or(end);
            self.class_body(open + 1, close.min(end), &child, kind == DeclKind::Enum);
            return (close + 1).min(end);
        }
        if self.is_punct(cursor, ";") {
            return (cursor + 1).min(end);
        }
        cursor
    }

    fn type_tail(&self, cursor: usize, end: usize, start: usize) -> (Option<usize>, usize) {
        if self.is_punct(cursor, "{") {
            let close = self.close(cursor).unwrap_or(end.saturating_sub(1));
            return (Some(cursor), close.min(end.saturating_sub(1)).max(start));
        }
        if cursor > start {
            (None, cursor.saturating_sub(1))
        } else {
            (None, start)
        }
    }

    pub(crate) fn class_body(&mut self, mut index: usize, end: usize, scope: &Scope, enum_head: bool) {
        if enum_head {
            index = self.enum_constants(index, end, scope);
            if self.is_punct(index, ";") {
                index += 1;
            }
        }
        while index < end && self.ok() {
            let next = self.member(index, end, scope);
            index = if next > index { next } else { index + 1 };
        }
    }

    fn enum_constants(&mut self, mut index: usize, end: usize, scope: &Scope) -> usize {
        while index < end && self.ok() {
            let start = index;
            let prefix = self.prefix(index, end);
            if !self.looks_like_constant(prefix.next, end, scope) {
                return start;
            }
            let name = prefix.next;
            let local = self.text(name).to_string();
            let mut cursor = name + 1;
            let args = self.is_punct(cursor, "(").then_some(cursor);
            if let Some(open) = args {
                cursor = self.close(open).map(|close| close + 1).unwrap_or(cursor + 1);
            }
            let body = self.is_punct(cursor, "{").then_some(cursor);
            if let Some(open) = body {
                cursor = self.close(open).map(|close| close + 1).unwrap_or(end);
            }
            let last = cursor.saturating_sub(1).max(name);
            let sig_end = args
                .and_then(|open| self.close(open))
                .map(|close| self.end_byte(close))
                .unwrap_or_else(|| self.end_byte(name));
            let body_span = body
                .and_then(|open| self.close(open).map(|close| (self.start_byte(open), self.end_byte(close))))
                .unwrap_or((self.start_byte(name), self.end_byte(name)));
            if let Some(child) = self.emit(
                SymbolDraft {
                    node_kind: NodeKind::Symbol,
                    symbol: SymbolKind::Const,
                    local: &local,
                    simple: &local,
                    scope_kind: ScopeKind::Class,
                    type_params: Vec::new(),
                    first: start,
                    last,
                    sig_end,
                    body: body_span,
                    is_public: true,
                    precision: Precision::Structural,
                },
                scope,
            ) {
                self.scan_annotation_list(&prefix, &child);
                if let Some(open) = args.and_then(|open| self.close(open).map(|close| (open, close))) {
                    self.scan_block(open.0 + 1, open.1, &child);
                }
                if let Some(open) = body.and_then(|open| self.close(open).map(|close| (open, close))) {
                    self.class_body(open.0 + 1, open.1, &child, false);
                }
            }
            if self.is_punct(cursor, ",") {
                index = cursor + 1;
                continue;
            }
            return cursor;
        }
        index
    }

    fn looks_like_constant(&self, index: usize, end: usize, scope: &Scope) -> bool {
        if !self.word(index) || is_modifier_word(self.text(index)) || self.decl_kind(index).is_some() {
            return false;
        }
        if self.text(index) == scope.simple && self.is_punct(index + 1, "(") {
            return false;
        }
        let next = index + 1;
        next >= end
            || self.is_punct(next, "(")
            || self.is_punct(next, "{")
            || self.is_punct(next, ",")
            || self.is_punct(next, ";")
    }

    fn emit_component(&mut self, component: &Declarator, scope: &Scope) {
        let local = self.text(component.name).to_string();
        if let Some(child) = self.emit(
            SymbolDraft {
                node_kind: NodeKind::Symbol,
                symbol: SymbolKind::Const,
                local: &local,
                simple: scope.simple.as_str(),
                scope_kind: scope.kind,
                type_params: Vec::new(),
                first: component.type_from,
                last: component.name,
                sig_end: self.end_byte(component.name),
                body: (self.start_byte(component.type_from), self.end_byte(component.name)),
                is_public: false,
                precision: Precision::Structural,
            },
            scope,
        ) {
            self.type_uses(component.type_from, component.type_to, &child);
            for annotation in &component.annotations {
                self.refer(
                    child.owner,
                    &child,
                    EdgeKind::UsesType,
                    annotation.clone(),
                    None,
                    component.name,
                    component.name,
                    Precision::Structural,
                );
            }
        }
    }

    fn heritage(&mut self, mut cursor: usize, end: usize) -> Heritage {
        let mut mode = HeritageMode::None;
        let mut heritage = Heritage {
            end: cursor,
            extends: Vec::new(),
            implements: Vec::new(),
            permits: Vec::new(),
        };
        while cursor < end && !self.is_punct(cursor, "{") && !self.is_punct(cursor, ";") && self.ok() {
            if self.is_word(cursor, "extends") {
                mode = HeritageMode::Extends;
                cursor += 1;
                continue;
            }
            if self.is_word(cursor, "implements") {
                mode = HeritageMode::Implements;
                cursor += 1;
                continue;
            }
            if self.is_word(cursor, "permits") {
                mode = HeritageMode::Permits;
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, ",") {
                cursor += 1;
                continue;
            }
            let from = cursor;
            let next = self.skip_one_type(cursor, end);
            if next <= cursor {
                cursor += 1;
                continue;
            }
            match mode {
                HeritageMode::Extends => heritage.extends.push((from, next)),
                HeritageMode::Implements => heritage.implements.push((from, next)),
                HeritageMode::Permits => heritage.permits.push((from, next)),
                HeritageMode::None => {}
            }
            cursor = next;
        }
        heritage.end = cursor;
        heritage
    }

    fn emit_heritage(&mut self, scope: &Scope, kind: DeclKind, heritage: &Heritage) {
        let interfaces = matches!(kind, DeclKind::Interface | DeclKind::Annotation);
        for &(from, to) in &heritage.extends {
            if interfaces {
                self.refer_outer(scope, EdgeKind::Implements, from, to);
                self.type_args_uses(from, to, scope);
            } else {
                self.type_uses(from, to, scope);
            }
        }
        for &(from, to) in &heritage.implements {
            self.refer_outer(scope, EdgeKind::Implements, from, to);
            self.type_args_uses(from, to, scope);
        }
        for &(from, to) in &heritage.permits {
            self.type_uses(from, to, scope);
        }
    }

    fn refer_outer(&mut self, scope: &Scope, kind: EdgeKind, from: usize, to: usize) {
        let name = self.outer_type_name(from, to);
        if name.is_empty() {
            return;
        }
        self.refer(scope.owner, scope, kind, name, None, from, from, Precision::Structural);
    }

    fn outer_type_name(&self, mut from: usize, to: usize) -> String {
        while from < to && self.is_annotation(from) {
            let next = self.skip_annotation_tokens(from, to);
            if next <= from {
                break;
            }
            from = next;
        }
        if !self.word(from) || from >= to {
            return String::new();
        }
        let mut name = self.text(from).to_string();
        let mut cursor = from + 1;
        while cursor + 1 < to && self.is_punct(cursor, ".") && self.word(cursor + 1) {
            name.push('.');
            name.push_str(self.text(cursor + 1));
            cursor += 2;
        }
        name
    }
}
