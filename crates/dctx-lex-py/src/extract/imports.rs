// SPDX-License-Identifier: Apache-2.0
//! `import` and `from ... import` statements, including relative modules and aliases.
//! Implements SPEC-MAP-MODE 5.3 import references for Python.
//! Called from statement dispatch; emits `imports` edges through the reference scan.
use super::{Scope, Walker};
use crate::domain::{DiagnosticReason, Precision};
use crate::lexer::Kind;
use dctx_map_model::EdgeKind;

impl Walker<'_> {
    pub(super) fn import_form(&mut self, scope: &Scope) -> bool {
        if self.at_text("import") {
            self.import_stmt(scope);
            return true;
        }
        if self.at_text("from") {
            self.import_from(scope);
            return true;
        }
        false
    }

    fn import_stmt(&mut self, scope: &Scope) {
        let lo = self.i;
        self.bump();
        loop {
            let module = self.read_dotted();
            if module.is_empty() {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                break;
            }
            let alias = self.read_alias();
            let end = self.i.saturating_sub(1).max(lo);
            self.emit_ref(
                scope,
                EdgeKind::Imports,
                module,
                alias,
                lo,
                end,
                Precision::Structural,
            );
            if !self.eat_op(",") {
                break;
            }
        }
    }

    fn import_from(&mut self, scope: &Scope) {
        let lo = self.i;
        self.bump();
        let module = self.read_relative();
        if module.is_empty() || !self.eat_text("import") {
            self.problem_here(DiagnosticReason::UnexpectedToken);
            self.sync_line();
            return;
        }
        if self.eat_op("*") {
            let end = self.i.saturating_sub(1).max(lo);
            self.emit_ref(
                scope,
                EdgeKind::Imports,
                format!("{module}::*"),
                None,
                lo,
                end,
                Precision::Structural,
            );
            return;
        }
        let paren = self.eat_op("(");
        loop {
            if paren && self.eat_op(")") {
                break;
            }
            if self.at(Kind::Newline) || self.at(Kind::Dedent) || self.eof() {
                break;
            }
            let Some(name) = self.take_name() else {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                break;
            };
            let alias = self.read_alias();
            let end = self.i.saturating_sub(1).max(lo);
            self.emit_ref(
                scope,
                EdgeKind::Imports,
                format!("{module}::{name}"),
                alias,
                lo,
                end,
                Precision::Structural,
            );
            if !self.eat_op(",") {
                if paren {
                    let _ = self.eat_op(")");
                }
                break;
            }
        }
    }

    fn read_dotted(&mut self) -> String {
        let mut module = String::new();
        let Some(first) = self.take_name() else {
            return module;
        };
        module.push_str(&first);
        while self.at_op(".") && self.peek_name() {
            self.bump();
            if let Some(part) = self.take_name() {
                module.push('.');
                module.push_str(&part);
            }
        }
        module
    }

    fn read_relative(&mut self) -> String {
        let mut module = String::new();
        loop {
            if self.eat_op("...") {
                module.push_str("...");
                continue;
            }
            if self.eat_op(".") {
                module.push('.');
                continue;
            }
            break;
        }
        if let Some(first) = self.take_name() {
            module.push_str(&first);
            while self.at_op(".") && self.peek_name() {
                self.bump();
                if let Some(part) = self.take_name() {
                    module.push('.');
                    module.push_str(&part);
                }
            }
        }
        module
    }

    fn read_alias(&mut self) -> Option<String> {
        if self.at_text("as") && self.peek_name() {
            self.bump();
            self.take_name()
        } else {
            None
        }
    }

    pub(super) fn take_name(&mut self) -> Option<String> {
        if !self.at_name() {
            return None;
        }
        let name = self.text().to_owned();
        self.bump();
        Some(name)
    }
}

impl crate::PyLexer {
    /// Statically assigned module-level `__all__`. Unknown expressions or mutations abstain.
    #[must_use]
    pub fn literal_exports(&self, source: &str) -> Option<std::collections::BTreeSet<String>> {
        if !source.contains("__all__") || source.len() > self.limits.max_bytes {
            return None;
        }
        let lexed = crate::lexer::lex(source, self.limits.max_depth).ok()?;
        if !lexed.diagnostics.is_empty() {
            return None;
        }
        let tokens = &lexed.tokens;
        let word = |i: usize| {
            tokens
                .get(i)
                .map_or("", |t| &source[t.start as usize..t.end as usize])
        };
        let mut names = None;
        let mut depth = 0usize;
        for (i, token) in tokens.iter().enumerate() {
            match token.kind {
                Kind::Indent => depth += 1,
                Kind::Dedent => depth = depth.saturating_sub(1),
                Kind::Ident if word(i) == "__all__" => {
                    if depth != 0
                        || (i > 0 && !matches!(tokens[i - 1].kind, Kind::Newline | Kind::Dedent))
                    {
                        return None;
                    }
                    let mut list = match word(i + 1) {
                        "=" => std::collections::BTreeSet::new(),
                        "+=" => names.take()?,
                        _ => return None,
                    };
                    let close = match word(i + 2) {
                        "[" => "]",
                        "(" => ")",
                        _ => return None,
                    };
                    let mut j = i + 3;
                    let mut comma = false;
                    while word(j) != close {
                        let literal = word(j);
                        let quote = literal.chars().next()?;
                        if !matches!(quote, '\'' | '"') {
                            return None;
                        }
                        let name = literal.strip_prefix(quote)?.strip_suffix(quote)?;
                        if name.is_empty() || !name.chars().all(|c| c == '_' || c.is_alphanumeric())
                        {
                            return None;
                        }
                        list.insert(name.to_owned());
                        j += 1;
                        if word(j) == "," {
                            comma = true;
                            j += 1;
                        } else if word(j) != close || (close == ")" && !comma) {
                            return None;
                        }
                    }
                    if tokens.get(j + 1).is_some_and(|t| t.kind != Kind::Newline) {
                        return None;
                    }
                    names = Some(list);
                }
                _ => {}
            }
        }
        names
    }
}
