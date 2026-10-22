//! Declaration entry points: package, imports, modules and type dispatch.
//! Implements SPEC-MAP-MODE 5.1 for a Java compilation unit.
//! Uses the type, member, modifier and generic walkers, plus the reference scan.
//! [`super::extract`] starts here; nested recovery calls [`Walker::type_decl`].

use super::{DeclKind, Prefix, Scope, Walker};
use crate::domain::{DiagnosticReason, Precision};
use dctx_map_model::EdgeKind;

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
                && self.word(Self::name_at(prefix.next, kind))
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
        self.close(cursor).map_or(end, |close| (close + 1).min(end))
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

    pub(super) fn recover(&mut self, start: usize, end: usize) -> usize {
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

#[cfg(test)]
mod tests {
    use crate::JavaLexer;

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
}
