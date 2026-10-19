//! Declaration dispatch, attributes and Swift type scopes.
//! Implements SPEC-MAP-MODE symbol extraction from balanced token shapes.
//! Uses the assembly walker; dynamic dispatch and macro expansion are abstained.
use super::{Item, Walker};
use crate::domain::{DiagnosticReason, Precision};
use crate::lexer::Kind;
use dctx_map_model::{EdgeKind, SymbolKind};

impl Walker<'_> {
    pub(super) fn walk(
        &mut self,
        mut i: usize,
        end: usize,
        owner: usize,
        in_enum: bool,
    ) -> Result<(), crate::LexError> {
        while i < end {
            if self.text(i) == ";" {
                i += 1;
                continue;
            }
            if self.lex.tokens[i].kind == Kind::Directive {
                let line = self.lex.line(self.start(i));
                let directive = self.text(i);
                let conditional = matches!(directive, "#if" | "#elseif" | "#else" | "#endif");
                if !conditional && !super::expression_directive(directive) {
                    self.problem(i, DiagnosticReason::Unrecognized);
                }
                i += 1;
                while i < end && self.lex.line(self.start(i)) == line {
                    i = self.close(i).map_or(i + 1, |j| j + 1);
                }
                continue;
            }
            let first = i;
            let (at, mut public) = self.prefix(i, end);
            let parent = &self.output.nodes[owner].fact;
            if parent.public
                && matches!(
                    parent.symbol_kind,
                    Some(SymbolKind::Interface | SymbolKind::Impl)
                )
                && !(first..at).any(|i| {
                    matches!(
                        self.text(i),
                        "private" | "fileprivate" | "internal" | "package"
                    )
                })
            {
                public = true;
            }
            let word = self.text(at).to_string();
            if word == "import" {
                i = self.import(at, end, owner)?;
                continue;
            }
            if word == "case" && in_enum {
                i = self.cases(
                    first,
                    at,
                    end,
                    owner,
                    public || self.output.nodes[owner].fact.public,
                )?;
                continue;
            }
            let kind = match word.as_str() {
                "class" | "actor" => Some(SymbolKind::Class),
                "struct" => Some(SymbolKind::Struct),
                "enum" => Some(SymbolKind::Enum),
                "protocol" => Some(SymbolKind::Interface),
                "extension" => Some(SymbolKind::Impl),
                "typealias" | "associatedtype" => Some(SymbolKind::Type),
                "func" => Some(if owner == 0 {
                    SymbolKind::Function
                } else {
                    SymbolKind::Method
                }),
                "init" | "deinit" | "subscript" => Some(SymbolKind::Method),
                "let" => Some(SymbolKind::Const),
                "var" => None,
                "operator" => Some(SymbolKind::Function),
                // Scripts and statement-only spans are valid Swift. Skip them
                // without a diagnostic; calls inside stay lexical references.
                _ => {
                    i = self.skip_statement(at, end, owner)?;
                    continue;
                }
            };
            let fixed = matches!(word.as_str(), "init" | "deinit" | "subscript");
            let name_at = if fixed { at } else { at + 1 };
            if name_at >= end
                || (!fixed && !self.ident(name_at) && word != "operator" && word != "func")
            {
                self.problem(at, DiagnosticReason::UnsupportedName);
                i = (at + 1).min(end);
                continue;
            }
            if matches!(self.text(name_at), "(" | "{" | "}" | ";" | "=" | "") {
                self.problem(at, DiagnosticReason::UnsupportedName);
                i = at + 1;
                continue;
            }
            let mut name_end = name_at;
            if word == "extension" {
                while name_end + 2 < end
                    && self.text(name_end + 1) == "."
                    && self.ident(name_end + 2)
                {
                    name_end += 2;
                }
            }
            let mut name =
                self.src[self.start(name_at) as usize..self.end(name_end) as usize].to_string();
            if word == "extension" {
                name = format!("extension {name}");
            }
            let (last, body, sig) = self.boundary(name_end + 1, end);
            let last = last.max(name_end).min(end - 1);
            let needs_body = matches!(
                word.as_str(),
                "class" | "actor" | "struct" | "enum" | "protocol" | "extension"
            );
            let callable_body = matches!(word.as_str(), "func" | "init" | "deinit" | "subscript")
                && self.output.nodes[owner].fact.symbol_kind != Some(SymbolKind::Interface);
            let incomplete = (needs_body || callable_body) && body.is_none();
            if incomplete {
                self.problem(at, DiagnosticReason::Unrecognized);
            }
            if matches!(self.text(last), "=" | "->" | ":" | "," | "where") {
                self.problem(last, DiagnosticReason::Unrecognized);
            }
            if matches!(word.as_str(), "func" | "init" | "subscript")
                && !(name_end + 1..=last).any(|i| self.text(i) == "(")
            {
                self.problem(at, DiagnosticReason::Unrecognized);
            }
            let item = Item {
                name,
                kind,
                first,
                last,
                signature: sig.max(self.end(name_end)),
                public,
                precision: if incomplete {
                    Precision::Partial
                } else {
                    Precision::Structural
                },
            };
            if let Some(index) = self.add(&item, owner)? {
                if let Some(open) = body {
                    let close = self.close(open).unwrap_or(end);
                    if needs_body {
                        self.walk(open + 1, close.min(end), index, word == "enum")?;
                    } else {
                        self.references(open + 1, close.min(end), index)?;
                    }
                }
                if matches!(word.as_str(), "var" | "let" | "typealias") {
                    self.initializer(name_end + 1, body.unwrap_or(last + 1), index)?;
                }
                if matches!(word.as_str(), "var" | "let") {
                    self.additional_bindings(
                        name_end + 1,
                        body.unwrap_or(last + 1),
                        owner,
                        public,
                    )?;
                }
            }
            i = last + 1;
        }
        Ok(())
    }

    /// Skips one statement, including an `else` chain, and records calls in it.
    fn skip_statement(
        &mut self,
        at: usize,
        end: usize,
        owner: usize,
    ) -> Result<usize, crate::LexError> {
        let mut next = self.statement_end(at, end);
        while next < end && self.text(next) == "else" {
            next = self.statement_end(next, end);
        }
        self.references(at, next, owner)?;
        Ok(next)
    }

    /// Index of the first token after the statement that starts at `at`.
    fn statement_end(&self, at: usize, end: usize) -> usize {
        if at >= end {
            return end;
        }
        if matches!(self.text(at), "{" | "(" | "[") {
            return self.close(at).map_or(at + 1, |j| (j + 1).min(end));
        }
        if at + 1 >= end {
            return end;
        }
        let (last, _, _) = self.boundary(at + 1, end);
        (last.max(at) + 1).min(end).max(at + 1)
    }

    fn prefix(&self, mut i: usize, end: usize) -> (usize, bool) {
        let mut public = false;
        while i < end {
            if self.text(i) == "@" && self.ident(i + 1) {
                i += 2;
                while self.text(i) == "." && self.ident(i + 1) {
                    i += 2;
                }
                if self.text(i) == "(" {
                    i = self.close(i).map_or(end, |j| j + 1);
                }
                continue;
            }
            if !modifier(self.text(i)) {
                break;
            }
            if self.text(i) == "class" && !matches!(self.text(i + 1), "func" | "var" | "subscript")
            {
                break;
            }
            let restricted_setter = matches!(
                self.text(i),
                "private" | "fileprivate" | "internal" | "package"
            ) && self.text(i + 1) == "(";
            if !restricted_setter && matches!(self.text(i), "public" | "open") {
                public = true;
            }
            i += 1;
            if self.text(i) == "(" {
                i = self.close(i).map_or(end, |j| j + 1);
            }
        }
        (i, public)
    }

    fn import(&mut self, at: usize, end: usize, owner: usize) -> Result<usize, crate::LexError> {
        let mut i = at + 1;
        if matches!(
            self.text(i),
            "typealias" | "struct" | "class" | "enum" | "protocol" | "let" | "var" | "func"
        ) {
            i += 1;
        }
        let start = i;
        if !self.ident(i) {
            self.problem(at, DiagnosticReason::UnsupportedName);
            return Ok(i);
        }
        i += 1;
        while i + 1 < end && self.text(i) == "." && self.ident(i + 1) {
            i += 2;
        }
        let name = self.src[self.start(start) as usize..self.end(i - 1) as usize]
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect();
        self.refer(
            owner,
            EdgeKind::Imports,
            name,
            start,
            i - 1,
            Precision::Structural,
        )?;
        Ok(i)
    }
}

fn modifier(word: &str) -> bool {
    matches!(
        word,
        "public"
            | "open"
            | "private"
            | "fileprivate"
            | "internal"
            | "package"
            | "final"
            | "static"
            | "class"
            | "override"
            | "required"
            | "convenience"
            | "lazy"
            | "weak"
            | "unowned"
            | "mutating"
            | "nonmutating"
            | "nonisolated"
            | "isolated"
            | "indirect"
            | "prefix"
            | "postfix"
            | "infix"
            | "distributed"
            | "optional"
            | "dynamic"
            | "borrowing"
            | "consuming"
    )
}
