// SPDX-License-Identifier: Apache-2.0
//! Reference and type scans: calls, uses-type, angles and statement ends.
//! Call names keep `.` for both `.` and `?.`. Bare reads and `::` abstain.
//! Angle brackets close only when the interior still looks like a type.
use super::{Scope, Walker, decl_word, is_modifier};
use crate::lexer::keyword;
use dctx_map_model::EdgeKind;

impl Walker<'_> {
    pub(crate) fn scan_refs(&mut self, start: usize, end: usize, scope: &Scope) {
        let mut index = start;
        let mut guard = 0usize;
        while index < end && guard <= end {
            guard += 1;
            if self.is(index, "@") {
                index = self.scan_annotation_use(index, end, scope);
                continue;
            }
            if self.is(index, "::") {
                index = self.skip_callable_ref(index, end);
                continue;
            }
            if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                index += 1;
                continue;
            }
            if self.call_start(index) && !self.prev_is_fun(index) {
                let next = self.maybe_call(index, end, scope);
                index = next.max(index + 1);
                continue;
            }
            index += 1;
        }
    }

    fn prev_is_fun(&self, index: usize) -> bool {
        index > 0 && self.is(index - 1, "fun")
    }

    fn scan_annotation_use(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        let ann = self.read_ann(index, end);
        if !ann.name.is_empty() {
            let last = ann.to.saturating_sub(1).max(ann.at);
            self.refer(scope, EdgeKind::UsesType, ann.name, None, ann.at, last);
        }
        if let Some(args) = ann.args {
            let close = self.matched(args, end).unwrap_or(args);
            if close > args {
                self.scan_refs(args + 1, close, scope);
            }
        }
        ann.to.max(index + 1)
    }

    fn skip_callable_ref(&self, index: usize, end: usize) -> usize {
        let next = index + 1;
        if next < end && (self.is_decl_name(next) || self.ident(next)) {
            next + 1
        } else {
            next
        }
    }

    fn maybe_call(&mut self, start: usize, end: usize, scope: &Scope) -> usize {
        let mut name = self.bare(start);
        let mut index = start + 1;
        let mut last = start;
        loop {
            if self.is(index, "<") {
                let Some(close) = self.generic_close(index, end) else {
                    break;
                };
                index = close + 1;
            }
            if self.is(index, ".") {
                let member = index + 1;
                if !self.chain_member(member) {
                    break;
                }
                name.push('.');
                name.push_str(&self.bare(member));
                last = member;
                index = member + 1;
                continue;
            }
            if self.is(index, "?") && self.is(index + 1, ".") {
                let member = index + 2;
                if !self.chain_member(member) {
                    break;
                }
                name.push('.');
                name.push_str(&self.bare(member));
                last = member;
                index = member + 1;
                continue;
            }
            break;
        }
        if self.is(index, "<")
            && let Some(close) = self.generic_close(index, end)
        {
            index = close + 1;
        }
        if index < end && (self.is(index, "(") || self.is(index, "{")) {
            self.refer(scope, EdgeKind::Calls, name, None, start, last);
        }
        index.max(start + 1)
    }

    fn chain_member(&self, index: usize) -> bool {
        self.call_start(index) || self.is_decl_name(index)
    }

    fn generic_close(&self, open: usize, end: usize) -> Option<usize> {
        let close = self.angles_end(open, end)?;
        let mut index = open + 1;
        while index < close {
            if self.is(index, "<") {
                let inner = self.angles_end(index, close + 1)?;
                index = inner + 1;
                continue;
            }
            if !self.type_token(index) {
                return None;
            }
            index += 1;
        }
        let after = close + 1;
        (self.is(after, "(") || self.is(after, ".") || self.is(after, "{")).then_some(close)
    }

    fn type_token(&self, index: usize) -> bool {
        if self.ident(index) {
            return true;
        }
        matches!(
            self.text(index),
            "." | "," | "?" | ":" | "*" | "(" | ")" | "[" | "]" | "->" | "<" | ">"
        )
    }

    pub(crate) fn angles_end(&self, open: usize, end: usize) -> Option<usize> {
        if !self.is(open, "<") {
            return None;
        }
        let mut depth = 0i32;
        let mut index = open;
        while index < end {
            if self.is(index, "{") || self.is(index, "}") || self.is_semi(index) {
                return None;
            }
            if self.is(index, "<") {
                depth += 1;
            } else if self.is(index, ">") {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            index += 1;
        }
        None
    }

    pub(crate) fn expr_end(&self, start: usize, end: usize) -> usize {
        let mut index = start;
        while index < end {
            if self.expr_stop(index) {
                return index;
            }
            if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                index = self.after(index).max(index + 1);
                continue;
            }
            index += 1;
        }
        index
    }

    fn expr_stop(&self, index: usize) -> bool {
        if self.is_semi(index) || self.is(index, "}") || self.is(index, ")") || self.is(index, "]")
        {
            return true;
        }
        self.ident(index)
            && matches!(
                self.text(index),
                "fun"
                    | "class"
                    | "interface"
                    | "object"
                    | "typealias"
                    | "package"
                    | "import"
                    | "constructor"
                    | "val"
                    | "var"
            )
    }

    pub(crate) fn type_tail(&self, start: usize, end: usize) -> (usize, u32) {
        let mut index = start;
        let mut last = start;
        let mut started = false;
        let mut guard = 0usize;
        while index < end && guard <= end {
            guard += 1;
            if self.type_stop(index) {
                break;
            }
            if self.is(index, "(") || self.is(index, "[") {
                let next = self.after(index).min(end);
                last = next.saturating_sub(1).max(index);
                started = true;
                index = next.max(index + 1);
                if self.is(index, "->") {
                    index += 1;
                }
                continue;
            }
            if self.is(index, "?") || self.is(index, "*") || self.is(index, "->") {
                if started || !self.is(index, "->") {
                    last = index;
                    started = true;
                }
                index += 1;
                continue;
            }
            if !self.type_name(index) {
                break;
            }
            last = index;
            started = true;
            index += 1;
            index = self.finish_type_name(index, end, &mut last);
        }
        let byte = if started {
            self.end_byte(last)
        } else {
            self.start_byte(start)
        };
        (index, byte)
    }

    fn finish_type_name(&self, mut index: usize, end: usize, last: &mut usize) -> usize {
        while self.is(index, ".") && self.type_name(index + 1) {
            *last = index + 1;
            index += 2;
        }
        if self.is(index, "<")
            && let Some(close) = self.angles_end(index, end)
        {
            *last = close;
            index = close + 1;
        }
        if self.is(index, "?") {
            *last = index;
            index += 1;
        }
        index
    }

    fn type_name(&self, index: usize) -> bool {
        if !self.ident(index) || self.type_stop(index) {
            return false;
        }
        let word = self.text(index);
        word.starts_with('`') || !keyword(word) || word == "typeof"
    }

    fn type_stop(&self, index: usize) -> bool {
        if self.is_semi(index) || matches!(self.text(index), "=" | "{" | "}" | "," | ")" | "]") {
            return true;
        }
        if !self.ident(index) {
            return false;
        }
        let word = self.text(index);
        matches!(word, "where" | "by" | "get" | "set") || decl_word(word)
    }

    pub(crate) fn colon_types(&mut self, start: usize, end: usize, scope: &Scope) {
        let mut index = start;
        let mut guard = 0usize;
        while index < end && guard <= end {
            guard += 1;
            if self.is(index, "<")
                && let Some(close) = self.angles_end(index, end)
            {
                self.colon_types(index + 1, close, scope);
                index = close + 1;
                continue;
            }
            if self.is(index, ":") {
                let (next, _) = self.type_tail(index + 1, end);
                if next > index + 1 {
                    self.emit_type(index + 1, next, scope);
                }
                index = next.max(index + 1);
                continue;
            }
            index += 1;
        }
    }

    pub(crate) fn emit_type(&mut self, start: usize, end: usize, scope: &Scope) {
        let mut index = start;
        let mut guard = 0usize;
        while index < end && guard <= end {
            guard += 1;
            if self.is(index, "<")
                && let Some(close) = self.angles_end(index, end.min(self.toks.len()))
            {
                self.emit_type_args(index + 1, close, scope);
                index = close + 1;
                continue;
            }
            if self.is(index, "(") || self.is(index, "[") {
                let next = self.after(index).min(end.max(index + 1));
                self.emit_type(index + 1, next.saturating_sub(1), scope);
                index = next.max(index + 1);
                continue;
            }
            if self.variance(index) {
                index += 1;
                continue;
            }
            if self.type_name(index) {
                let (name, next) = self.read_dotted(index, end);
                if !name.is_empty() && next > index {
                    self.refer(scope, EdgeKind::UsesType, name, None, index, next - 1);
                }
                index = next.max(index + 1);
                continue;
            }
            index += 1;
        }
    }

    pub(crate) fn emit_type_args(&mut self, start: usize, end: usize, scope: &Scope) {
        let mut index = start;
        let mut part = start;
        let mut depth = 0i32;
        while index < end {
            if self.is(index, "<") || self.is(index, "(") || self.is(index, "[") {
                depth += 1;
            } else if self.is(index, ">") || self.is(index, ")") || self.is(index, "]") {
                depth -= 1;
            } else if depth == 0 && self.is(index, ",") {
                self.emit_type(part, index, scope);
                part = index + 1;
            }
            index += 1;
        }
        if part < end {
            self.emit_type(part, end, scope);
        }
    }

    fn variance(&self, index: usize) -> bool {
        self.ident(index) && matches!(self.text(index), "in" | "out" | "reified" | "suspend")
    }

    pub(crate) fn emit_receiver(&mut self, start: usize, name: usize, scope: &Scope) {
        if start < name {
            self.emit_type(start, name, scope);
        }
    }

    pub(crate) fn read_dotted(&self, start: usize, end: usize) -> (String, usize) {
        if !self.type_name(start) && !self.is_decl_name(start) {
            return (String::new(), start);
        }
        let mut name = self.bare(start);
        let mut index = start + 1;
        while index < end && self.is(index, ".") {
            let next = index + 1;
            if self.is(next, "*") {
                name.push_str(".*");
                return (name, next + 1);
            }
            if !self.type_name(next) && !self.is_decl_name(next) {
                break;
            }
            name.push('.');
            name.push_str(&self.bare(next));
            index = next + 1;
        }
        (name, index)
    }

    pub(crate) fn recover_open(&self, open: usize, end: usize) -> usize {
        let mut index = open + 1;
        let mut paren = 1i32;
        let mut brace = 0i32;
        while index < end {
            if brace == 0 && paren > 0 && self.decl_at(index) {
                return index;
            }
            if brace == 0 && self.is(index, "}") {
                return index;
            }
            if self.is(index, "(") {
                paren += 1;
            } else if self.is(index, ")") {
                paren -= 1;
                if paren == 0 {
                    return index + 1;
                }
            } else if self.is(index, "{") {
                brace += 1;
            } else if self.is(index, "}") && brace > 0 {
                brace -= 1;
            }
            index += 1;
        }
        index.min(end)
    }

    pub(crate) fn clause_end(&self, start: usize, end: usize) -> usize {
        let mut index = start + 1;
        let mut depth = 0i32;
        while index < end {
            if depth == 0
                && (self.is(index, "{")
                    || self.is(index, "=")
                    || self.is_semi(index)
                    || self.decl_at(index))
            {
                return index;
            }
            if self.is(index, "<")
                && depth == 0
                && let Some(close) = self.angles_end(index, end)
            {
                index = close + 1;
                continue;
            }
            if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                depth += 1;
            } else if self.is(index, ")") || self.is(index, "]") || self.is(index, "}") {
                depth = depth.saturating_sub(1);
            }
            index += 1;
        }
        index
    }

    pub(crate) fn param_end(&self, start: usize, close: usize) -> usize {
        let mut index = start;
        let mut depth = 0i32;
        while index < close {
            if depth == 0 && (self.is(index, ",") || self.is_semi(index)) {
                return index;
            }
            if depth == 0
                && self.is(index, "<")
                && let Some(end) = self.angles_end(index, close)
            {
                index = end + 1;
                continue;
            }
            if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                depth += 1;
            } else if self.is(index, ")") || self.is(index, "]") || self.is(index, "}") {
                if depth == 0 {
                    return index;
                }
                depth -= 1;
            }
            index += 1;
        }
        index
    }

    pub(crate) fn resume(&self, index: usize, end: usize) -> usize {
        if index >= end || self.is(index, "}") || self.decl_at(index) {
            return index.min(end);
        }
        let stop = self.expr_end(index, end);
        let stop = if stop <= index {
            (index + 1).min(end)
        } else {
            stop.min(end)
        };
        if stop < end && (self.is(stop, "}") || self.decl_at(stop)) {
            stop
        } else {
            self.finish_stmt(stop, end)
        }
    }

    pub(crate) fn dot_between(&self, start: usize, end: usize) -> bool {
        (start..end).any(|index| self.is(index, "."))
    }

    pub(crate) fn modifier_word(&self, index: usize) -> bool {
        self.ident(index) && is_modifier(self.text(index))
    }
}

#[cfg(test)]
mod tests {
    use super::decl_word;
    use crate::lexer::keyword;

    #[test]
    fn variance_words_are_not_hard_keywords() {
        assert!(!keyword("out") && !keyword("reified") && decl_word("typealias"));
    }
}
