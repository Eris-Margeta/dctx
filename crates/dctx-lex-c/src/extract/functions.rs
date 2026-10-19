//! Functions, methods, operators and declarator names.
//! Implements SPEC-MAP-MODE 5.1 signatures. A name that is a macro is abstained.
//! File-scope variables are symbols with no symbol kind; function bodies are not.
//! Out-of-line `Type::name` definitions stay methods of the enclosing scope.
use super::{Item, Region, Scope, Walker};
use crate::domain::DiagnosticReason;
use crate::lexer::{keyword, opens_template_args, AngleDepth, Kind};
use dctx_map_model::SymbolKind;

pub(crate) struct FoundFn {
    name_lo: usize,
    name_hi: usize,
    bare_at: usize,
    local: String,
    close: usize,
    term: usize,
}

impl Walker<'_> {
    pub(crate) fn func_or_var(&mut self, start: usize, end: usize, scope: &Scope) -> usize {
        if let Some(func) = self.find_func(start, end) {
            if self.known_macro(self.text(func.bare_at)) {
                self.problem(self.start_of(func.bare_at), DiagnosticReason::HiddenByMacro);
                return self.func_resume(&func, end);
            }
            let is_method = scope.region == Region::Type || func.local.contains("::");
            let last = if self.is(func.term, "{") {
                self.close(func.term).unwrap_or(func.term)
            } else {
                func.term
            };
            let body = if self.is(func.term, "{") {
                (self.start_of(func.term), self.end_of(last))
            } else {
                (self.start_of(start), self.end_of(func.term))
            };
            let item = Item {
                symbol: Some(if is_method { SymbolKind::Method } else { SymbolKind::Function }),
                local: func.local.clone(),
                first: start,
                last: last.min(self.toks.len().saturating_sub(1)),
                sig_end: self.start_of(func.term),
                body,
                public: self.decl_public(start, func.bare_at, scope),
            };
            if let Some(inner) = self.emit(item, scope) {
                self.emit_types(start, func.close + 1, func.name_lo, func.name_hi, scope);
                if self.is(func.term, "{") {
                    let close = self.close(func.term).unwrap_or(last);
                    self.scan_range(func.term + 1, close.min(end), &inner);
                }
            }
            return self.func_resume(&func, end);
        }
        let stop = self.consume_semi(start, end);
        let names = self.decl_names(start, stop);
        if names.is_empty() {
            self.note_skipped(start, stop);
            self.scan_range(start, stop, scope);
            return stop;
        }
        for (at, bare) in &names {
            if self.known_macro(bare) {
                self.problem(self.start_of(*at), DiagnosticReason::HiddenByMacro);
                continue;
            }
            let last = stop.saturating_sub(1).max(start);
            let item = Item {
                symbol: None,
                local: bare.clone(),
                first: start,
                last: last.min(self.toks.len().saturating_sub(1)),
                sig_end: self.start_of(last),
                body: (self.start_of(start), self.end_of(last)),
                public: self.decl_public(start, *at, scope),
            };
            let _ = self.emit(item, scope);
        }
        self.emit_types(start, stop, usize::MAX, usize::MAX, scope);
        self.scan_range(start, stop, scope);
        stop
    }

    fn func_resume(&self, func: &FoundFn, end: usize) -> usize {
        if self.is(func.term, "{") {
            self.after(func.term).min(end)
        } else {
            (func.term + 1).min(end)
        }
    }

    fn find_func(&self, start: usize, end: usize) -> Option<FoundFn> {
        let mut index = start;
        let mut depth = 0i32;
        let mut found = None;
        while index < end && depth >= 0 {
            if self.starts_attribute(index) {
                let next = self.skip_attributes(index, end);
                if next > index {
                    index = next;
                    continue;
                }
            }
            if depth == 0 && self.is(index, ";") {
                return found.filter(|func: &FoundFn| func.term <= index);
            }
            if depth == 0 && self.is(index, "{") {
                return found.filter(|func: &FoundFn| func.term == index);
            }
            if depth == 0
                && self.is(index, "operator")
                && let Some(func) = self.operator_func(index, end)
            {
                found = Some(func);
            } else if depth == 0 && self.is(index, "(") && let Some(func) = self.func_at(index, end) {
                let keep_operator = found
                    .as_ref()
                    .is_some_and(|found| self.is(found.bare_at, "operator"));
                if !keep_operator {
                    found = Some(func);
                }
            }
            if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                depth += 1;
            } else if self.is(index, ")") || self.is(index, "]") || self.is(index, "}") {
                depth -= 1;
            }
            index += 1;
        }
        found
    }

    fn func_at(&self, paren: usize, end: usize) -> Option<FoundFn> {
        let bare_at = self.declarator_before(paren)?;
        if keyword(self.text(bare_at)) || self.text(bare_at) == "operator" {
            return None;
        }
        let (name_lo, local) = self.qualify(bare_at);
        let close = self.close(paren)?;
        let term = self.postfix_term(close + 1, end)?;
        Some(FoundFn {
            name_lo,
            name_hi: bare_at + 1,
            bare_at,
            local,
            close,
            term,
        })
    }

    fn declarator_before(&self, paren: usize) -> Option<usize> {
        let mut index = paren.checked_sub(1)?;
        loop {
            if self.is(index, "*") || self.is(index, "&") || self.is(index, "&&") || self.is(index, "^") {
                if index == 0 {
                    return None;
                }
                index -= 1;
                continue;
            }
            if self.ident(index) && matches!(self.text(index), "const" | "volatile") {
                if index == 0 {
                    return None;
                }
                index -= 1;
                continue;
            }
            break;
        }
        if self.is(index, ">") {
            let lt = self.find_lt_back(index)?;
            index = lt.checked_sub(1)?;
        }
        self.ident(index).then_some(index)
    }

    fn qualify(&self, bare_at: usize) -> (usize, String) {
        let tilde = bare_at > 0 && self.is(bare_at - 1, "~");
        let bare_text = self.text(bare_at);
        let bare = if tilde {
            format!("dtor_{bare_text}")
        } else {
            bare_text.to_string()
        };
        let mut parts = vec![bare];
        let mut start = if tilde { bare_at - 1 } else { bare_at };
        while start > 0 && self.is(start - 1, "::") {
            let mut name_at = start - 1;
            if name_at == 0 {
                break;
            }
            name_at -= 1;
            if self.is(name_at, ">") {
                let Some(lt) = self.find_lt_back(name_at) else {
                    break;
                };
                if lt == 0 {
                    break;
                }
                name_at = lt - 1;
            }
            if !self.can_name(name_at) {
                break;
            }
            parts.push(self.text(name_at).to_string());
            start = name_at;
        }
        parts.reverse();
        (start, parts.join("::"))
    }

    fn operator_func(&self, op: usize, end: usize) -> Option<FoundFn> {
        let mut index = op + 1;
        let mut spelling = String::from("operator");
        if self.is(index, "new") || self.is(index, "delete") {
            spelling.push(' ');
            spelling.push_str(self.text(index));
            index += 1;
            if self.is(index, "[") && self.is(index + 1, "]") {
                spelling.push_str("[]");
                index += 2;
            }
        } else if self.is(index, "(") && self.close(index).is_some_and(|close| self.is(close + 1, "(")) {
            spelling.push_str("()");
            index = self.close(index)? + 1;
        } else if self.is(index, "[") && self.is(index + 1, "]") {
            spelling.push_str("[]");
            index += 2;
        } else if self.kind(index) == Some(Kind::String) || self.kind(index) == Some(Kind::Punct) {
            spelling.push_str(self.text(index));
            index += 1;
            if self.kind(index - 1) == Some(Kind::String)
                && self.ident(index)
                && self.text(index).starts_with('_')
            {
                spelling.push_str(self.text(index));
                index += 1;
            }
        } else {
            return None;
        }
        if spelling.contains('~') {
            spelling = spelling.replace('~', "_tilde");
        }
        if spelling.contains('#') || !self.is(index, "(") {
            return None;
        }
        let close = self.close(index)?;
        let term = self.postfix_term(close + 1, end)?;
        Some(FoundFn {
            name_lo: op,
            name_hi: index,
            bare_at: op,
            local: spelling,
            close,
            term,
        })
    }

    fn postfix_term(&self, mut index: usize, end: usize) -> Option<usize> {
        while index < end {
            if self.starts_attribute(index) {
                let next = self.skip_attributes(index, end);
                if next > index {
                    index = next;
                    continue;
                }
            }
            if self.is(index, "{") || self.is(index, ";") {
                return Some(index);
            }
            if self.is(index, "requires") {
                index += 1;
                while index < end && !self.is(index, "{") && !self.is(index, ";") {
                    if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                        index = self.after(index);
                    } else {
                        index += 1;
                    }
                }
                continue;
            }
            if self.is(index, ",") || self.is(index, "}") {
                return None;
            }
            if matches!(self.text(index), "noexcept" | "throw" | "alignas" | "_Alignas") {
                index += 1;
                if self.is(index, "(") {
                    index = self.after(index);
                }
                continue;
            }
            if self.is(index, "[") && self.is(index + 1, "[") {
                index = self.after(index);
                continue;
            }
            if self.is(index, "=") {
                let next = index + 1;
                if matches!(self.text(next), "default" | "delete") || self.kind(next) == Some(Kind::Number) {
                    index = next + 1;
                    continue;
                }
                return None;
            }
            if self.is(index, ":") {
                index += 1;
                while index < end && !self.is(index, "{") && !self.is(index, ";") {
                    if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                        index = self.after(index);
                    } else {
                        index += 1;
                    }
                }
                continue;
            }
            if self.is(index, "->") {
                index += 1;
                while index < end && !self.is(index, "{") && !self.is(index, ";") && !self.is(index, ",") {
                    if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                        index = self.after(index);
                    } else {
                        index += 1;
                    }
                }
                continue;
            }
            if matches!(self.text(index), "&" | "&&" | "*" | "try")
                || (self.ident(index)
                    && matches!(
                        self.text(index),
                        "const" | "volatile" | "override" | "final" | "mutable" | "constexpr" | "consteval"
                    ))
                || (self.ident(index) && self.known_macro(self.text(index)))
            {
                index += 1;
                if self.is(index, "(") {
                    index = self.after(index);
                }
                continue;
            }
            return None;
        }
        None
    }

    pub(crate) fn decl_names(&self, start: usize, end: usize) -> Vec<(usize, String)> {
        self.decl_names_forced(start, end, false)
    }

    /// `force_single` accepts one declarator after a type body (`struct S { } name;`).
    pub(crate) fn decl_names_forced(
        &self,
        start: usize,
        end: usize,
        force_single: bool,
    ) -> Vec<(usize, String)> {
        let end = end.min(self.toks.len());
        let mut names = Vec::new();
        let mut allow_single = force_single || self.type_cue(start, end);
        let mut segment = start;
        let mut index = start;
        let mut nest = AngleDepth::new();
        while index < end {
            if nest.at_statement_level() && (self.is(index, ",") || self.is(index, ";")) {
                if let Some(name) = self.segment_name(segment, index, allow_single) {
                    allow_single = true;
                    names.push(name);
                }
                segment = index + 1;
            } else {
                self.observe_nest(index, end, &mut nest);
            }
            index += 1;
        }
        if segment < end
            && let Some(name) = self.segment_name(segment, end, allow_single)
        {
            names.push(name);
        }
        names
    }

    fn type_cue(&self, start: usize, end: usize) -> bool {
        let mut index = start;
        let mut depth = 0i32;
        while index < end.min(self.toks.len()) {
            if self.is(index, "{") || self.is(index, "(") || self.is(index, "[") {
                depth += 1;
            } else if self.is(index, "}") || self.is(index, ")") || self.is(index, "]") {
                depth -= 1;
            } else if depth == 0
                && (self.is(index, "*")
                    || (self.ident(index) && crate::lexer::type_word(self.text(index))))
            {
                return true;
            }
            index += 1;
        }
        false
    }

    fn segment_name(&self, start: usize, end: usize, allow_single: bool) -> Option<(usize, String)> {
        let mut limit = end;
        let mut nest = AngleDepth::new();
        for index in start..end.min(self.toks.len()) {
            if nest.at_statement_level() && matches!(self.text(index), "=" | ":" | "{") {
                limit = index;
                break;
            }
            self.observe_nest(index, end, &mut nest);
        }
        let mut chosen = None;
        let mut idents = 0;
        let mut index = start;
        let mut nest = AngleDepth::new();
        while index < limit {
            if self.can_name(index) && !self.known_macro(self.text(index)) && nest.nest() <= 1 {
                let preceded = index > start && self.is(index - 1, "::");
                let followed = self.is(index + 1, "::");
                if !preceded && !followed {
                    idents += 1;
                    chosen = Some(index);
                }
            }
            self.observe_nest(index, end, &mut nest);
            index += 1;
        }
        let at = chosen?;
        if !allow_single && idents < 2 && !self.type_cue(start, end) {
            return None;
        }
        Some((at, self.text(at).to_string()))
    }

    fn observe_nest(&self, index: usize, end: usize, nest: &mut AngleDepth) {
        let spelling = self.text(index);
        let opens = spelling == "<" && opens_template_args(|at| self.text(at), end, index);
        nest.observe(spelling, opens);
    }
}

#[cfg(test)]
mod tests {
    use crate::CLexer;
    use dctx_map_model::SymbolKind;

    #[test]
    fn methods_operators_and_macro_names_abstain() {
        let src = b"struct Box { Box(); ~Box(); bool operator==(const Box&) const; };\n#define NAME name\nint NAME(void);\n";
        let facts = CLexer::default().extract("m.hpp", src).unwrap();
        let names: Vec<_> = facts.nodes.iter().map(|n| n.fact.name.as_str()).collect();
        assert!(names.contains(&"Box::Box"), "{names:?}");
        assert!(names.contains(&"Box::dtor_Box"), "{names:?}");
        assert!(names.contains(&"Box::operator=="), "{names:?}");
        assert_eq!(names.iter().filter(|name| **name == "NAME").count(), 1);
        assert!(facts.nodes.iter().any(|n| n.fact.name == "NAME" && n.fact.symbol_kind == Some(SymbolKind::Macro)));
        assert!(facts.diagnostics.iter().any(|d| d.reason == crate::DiagnosticReason::HiddenByMacro));
    }
}
