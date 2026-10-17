//! Declarations: package, imports, types, fields, methods and constructors.
//! Implements SPEC-MAP-MODE 5.1 for Java classes, interfaces, enums and records.
//! Uses the walker in `extract` and the reference scan for bodies and signatures.
use super::{
    is_modifier_word, is_primitive, DeclKind, Prefix, Scope, ScopeKind, SymbolDraft, Walker,
};
use crate::domain::{DiagnosticReason, Precision};
use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};

/// One formal or record component.
struct Declarator {
    type_from: usize,
    type_to: usize,
    name: usize,
    annotations: Vec<String>,
}

enum Shape {
    Method { name: usize, paren: usize },
    Field { name: usize },
    Ctor { name: usize, paren: usize },
    Compact { name: usize, brace: usize },
    Unknown,
}

struct MethodShape {
    start: usize,
    type_from: usize,
    name: usize,
    paren: usize,
    constructor: bool,
    params: Vec<String>,
    bounds: Vec<(usize, usize)>,
}

struct FieldAt {
    start: usize,
    type_from: usize,
    type_to: usize,
    name: usize,
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

struct Heritage {
    end: usize,
    extends: Vec<(usize, usize)>,
    implements: Vec<(usize, usize)>,
    permits: Vec<(usize, usize)>,
}

impl Walker<'_> {
    pub(crate) fn compilation_unit(&mut self) {
        let end = self.toks.len();
        let scope = Scope::file();
        let mut index = 0;
        if self.is_word(0, "package") {
            index = self.package_stmt(0, end);
        }
        while index < end && self.is_word(index, "import") && self.ok() {
            let next = self.import_stmt(index, end);
            index = if next > index { next } else { index + 1 };
        }
        while index < end && self.ok() {
            if self.is_word(index, "module")
                || (self.is_word(index, "open") && self.is_word(index + 1, "module"))
            {
                index = self.skip_module(index, end);
                continue;
            }
            let prefix = self.prefix(index, end);
            if let Some(kind) = self.decl_kind(prefix.next)
                && self.word(self.name_at(prefix.next, kind))
            {
                let next = self.type_decl(index, &prefix, kind, end, &scope);
                index = if next > index { next } else { index + 1 };
                continue;
            }
            if self.is_punct(index, ";") {
                index += 1;
                continue;
            }
            let next = self.recover(index, end);
            index = if next > index { next } else { index + 1 };
        }
    }

    fn package_stmt(&mut self, index: usize, end: usize) -> usize {
        let mut cursor = index + 1;
        if !self.word(cursor) {
            self.problem_at(index, DiagnosticReason::Malformed);
            return self.recover(index, end);
        }
        let start = cursor;
        cursor += 1;
        while cursor + 1 < end && self.is_punct(cursor, ".") && self.word(cursor + 1) {
            cursor += 2;
        }
        let name = self.qualified(start, cursor - 1);
        self.draft.set_package(&name);
        if self.is_punct(cursor, ";") {
            cursor + 1
        } else {
            self.problem_at(index, DiagnosticReason::Malformed);
            cursor
        }
    }

    fn import_stmt(&mut self, index: usize, end: usize) -> usize {
        let mut cursor = index + 1;
        let static_import = self.is_word(cursor, "static");
        if static_import {
            cursor += 1;
        }
        if !self.word(cursor) {
            self.problem_at(index, DiagnosticReason::Malformed);
            return self.recover(index, end);
        }
        let start = cursor;
        cursor += 1;
        while cursor + 1 < end && self.is_punct(cursor, ".") {
            if self.is_punct(cursor + 1, "*") {
                cursor += 2;
                break;
            }
            if self.word(cursor + 1) {
                cursor += 2;
                continue;
            }
            break;
        }
        let mut name = self.qualified(start, cursor - 1);
        if static_import {
            name = format!("static {name}");
        }
        let scope = Scope::file();
        let last = cursor.saturating_sub(1);
        self.refer(
            0,
            &scope,
            EdgeKind::Imports,
            name,
            None,
            index,
            last,
            Precision::Structural,
        );
        if self.is_punct(cursor, ";") {
            cursor + 1
        } else {
            cursor
        }
    }

    fn skip_module(&mut self, index: usize, end: usize) -> usize {
        let mut cursor = index;
        while cursor < end && !self.is_punct(cursor, "{") {
            cursor += 1;
        }
        self.close(cursor).map(|close| (close + 1).min(end)).unwrap_or(end)
    }

    pub(crate) fn type_decl(
        &mut self,
        start: usize,
        prefix: &Prefix,
        kind: DeclKind,
        end: usize,
        scope: &Scope,
    ) -> usize {
        if !self.enter(prefix.next) {
            return end;
        }
        let next = self.type_decl_body(start, prefix, kind, end, scope);
        self.leave();
        next
    }

