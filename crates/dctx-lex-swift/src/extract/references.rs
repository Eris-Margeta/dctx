// SPDX-License-Identifier: Apache-2.0
//! Unresolved Swift call and member references with lexical owners.
//! Implements SPEC-MAP-MODE 5.3 without inventing dispatch destinations.
//! Macros are opaque; member reads use convention-labeled uses-type hints.
use super::Walker;
use crate::domain::{Labeled, Precision, RawReference};
use crate::lexer::Kind;
use dctx_map_model::EdgeKind;

impl Walker<'_> {
    pub(super) fn refer(
        &mut self,
        owner: usize,
        kind: EdgeKind,
        name: String,
        first: usize,
        last: usize,
        precision: Precision,
    ) -> Result<(), crate::LexError> {
        let node = &self.output.nodes[owner].fact;
        let start = self.start(first);
        let end = self.end(last).saturating_sub(1).max(start);
        let precision = if self
            .output
            .diagnostics
            .iter()
            .any(|d| start <= d.byte && d.byte <= self.end(last))
        {
            Precision::Partial
        } else {
            precision
        };
        self.output.references.push(Labeled {
            fact: RawReference {
                src: node.id.clone(),
                kind,
                name,
                scope: if owner == 0 {
                    String::new()
                } else {
                    node.name.clone()
                },
                alias: None,
                lines: [self.lex.line(start), self.lex.line(end)],
                owner_path: self.output.path.clone(),
            },
            precision,
        });
        self.check()
    }

    pub(super) fn initializer(
        &mut self,
        first: usize,
        end: usize,
        owner: usize,
    ) -> Result<(), crate::LexError> {
        if let Some(eq) = (first..end).find(|&i| self.text(i) == "=") {
            self.references(eq + 1, end, owner)?;
        }
        Ok(())
    }

    pub(super) fn references(
        &mut self,
        mut i: usize,
        end: usize,
        owner: usize,
    ) -> Result<(), crate::LexError> {
        let mut control_header = false;
        let mut next_local = i;
        while i < end {
            if i >= next_local
                && let Some(next) = self.local_function(i, end, owner, &mut next_local)?
            {
                i = next;
                continue;
            }
            if self.lex.tokens[i].kind == Kind::Directive {
                let directive = self.text(i);
                let conditional = matches!(directive, "#if" | "#elseif" | "#else" | "#endif");
                let expression = super::expression_directive(directive);
                if !conditional && !expression {
                    self.problem(i, crate::DiagnosticReason::Unrecognized);
                }
                let line = self.lex.line(self.start(i));
                i += 1;
                if conditional {
                    while i < end && self.lex.line(self.start(i)) == line {
                        i += 1;
                    }
                } else {
                    if self.text(i) == "(" {
                        i = self.close(i).map_or(end, |j| j + 1);
                    }
                    // An unknown macro may own a brace body. Availability and
                    // expression directives do not: the brace is the enclosing
                    // `if` or call.
                    if !expression && self.text(i) == "{" {
                        i = self.close(i).map_or(end, |j| j + 1);
                    }
                }
                continue;
            }
            if matches!(
                self.text(i),
                "if" | "while" | "for" | "guard" | "switch" | "catch"
            ) {
                control_header = true;
            } else if matches!(self.text(i), "{" | "}" | ";") {
                control_header = false;
            }
            if !self.ident(i) || non_callee(self.text(i)) {
                i += 1;
                continue;
            }
            let first = i;
            let mut last = i;
            while last + 1 < end {
                let dot = if matches!(self.text(last + 1), "?" | "!") {
                    last + 2
                } else {
                    last + 1
                };
                if self.text(dot) != "." || !self.ident(dot + 1) {
                    break;
                }
                last = dot + 1;
            }
            let next = self.specialization(last + 1, end);
            let is_call = self.text(next) == "(" || (self.text(next) == "{" && !control_header);
            let declaration = first > 0
                && matches!(
                    self.text(first - 1),
                    "func" | "let" | "var" | "class" | "struct" | "enum"
                );
            if !declaration && (is_call || last > first) {
                let name = self.src[self.start(first) as usize..self.end(last) as usize]
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                self.refer(
                    owner,
                    if is_call {
                        EdgeKind::Calls
                    } else {
                        EdgeKind::UsesType
                    },
                    name,
                    first,
                    last,
                    if is_call {
                        Precision::Structural
                    } else {
                        Precision::Convention
                    },
                )?;
            }
            i = if is_call && next > last + 1 {
                next
            } else {
                last + 1
            };
        }
        Ok(())
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "Source offsets, line counts and angle-bracket runs fit the target integer because SwiftLexer::extract enforces the 8 MiB input ceiling."
    )]
    #[allow(
        clippy::cast_possible_wrap,
        reason = "Angle-bracket runs and accumulated depth cannot exceed the 8 MiB file ceiling, well within i32."
    )]
    fn specialization(&self, first: usize, end: usize) -> usize {
        if self.text(first) != "<" {
            return first;
        }
        let mut depth = 0i32;
        for i in first..end {
            let text = self.text(i);
            if text.chars().all(|c| c == '<') {
                depth += text.len() as i32;
            } else if text.chars().all(|c| c == '>') {
                depth -= text.len() as i32;
            } else if !(self.ident(i)
                || matches!(text, "," | "." | "?" | "!" | "[" | "]" | ":" | "&"))
            {
                return first;
            }
            if depth == 0 {
                return i + 1;
            }
            if depth < 0 {
                return first;
            }
        }
        first
    }
}

fn non_callee(word: &str) -> bool {
    matches!(
        word,
        "if" | "while"
            | "switch"
            | "for"
            | "catch"
            | "return"
            | "throw"
            | "guard"
            | "else"
            | "case"
            | "let"
            | "var"
            | "func"
            | "in"
            | "as"
            | "is"
            | "try"
            | "await"
            | "some"
            | "any"
            | "get"
            | "set"
            | "didSet"
            | "willSet"
            | "true"
            | "false"
            | "nil"
    )
}
