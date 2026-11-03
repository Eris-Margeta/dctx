// SPDX-License-Identifier: Apache-2.0
//! PHP imports, trait uses, literal includes and unresolved call/member sites.
//! Implements decision 077 abstention: no runtime evaluation or dynamic target edges.
//! Facts retain source spellings and scopes for a later authorized resolver.
use super::{Scope, Walker};
use crate::LexError;
use crate::domain::{DiagnosticReason, Precision};
use crate::lexer::Kind;
use dctx_map_model::EdgeKind;

impl Walker<'_> {
    pub(super) fn attributes(
        &mut self,
        at: usize,
        end: usize,
        scope: &Scope,
    ) -> Result<usize, LexError> {
        let close = self.close(at, end).unwrap_or(end);
        let mut i = at + 1;
        while i < close {
            let (name, next) = self.name(i, close);
            if name.is_empty() {
                self.problem(i, DiagnosticReason::Unrecognized);
                break;
            }
            self.reference(
                scope,
                EdgeKind::UsesType,
                name,
                None,
                (i, Precision::Structural),
            )?;
            i = if self.is(next, "(") {
                self.after(next, close)
            } else {
                next
            };
            if !self.is(i, ",") {
                break;
            }
            i += 1;
        }
        Ok((close + 1).min(end))
    }

    pub(super) fn use_decl(
        &mut self,
        at: usize,
        end: usize,
        scope: &Scope,
    ) -> Result<usize, LexError> {
        let mut i = at + 1;
        if self.is(i, "function") || self.is(i, "const") {
            i += 1;
        }
        let kind = if scope.class {
            EdgeKind::UsesType
        } else {
            EdgeKind::Imports
        };
        let mut prefix = String::new();
        let mut grouped = false;
        while i < end {
            if self.is(i, ";") {
                return Ok(i + 1);
            }
            if self.is(i, "}") && grouped {
                grouped = false;
                prefix.clear();
                i += 1;
                continue;
            }
            if self.is(i, ",") || self.is(i, "function") || self.is(i, "const") {
                i += 1;
                continue;
            }
            if self.is(i, "{") && scope.class {
                return Ok(self.after(i, end));
            }
            let site = i;
            let (name, next) = self.name(i, end);
            if name.is_empty() {
                self.problem(i, DiagnosticReason::Unrecognized);
                return Ok(i + 1);
            }
            i = next;
            if self.is(i, "\\") && self.is(i + 1, "{") {
                prefix = format!("{name}\\");
                grouped = true;
                i += 2;
                continue;
            }
            let alias = if self.is(i, "as") {
                i += 1;
                if self.kind(i) == Some(Kind::Ident) {
                    let value = self.text(i).to_string();
                    i += 1;
                    Some(value)
                } else {
                    self.problem(i, DiagnosticReason::Unrecognized);
                    None
                }
            } else {
                None
            };
            self.reference(
                scope,
                kind,
                format!("{prefix}{name}"),
                alias,
                (site, Precision::Structural),
            )?;
            if scope.class && self.is(i, "{") {
                return Ok(self.after(i, end));
            }
            if !matches!(self.text(i), "," | ";" | "}") {
                self.problem(i, DiagnosticReason::Unrecognized);
                return Ok(i.max(at + 1));
            }
        }
        self.problem(at, DiagnosticReason::Unrecognized);
        Ok(end)
    }

    pub(super) fn include(
        &mut self,
        at: usize,
        end: usize,
        scope: &Scope,
    ) -> Result<usize, LexError> {
        let stop = self.statement_end(at + 1, end);
        let mut first = at + 1;
        let mut last = stop;
        while self.is(first, "(") && self.close(first, end) == last.checked_sub(1) {
            first += 1;
            last -= 1;
        }
        if first + 1 == last && self.kind(first) == Some(Kind::String) {
            let raw = self.text(first);
            if let Some(value) = raw.get(1..raw.len().saturating_sub(1))
                && !value.contains('\\')
            {
                self.reference(
                    scope,
                    EdgeKind::Imports,
                    value.to_string(),
                    None,
                    (first, Precision::Structural),
                )?;
            }
        }
        Ok((stop + usize::from(self.is(stop, ";"))).min(end))
    }

    pub(super) fn expression(
        &mut self,
        i: usize,
        end: usize,
        scope: &Scope,
    ) -> Result<usize, LexError> {
        if self.is(i, "new") || self.is(i, "instanceof") {
            let (name, next) = self.name(i + 1, end);
            if !name.is_empty() && !name.eq_ignore_ascii_case("class") {
                self.reference(
                    scope,
                    EdgeKind::UsesType,
                    name,
                    None,
                    (i + 1, Precision::Structural),
                )?;
                return Ok(next);
            }
        }
        if self.kind(i) == Some(Kind::Variable) {
            if i > 0 && self.is(i - 1, "$") {
                return Ok(i + 1);
            }
            if self.is(i, "$this")
                && matches!(self.text(i + 1), "->" | "?->")
                && self.kind(i + 2) == Some(Kind::Ident)
            {
                let kind = if self.is(i + 3, "(") {
                    EdgeKind::Calls
                } else {
                    EdgeKind::UsesType
                };
                let name = format!("$this->{}", self.text(i + 2));
                self.reference(scope, kind, name, None, (i, Precision::Convention))?;
                return Ok(i + 3);
            }
            return Ok(i + 1);
        }
        if self.kind(i) != Some(Kind::Ident) && !self.is(i, "\\") {
            return Ok(i + 1);
        }
        if i > 0 && matches!(self.text(i - 1), "$" | "->" | "?->" | "::" | "\\") {
            return Ok(i + 1);
        }
        let (name, next) = self.name(i, end);
        if name.is_empty() {
            return Ok(i + 1);
        }
        if self.is(next, "::") {
            if self.kind(next + 1) == Some(Kind::Variable) && !self.is(next + 2, "(") {
                let spelling = format!("{name}::{}", self.text(next + 1));
                self.reference(
                    scope,
                    EdgeKind::UsesType,
                    spelling,
                    None,
                    (i, Precision::Structural),
                )?;
                return Ok(next + 2);
            }
            if self.kind(next + 1) == Some(Kind::Ident) {
                let member = self.text(next + 1);
                let spelling = if member.eq_ignore_ascii_case("class") {
                    name
                } else {
                    format!("{name}::{member}")
                };
                let kind = if self.is(next + 2, "(") {
                    EdgeKind::Calls
                } else {
                    EdgeKind::UsesType
                };
                if !(self.is(next + 3, "...") && self.is(next + 4, ")")) {
                    self.reference(scope, kind, spelling, None, (i, Precision::Structural))?;
                }
                return Ok(next + 2);
            }
            return Ok(next + 1);
        }
        if self.is(next, "(")
            && !non_callee(&name)
            && !(self.is(next + 1, "...") && self.is(next + 2, ")"))
        {
            self.reference(
                scope,
                EdgeKind::Calls,
                name,
                None,
                (i, Precision::Structural),
            )?;
        }
        Ok(next)
    }
}

fn non_callee(word: &str) -> bool {
    matches!(
        word.to_ascii_lowercase().as_str(),
        "if" | "elseif"
            | "while"
            | "for"
            | "foreach"
            | "switch"
            | "catch"
            | "match"
            | "isset"
            | "unset"
            | "empty"
            | "eval"
            | "exit"
            | "die"
            | "echo"
            | "print"
            | "array"
            | "list"
            | "declare"
            | "function"
            | "fn"
            | "use"
            | "include"
            | "include_once"
            | "require"
            | "require_once"
    )
}
