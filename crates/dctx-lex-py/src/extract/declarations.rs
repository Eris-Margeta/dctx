// SPDX-License-Identifier: Apache-2.0
//! Function, class and compound statements, plus the suites they own.
//! Implements SPEC-MAP-MODE 5.1 declaration spans for Python `def`, `class` and blocks.
//! Calls import, reference and docstring scans; the walker drives `statement`.
use super::{Scope, Walker, qualify};
use crate::domain::DiagnosticReason;
use crate::lexer::Kind;
use dctx_map_model::SymbolKind;

impl Walker<'_> {
    pub(super) fn unexpected_indent(&mut self, scope: &Scope) {
        self.problem_here(DiagnosticReason::InconsistentIndent);
        self.bump();
        let nested = scope.blocked();
        while self.i < self.toks.len() && self.refused.is_none() && !self.at(Kind::Dedent) {
            if self.at(Kind::Newline) {
                self.bump();
                continue;
            }
            let mark = self.i;
            self.statement(&nested);
            if self.i == mark {
                self.bump();
            }
        }
        if self.at(Kind::Dedent) {
            self.bump();
        }
    }

    pub(super) fn statement(&mut self, scope: &Scope) {
        if self.refused.is_some() || self.eof() {
            return;
        }
        let start = self.i;
        let decorated = self.leading_decorators(scope);
        if self.definition(start, scope) || self.compound(scope) || self.import_form(scope) {
            return;
        }
        if decorated || self.stray_clause() {
            self.recover_stray(scope);
        } else {
            self.simple(scope);
        }
    }

    fn leading_decorators(&mut self, scope: &Scope) -> bool {
        let mut decorated = false;
        while self.at_op("@") {
            decorated = true;
            self.decorator(scope);
            while self.at(Kind::Newline) {
                self.bump();
            }
        }
        decorated
    }

    fn definition(&mut self, start: usize, scope: &Scope) -> bool {
        if (self.at_text("async") && self.peek_text("def")) || self.at_text("def") {
            self.function(start, scope);
            return true;
        }
        if self.at_text("class") {
            self.class_stmt(start, scope);
            return true;
        }
        false
    }

    fn compound(&mut self, scope: &Scope) -> bool {
        if self.async_for_or_with(scope)
            || self.word_stmt("if", Self::if_stmt, scope)
            || self.word_stmt("while", Self::while_stmt, scope)
            || self.word_stmt("for", Self::for_stmt, scope)
            || self.word_stmt("try", Self::try_stmt, scope)
            || self.word_stmt("with", Self::with_stmt, scope)
        {
            return true;
        }
        if self.at_text("match") && self.header_follows() {
            self.match_stmt(scope);
            return true;
        }
        if scope.in_match && self.at_text("case") {
            self.case_clause(scope);
            return true;
        }
        false
    }

    fn word_stmt(&mut self, word: &str, run: fn(&mut Self, &Scope), scope: &Scope) -> bool {
        if self.at_text(word) {
            run(self, scope);
            true
        } else {
            false
        }
    }

    fn async_for_or_with(&mut self, scope: &Scope) -> bool {
        if self.at_text("async") && (self.peek_text("for") || self.peek_text("with")) {
            self.bump();
            if self.at_text("for") {
                self.for_stmt(scope);
            } else {
                self.with_stmt(scope);
            }
            return true;
        }
        false
    }

    fn stray_clause(&self) -> bool {
        self.at_text("else")
            || self.at_text("elif")
            || self.at_text("except")
            || self.at_text("finally")
    }

    fn recover_stray(&mut self, scope: &Scope) {
        self.problem_here(DiagnosticReason::UnexpectedToken);
        self.bump();
        self.scan_until_colon();
        if self.at_op(":") {
            self.suite(&scope.blocked());
        } else {
            self.sync_line();
        }
    }

    fn function(&mut self, sig_at: usize, scope: &Scope) {
        if self.at_text("async") {
            self.bump();
        }
        if !self.eat_text("def") {
            self.problem_here(DiagnosticReason::UnexpectedToken);
            self.sync_line();
            return;
        }
        let Some(local) = self.take_name() else {
            self.problem_here(DiagnosticReason::UnexpectedToken);
            self.sync_line();
            return;
        };
        let header = self.i;
        let (_, hi) = self.scan_until_colon();
        self.calls(header, hi, scope);
        if !self.at_op(":") {
            self.problem_here(DiagnosticReason::UnexpectedToken);
            self.sync_line();
            return;
        }
        let colon = self.i;
        let doc = self.lookahead_suite_doc();
        let symbol = if scope.class_body {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };
        let qualified = qualify(scope, &local);
        let Some(owner) = self.emit_symbol(scope, &qualified, &local, symbol, sig_at, colon, doc)
        else {
            self.suite(&scope.blocked());
            return;
        };
        let inner = Scope {
            owner,
            name: qualified,
            class_body: false,
            module_top: false,
            in_match: false,
        };
        self.suite(&inner);
        self.finish_body(owner, colon);
    }

    fn class_stmt(&mut self, sig_at: usize, scope: &Scope) {
        self.bump();
        let Some(local) = self.take_name() else {
            self.problem_here(DiagnosticReason::UnexpectedToken);
            self.sync_line();
            return;
        };
        let header = self.i;
        let (_, hi) = self.scan_until_colon();
        self.calls(header, hi, scope);
        if !self.at_op(":") {
            self.problem_here(DiagnosticReason::UnexpectedToken);
            self.sync_line();
            return;
        }
        let colon = self.i;
        let doc = self.lookahead_suite_doc();
        let qualified = qualify(scope, &local);
        let Some(owner) = self.emit_symbol(
            scope,
            &qualified,
            &local,
            SymbolKind::Class,
            sig_at,
            colon,
            doc,
        ) else {
            self.suite(&scope.blocked());
            return;
        };
        let inner = Scope {
            owner,
            name: qualified,
            class_body: true,
            module_top: false,
            in_match: false,
        };
        self.suite(&inner);
        self.finish_body(owner, colon);
    }

    fn suite(&mut self, scope: &Scope) {
        if self.refused.is_some() {
            return;
        }
        if !self.at_op(":") {
            self.problem_here(DiagnosticReason::UnexpectedToken);
            self.sync_line();
            return;
        }
        if self.depth >= self.max_depth {
            self.refuse_depth();
            return;
        }
        self.depth += 1;
        self.bump();
        if self.at(Kind::Newline) {
            self.bump();
            if self.at(Kind::Indent) {
                self.bump();
                self.block(scope);
                if self.at(Kind::Dedent) {
                    self.bump();
                }
            }
            self.depth -= 1;
            return;
        }
        while self.refused.is_none()
            && !self.eof()
            && !self.at(Kind::Newline)
            && !self.at(Kind::Dedent)
        {
            let mark = self.i;
            self.statement(scope);
            if self.i == mark {
                self.bump();
            }
            if !self.eat_op(";") {
                break;
            }
        }
        if self.at(Kind::Newline) {
            self.bump();
        }
        self.depth -= 1;
    }

    fn block(&mut self, scope: &Scope) {
        while self.refused.is_none() && !self.eof() && !self.at(Kind::Dedent) {
            if self.at(Kind::Newline) {
                self.bump();
                continue;
            }
            let mark = self.i;
            self.statement(scope);
            if self.i == mark {
                self.bump();
            }
        }
    }

    fn if_stmt(&mut self, scope: &Scope) {
        let nested = scope.blocked();
        loop {
            self.bump();
            let (lo, hi) = self.scan_until_colon();
            self.calls(lo, hi, scope);
            if !self.at_op(":") {
                self.problem_here(DiagnosticReason::UnexpectedToken);
                self.sync_line();
                return;
            }
            self.suite(&nested);
            if self.at_text("elif") {
                continue;
            }
            break;
        }
        self.optional_else(&nested);
    }

    fn while_stmt(&mut self, scope: &Scope) {
        self.header_suite(scope, true);
    }

    fn with_stmt(&mut self, scope: &Scope) {
        self.header_suite(scope, false);
    }

    fn header_suite(&mut self, scope: &Scope, else_clause: bool) {
        self.bump();
        let nested = scope.blocked();
        let (lo, hi) = self.scan_until_colon();
        self.calls(lo, hi, scope);
        if !self.at_op(":") {
            self.problem_here(DiagnosticReason::UnexpectedToken);
            self.sync_line();
            return;
        }
        self.suite(&nested);
        if else_clause {
            self.optional_else(&nested);
        }
    }

    fn for_stmt(&mut self, scope: &Scope) {
        self.bump();
        let nested = scope.blocked();
        let mut depth = 0i32;
        let mut expr = None;
        while !self.eof() {
            if depth == 0 && (self.at_op(":") || self.at(Kind::Newline)) {
                break;
            }
            if depth == 0 && self.at_text("in") && expr.is_none() {
                expr = Some(self.i + 1);
            }
            depth += self.delta(self.i);
            if !self.bump() {
                break;
            }
        }
        if let Some(lo) = expr {
            self.calls(lo, self.i, scope);
        }
        if !self.at_op(":") {
            self.problem_here(DiagnosticReason::UnexpectedToken);
            self.sync_line();
            return;
        }
        self.suite(&nested);
        self.optional_else(&nested);
    }

    fn try_stmt(&mut self, scope: &Scope) {
        self.bump();
        let nested = scope.blocked();
        if !self.at_op(":") {
            self.problem_here(DiagnosticReason::UnexpectedToken);
            self.sync_line();
            return;
        }
        self.suite(&nested);
        while self.at_text("except") {
            self.bump();
            let (lo, hi) = self.scan_until_colon();
            self.calls(lo, hi, scope);
            if !self.at_op(":") {
                break;
            }
            self.suite(&nested);
        }
        self.optional_else(&nested);
        if self.at_text("finally") {
            self.bump();
            if self.at_op(":") {
                self.suite(&nested);
            }
        }
    }

    fn match_stmt(&mut self, scope: &Scope) {
        self.bump();
        let (lo, hi) = self.scan_until_colon();
        self.calls(lo, hi, scope);
        let mut body = scope.blocked();
        body.in_match = true;
        if self.at_op(":") {
            self.suite(&body);
        } else {
            self.problem_here(DiagnosticReason::UnexpectedToken);
            self.sync_line();
        }
    }

    fn case_clause(&mut self, scope: &Scope) {
        self.bump();
        let mut depth = 0i32;
        let mut guard = None;
        while !self.eof() {
            if depth == 0 && self.at_op(":") {
                break;
            }
            if depth == 0 && self.at_text("if") && guard.is_none() {
                guard = Some(self.i);
            }
            depth += self.delta(self.i);
            if !self.bump() {
                break;
            }
        }
        if let Some(lo) = guard {
            self.calls(lo, self.i, scope);
        }
        let nested = scope.blocked();
        if self.at_op(":") {
            self.suite(&nested);
        } else {
            self.problem_here(DiagnosticReason::UnexpectedToken);
            self.sync_line();
        }
    }

    fn optional_else(&mut self, scope: &Scope) {
        if self.at_text("else") {
            self.bump();
            if self.at_op(":") {
                self.suite(scope);
            }
        }
    }

    fn simple(&mut self, scope: &Scope) {
        let lo = self.i;
        if lo >= self.toks.len() {
            return;
        }
        let mut depth = 0i32;
        while self.i < self.toks.len() {
            if depth == 0
                && (self.at(Kind::Newline)
                    || self.at(Kind::Dedent)
                    || self.at(Kind::Indent)
                    || self.at_op(";"))
            {
                break;
            }
            depth += self.delta(self.i);
            if !self.bump() {
                break;
            }
        }
        let hi = self.i;
        if scope.module_top {
            self.constants(lo, hi, scope);
        }
        self.calls(lo, hi, scope);
    }

    fn decorator(&mut self, scope: &Scope) {
        self.bump();
        let lo = self.i;
        let mut depth = 0i32;
        while self.i < self.toks.len() {
            if depth == 0 && (self.at(Kind::Newline) || self.at(Kind::Dedent)) {
                break;
            }
            depth += self.delta(self.i);
            if !self.bump() {
                break;
            }
        }
        self.calls(lo, self.i, scope);
    }

    fn scan_until_colon(&mut self) -> (usize, usize) {
        let lo = self.i;
        let mut depth = 0i32;
        while self.i < self.toks.len() {
            if depth <= 0
                && (self.at_op(":")
                    || self.at(Kind::Newline)
                    || self.at(Kind::Dedent)
                    || self.at(Kind::Indent))
            {
                break;
            }
            depth += self.delta(self.i);
            if !self.bump() {
                break;
            }
        }
        (lo, self.i)
    }
}
