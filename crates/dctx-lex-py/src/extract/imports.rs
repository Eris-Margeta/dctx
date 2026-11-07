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
