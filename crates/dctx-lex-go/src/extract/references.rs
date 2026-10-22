//! Call, selector and type-name references inside already-recognised spans.
//! Implements SPEC-MAP-MODE 5.3 calls and uses-type. Index-versus-instantiation
//! (`F[T](x)`) and bare field selectors are abstained. Called by declaration parsing.
use super::{compact, Walker};
use crate::lexer::{keyword, Kind};
use dctx_map_model::EdgeKind;

impl Walker<'_> {
    /// Walks a type starting at `index`. Named types, map keys and type arguments
    /// are emitted when `emit` is set. A slice or array bracket is a length, not a type.
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    pub(crate) fn skip_type(
        &mut self,
        mut index: usize,
        end: usize,
        emit: bool,
        owner: usize,
        scope: &str,
    ) -> usize {
        let limit = self.toks.len().saturating_add(1) as u32;
        let mut steps = 0_u32;
        while index < end && steps < limit {
            steps += 1;
            if self.is(index, "*") || self.is(index, "<-") || self.is(index, "...") {
                index += 1;
                if self.is(index, "chan") {
                    index += 1;
                }
                continue;
            }
            if self.is(index, "[") {
                index = self.after(index).min(end);
                continue;
            }
            if self.is(index, "chan") {
                index += 1;
                if self.is(index, "<-") {
                    index += 1;
                }
                continue;
            }
            if self.is(index, "map") {
                index += 1;
                if self.is(index, "[") {
                    let close = self.close_of(index, end);
                    let _ = self.skip_type(index + 1, close, emit, owner, scope);
                    index = self.after(index).min(end);
                }
                continue;
            }
            if self.is(index, "struct") || self.is(index, "interface") {
                index += 1;
                if self.is(index, "{") {
                    index = self.after(index).min(end);
                }
                return index;
            }
            if self.is(index, "func") {
                index += 1;
                if self.is(index, "[") {
                    index = self.after(index).min(end);
                }
                if self.is(index, "(") {
                    index = self.after(index).min(end);
                }
                if self.is(index, "(") {
                    index = self.after(index).min(end);
                }
                continue;
            }
            if self.ident(index) && !keyword(self.text(index)) {
                return self.named_type(index, end, emit, owner, scope);
            }
            return index;
        }
        index
    }

    fn named_type(&mut self, start: usize, end: usize, emit: bool, owner: usize, scope: &str) -> usize {
        let mut index = start + 1;
        while index < end && self.is(index, ".") && self.ident(index + 1) && !keyword(self.text(index + 1))
        {
            index += 2;
        }
        let name_end = index;
        if emit {
            let bytes = self.src.get(self.start_byte(start) as usize..self.end_byte(name_end - 1) as usize);
            let spelling = compact(bytes.unwrap_or(""));
            if !spelling.is_empty() {
                self.refer(owner, scope, EdgeKind::UsesType, spelling, None, (start, name_end - 1));
            }
        }
        if self.is(index, "[") {
            let close = self.close_of(index, end);
            self.type_arguments(index + 1, close, emit, owner, scope);
            index = self.after(index).min(end);
        }
        index
    }

    /// Comma-separated types inside an instantiation. `end` is the closing bracket.
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    fn type_arguments(&mut self, mut index: usize, end: usize, emit: bool, owner: usize, scope: &str) {
        let limit = self.toks.len().saturating_add(1) as u32;
        let mut steps = 0_u32;
        while index < end && steps < limit {
            steps += 1;
            if self.is(index, ",") || self.is_semi(index) {
                index += 1;
                continue;
            }
            let next = self.skip_type(index, end, emit, owner, scope);
            index = if next > index { next } else { index + 1 };
        }
    }

    /// Parameter or field list between an opener and its closer, exclusive.
    pub(crate) fn scan_params(&mut self, open: usize, close: usize, owner: usize, scope: &str) {
        let mut index = open + 1;
        while index < close {
            if self.is(index, ",") || self.is_semi(index) {
                index += 1;
                continue;
            }
            let next = self.scan_one_field(index, close, owner, scope, false);
            index = next.max(index + 1);
        }
    }

    /// One struct field or parameter. `strict` records a diagnostic when the field is not a type.
    pub(crate) fn scan_one_field(
        &mut self,
        index: usize,
        end: usize,
        owner: usize,
        scope: &str,
        strict: bool,
    ) -> usize {
        if index >= end {
            return index;
        }
        let type_at = self.split_names(index, end);
        let mut cursor = type_at.unwrap_or(index);
        if self.is(cursor, "...") {
            cursor += 1;
        }
        if type_at.is_none() && !self.type_starts(cursor) {
            if strict {
                self.problem(self.start_byte(index), crate::domain::DiagnosticReason::Unrecognized);
            }
            return self.skip_element(index, end);
        }
        let after_type = self.skip_type(cursor, end, true, owner, scope);
        let mut next = if after_type > cursor { after_type } else { cursor + 1 };
        while matches!(self.kind(next), Some(Kind::Str | Kind::Raw)) {
            next += 1;
        }
        next
    }

    pub(crate) fn scan_fields(&mut self, open: usize, close: usize, owner: usize, scope: &str) {
        let mut index = open + 1;
        while index < close {
            index = self.skip_semis(index, close);
            if index >= close {
                break;
            }
            let next = self.scan_one_field(index, close, owner, scope, true);
            if next <= index {
                break;
            }
            index = next;
        }
    }

    /// Where a type starts after a name list, if the idents are names rather than types.
    pub(crate) fn split_names(&self, index: usize, end: usize) -> Option<usize> {
        if !self.can_be_name(index) {
            return None;
        }
        let mut cursor = index;
        loop {
            cursor += 1;
            if cursor < end && self.is(cursor, ",") && self.can_be_name(cursor + 1) {
                cursor += 1;
                continue;
            }
            break;
        }
        (cursor < end && (self.is(cursor, "...") || self.type_starts(cursor))).then_some(cursor)
    }

    pub(crate) fn skip_element(&self, mut index: usize, end: usize) -> usize {
        while index < end && !self.is_semi(index) && !self.is(index, ")") && !self.is(index, "}") {
            if matches!(self.text(index), "(" | "[" | "{") {
                index = self.after(index).min(end);
            } else {
                index += 1;
            }
        }
        index
    }

    /// End of an expression list: a semicolon, or a bracket that closes an outer group.
    pub(crate) fn skip_expr(&self, start: usize, end: usize) -> usize {
        let mut index = start;
        while index < end {
            if self.is_semi(index) {
                return index;
            }
            if matches!(self.text(index), ")" | "]" | "}")
                && let Some(partner) = self.partner.get(index).copied()
                && partner != crate::lexer::NONE
                && (partner as usize) < start
            {
                return index;
            }
            if matches!(self.text(index), "(" | "[" | "{") {
                index = self.after(index).min(end);
                continue;
            }
            index += 1;
        }
        index
    }

    pub(crate) fn scan_result(&mut self, index: usize, end: usize, owner: usize, scope: &str) {
        if index >= end || self.is(index, "{") || self.is_semi(index) {
            return;
        }
        if self.is(index, "(") {
            self.scan_params(index, self.close_of(index, end), owner, scope);
            return;
        }
        let _ = self.skip_type(index, end, true, owner, scope);
    }

    /// Calls whose callee is an ident or selector chain, plus composite-literal types.
    pub(crate) fn scan_calls(&mut self, mut index: usize, end: usize, owner: usize, scope: &str) {
        while index < end {
            if !self.can_be_name(index) {
                index += 1;
                continue;
            }
            let start = index;
            let mut cursor = index + 1;
            while cursor < end && self.is(cursor, ".") && self.can_be_name(cursor + 1) {
                cursor += 2;
            }
            if self.is(cursor, "(") {
                let spelling = compact(self.span(start, cursor));
                let close = self.close_of(cursor, end);
                self.refer(owner, scope, EdgeKind::Calls, spelling, None, (start, close));
                index = cursor + 1;
                continue;
            }
            if self.is(cursor, "{") && !self.prev_control(start) {
                let spelling = compact(self.span(start, cursor));
                self.refer(owner, scope, EdgeKind::UsesType, spelling, None, (start, cursor - 1));
            }
            index = cursor;
        }
    }

    fn prev_control(&self, index: usize) -> bool {
        index > 0 && matches!(self.text(index - 1), "if" | "for" | "switch" | "select" | "else")
    }

    fn span(&self, start: usize, end_exclusive: usize) -> &str {
        if end_exclusive == 0 {
            return "";
        }
        let from = self.start_byte(start) as usize;
        let to = self.end_byte(end_exclusive - 1) as usize;
        self.src.get(from..to).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use crate::GoLexer;
    use dctx_map_model::EdgeKind;

    #[test]
    fn calls_keep_selectors_and_strings_do_not_invent_them() {
        let source = "package p\nfunc f() {\n\tfmt.Println(a)\n\t_ = \"func Hidden()\"\n}\n";
        let facts = GoLexer::default().extract("p.go", source.as_bytes()).unwrap();
        let calls: Vec<_> = facts
            .references
            .iter()
            .filter(|reference| reference.fact.kind == EdgeKind::Calls)
            .map(|reference| reference.fact.name.as_str())
            .collect();
        assert_eq!(calls, ["fmt.Println"]);
        assert!(facts.nodes.iter().all(|node| node.fact.name != "Hidden"));
    }
}