    fn type_decl_body(
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

    fn member(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
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
            && self.word(self.name_at(at, kind))
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
                FieldAt {
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

    fn finish_method(
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
        let mut cursor = close.map(|index| index + 1).unwrap_or(paren + 1);
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

    fn finish_field(
        &mut self,
        at: FieldAt,
        end: usize,
        scope: &Scope,
        prefix: &Prefix,
        extra: &[String],
    ) -> usize {
        let mut cursor = self.one_field(&at, at.name, end, scope, prefix, extra);
        while self.is_punct(cursor, ",") && self.ok() {
            cursor += 1;
            cursor = self.skip_declarator_mods(cursor, end);
            if !self.word(cursor) {
                break;
            }
            let next_name = cursor;
            cursor = self.one_field(&at, next_name, end, scope, prefix, extra);
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

    fn after_brackets(&self, mut index: usize, end: usize) -> usize {
        while index < end && self.is_punct(index, "[") {
            match self.close(index) {
                Some(close) => index = close + 1,
                None => break,
            }
        }
        index
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
                && (self.is_punct(index, "(") || self.is_punct(index, "{") || self.is_punct(index, "["))
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
                return Some(Shape::Compact { name: top[0], brace: index });
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

    fn type_params(&mut self, open: usize, end: usize) -> (usize, Vec<String>, Vec<(usize, usize)>) {
        let mut cursor = open + 1;
        let mut depth = 1u32;
        let mut names = Vec::new();
        let mut bounds = Vec::new();
        let mut expect_name = true;
        let mut bound_from = None;
        while cursor < end && depth > 0 {
            if self.is_punct(cursor, "<") {
                if depth >= self.max_depth {
                    self.refuse_depth(cursor);
                    return (end, names, bounds);
                }
                depth += 1;
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, ">") {
                depth -= 1;
                if depth == 0 {
                    if let Some(from) = bound_from.take() {
                        bounds.push((from, cursor));
                    }
                    return (cursor + 1, names, bounds);
                }
                cursor += 1;
                continue;
            }
            if depth != 1 {
                cursor += 1;
                continue;
            }
            if self.is_word(cursor, "extends") || self.is_word(cursor, "super") {
                bound_from = Some(cursor + 1);
                expect_name = false;
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, "&") {
                if let Some(from) = bound_from.take() {
                    bounds.push((from, cursor));
                }
                bound_from = Some(cursor + 1);
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, ",") {
                if let Some(from) = bound_from.take() {
                    bounds.push((from, cursor));
                }
                expect_name = true;
                cursor += 1;
                continue;
            }
            if self.is_annotation(cursor) {
                let next = self.skip_annotation_tokens(cursor, end);
                cursor = if next > cursor { next } else { cursor + 1 };
                continue;
            }
            if expect_name && self.word(cursor) && !is_primitive(self.text(cursor)) {
                names.push(self.text(cursor).to_string());
                expect_name = false;
                cursor += 1;
                continue;
            }
            cursor += 1;
        }
        self.problem_at(open, DiagnosticReason::UnbalancedBracket);
        (cursor, names, bounds)
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

    pub(crate) fn skip_one_type(&mut self, mut cursor: usize, end: usize) -> usize {
        while cursor < end && self.is_annotation(cursor) {
            let next = self.skip_annotation_tokens(cursor, end);
            if next <= cursor {
                break;
            }
            cursor = next;
        }
        if cursor >= end || !self.word(cursor) {
            return cursor;
        }
        cursor += 1;
        while cursor + 1 < end && self.is_punct(cursor, ".") && self.word(cursor + 1) {
            cursor += 2;
        }
        if cursor < end && self.is_punct(cursor, "<") {
            cursor = self.skip_angles(cursor, end);
        }
        while cursor < end && self.is_punct(cursor, "[") {
            match self.close(cursor) {
                Some(close) => cursor = close + 1,
                None => break,
            }
        }
        if cursor < end && self.is_punct(cursor, "...") {
            cursor += 1;
        }
        cursor
    }

    pub(crate) fn skip_angles(&mut self, open: usize, end: usize) -> usize {
        let mut cursor = open + 1;
        let mut depth = 1u32;
        while cursor < end && depth > 0 {
            if self.is_punct(cursor, "<") {
                if depth >= self.max_depth {
                    self.refuse_depth(cursor);
                    return end;
                }
                depth += 1;
            } else if self.is_punct(cursor, ">") {
                depth -= 1;
                if depth == 0 {
                    return cursor + 1;
                }
            } else if (self.is_punct(cursor, "(")
                || self.is_punct(cursor, "[")
                || self.is_punct(cursor, "{"))
                && let Some(close) = self.close(cursor)
            {
                cursor = close + 1;
                continue;
            }
            cursor += 1;
        }
        if depth != 0 {
            self.problem_at(open, DiagnosticReason::UnbalancedBracket);
        }
        cursor
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

    fn type_args_uses(&mut self, from: usize, to: usize, scope: &Scope) {
        let mut cursor = from;
        while cursor < to && !self.is_punct(cursor, "<") {
            cursor += 1;
        }
        if cursor >= to || !self.is_punct(cursor, "<") {
            return;
        }
        let close = self.matching_angle(cursor, to);
        if close > cursor + 1 {
            self.type_uses(cursor + 1, close, scope);
        }
    }

    fn matching_angle(&self, open: usize, end: usize) -> usize {
        let mut depth = 0i32;
        let mut cursor = open;
        while cursor < end {
            if self.is_punct(cursor, "<") {
                depth += 1;
            } else if self.is_punct(cursor, ">") {
                depth -= 1;
                if depth == 0 {
                    return cursor;
                }
            }
            cursor += 1;
        }
        end
    }

    fn declarators(&self, from: usize, to: usize) -> Vec<Declarator> {
        let mut out = Vec::new();
        let mut cursor = from;
        let mut segment = from;
        let mut angle = 0i32;
        while cursor < to {
            if self.is_punct(cursor, "<") {
                angle += 1;
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, ">") && angle > 0 {
                angle -= 1;
                cursor += 1;
                continue;
            }
            if angle == 0
                && (self.is_punct(cursor, "(") || self.is_punct(cursor, "[") || self.is_punct(cursor, "{"))
                && let Some(close) = self.close(cursor)
            {
                cursor = (close + 1).min(to);
                continue;
            }
            if angle == 0 && self.is_punct(cursor, ",") {
                if let Some(declarator) = self.declarator_in(segment, cursor) {
                    out.push(declarator);
                }
                cursor += 1;
                segment = cursor;
                continue;
            }
            cursor += 1;
        }
        if let Some(declarator) = self.declarator_in(segment, to) {
            out.push(declarator);
        }
        out
    }

    fn declarator_in(&self, mut start: usize, end: usize) -> Option<Declarator> {
        let mut annotations = Vec::new();
        while start < end {
            if self.is_annotation(start) {
                annotations.push(self.annotation_name(start));
                let next = self.skip_annotation_tokens(start, end);
                start = if next > start { next } else { start + 1 };
                continue;
            }
            if self.word(start) && is_modifier_word(self.text(start)) {
                start += 1;
                continue;
            }
            break;
        }
        let type_from = start;
        let mut last_word = None;
        let mut cursor = start;
        let mut angle = 0i32;
        let mut prev_dot = false;
        while cursor < end {
            if self.is_punct(cursor, "<") {
                angle += 1;
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
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, "[")
                && let Some(close) = self.close(cursor)
            {
                cursor = (close + 1).min(end);
                prev_dot = false;
                continue;
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
            if self.word(cursor) {
                if !prev_dot {
                    last_word = Some(cursor);
                }
                prev_dot = false;
                cursor += 1;
                continue;
            }
            prev_dot = false;
            cursor += 1;
        }
        let name = last_word?;
        if name <= type_from {
            return None;
        }
        Some(Declarator {
            type_from,
            type_to: name,
            name,
            annotations,
        })
    }

    fn recover(&mut self, start: usize, end: usize) -> usize {
        if self.word(start) || self.is_punct(start, "@") {
            self.problem_at(start, DiagnosticReason::Malformed);
        }
        let mut cursor = start + 1;
        while cursor < end {
            if (self.is_punct(cursor, "{") || self.is_punct(cursor, "(") || self.is_punct(cursor, "["))
                && let Some(close) = self.close(cursor)
            {
                cursor = close + 1;
                continue;
            }
            if self.is_punct(cursor, ";") {
                return cursor + 1;
            }
            if self.is_punct(cursor, "}") {
                return cursor;
            }
            cursor += 1;
        }
        end
    }

    pub(crate) fn scan_group(&mut self, open: usize, end: usize, scope: &Scope) -> usize {
        if let Some(close) = self.close(open) {
            self.scan_block(open + 1, close.min(end), scope);
            return (close + 1).min(end);
        }
        open + 1
    }
}

enum HeritageMode {
    None,
    Extends,
    Implements,
    Permits,
}

fn is_visible(scope: &Scope, prefix: &Prefix, enum_const: bool) -> bool {
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
    fn static_and_wildcard_imports_keep_their_spelling() {
        let source = "import java.util.*; import static java.util.List.of;";
        let facts = JavaLexer::default()
            .extract("I.java", source.as_bytes())
            .expect("extract");
        let imports: Vec<_> = facts
            .references
            .iter()
            .filter(|reference| reference.fact.kind.as_str() == "imports")
            .map(|reference| reference.fact.name.as_str())
            .collect();
        assert_eq!(imports, ["java.util.*", "static java.util.List.of"]);
    }

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
        assert_eq!(test.fact.symbol_kind.map(|kind| kind.as_str()), Some("method"));
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
