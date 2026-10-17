//! Type parameters, generic arguments and formal declarators.
//! Implements SPEC-MAP-MODE 5.1 type-parameter lists and 5.3 type-argument spans.
//! Depends on the walker cursor for brackets, annotations and depth limits.
//! Called from type declarations, methods, heritage clauses and parameter lists.

use super::{is_modifier_word, is_primitive, Scope, Walker};
use crate::domain::DiagnosticReason;

/// One formal or record component.
pub(super) struct Declarator {
    pub(super) type_from: usize,
    pub(super) type_to: usize,
    pub(super) name: usize,
    pub(super) annotations: Vec<String>,
}

impl Walker<'_> {
    pub(super) fn type_params(&mut self, open: usize, end: usize) -> (usize, Vec<String>, Vec<(usize, usize)>) {
        let mut cursor = open + 1;
        let mut depth = 1u32;
        let mut names = Vec::new();
        let mut bounds = Vec::new();
        let mut expect_name = true;
        let mut bound_from = None;
        while cursor < end && depth > 0 {
            if self.is_punct(cursor, "<") {
                if depth >= self.max_depth {
                    self.refuse_depth(cursor);
                    return (end, names, bounds);
                }
                depth += 1;
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, ">") {
                depth -= 1;
                if depth == 0 {
                    if let Some(from) = bound_from.take() {
                        bounds.push((from, cursor));
                    }
                    return (cursor + 1, names, bounds);
                }
                cursor += 1;
                continue;
            }
            if depth != 1 {
                cursor += 1;
                continue;
            }
            if self.is_word(cursor, "extends") || self.is_word(cursor, "super") {
                bound_from = Some(cursor + 1);
                expect_name = false;
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, "&") {
                if let Some(from) = bound_from.take() {
                    bounds.push((from, cursor));
                }
                bound_from = Some(cursor + 1);
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, ",") {
                if let Some(from) = bound_from.take() {
                    bounds.push((from, cursor));
                }
                expect_name = true;
                cursor += 1;
                continue;
            }
            if self.is_annotation(cursor) {
                let next = self.skip_annotation_tokens(cursor, end);
                cursor = if next > cursor { next } else { cursor + 1 };
                continue;
            }
            if expect_name && self.word(cursor) && !is_primitive(self.text(cursor)) {
                names.push(self.text(cursor).to_string());
                expect_name = false;
                cursor += 1;
                continue;
            }
            cursor += 1;
        }
        self.problem_at(open, DiagnosticReason::UnbalancedBracket);
        (cursor, names, bounds)
    }

    pub(crate) fn skip_one_type(&mut self, mut cursor: usize, end: usize) -> usize {
        while cursor < end && self.is_annotation(cursor) {
            let next = self.skip_annotation_tokens(cursor, end);
            if next <= cursor {
                break;
            }
            cursor = next;
        }
        if cursor >= end || !self.word(cursor) {
            return cursor;
        }
        cursor += 1;
        while cursor + 1 < end && self.is_punct(cursor, ".") && self.word(cursor + 1) {
            cursor += 2;
        }
        if cursor < end && self.is_punct(cursor, "<") {
            cursor = self.skip_angles(cursor, end);
        }
        while cursor < end && self.is_punct(cursor, "[") {
            match self.close(cursor) {
                Some(close) => cursor = close + 1,
                None => break,
            }
        }
        if cursor < end && self.is_punct(cursor, "...") {
            cursor += 1;
        }
        cursor
    }

    pub(crate) fn skip_angles(&mut self, open: usize, end: usize) -> usize {
        let mut cursor = open + 1;
        let mut depth = 1u32;
        while cursor < end && depth > 0 {
            if self.is_punct(cursor, "<") {
                if depth >= self.max_depth {
                    self.refuse_depth(cursor);
                    return end;
                }
                depth += 1;
            } else if self.is_punct(cursor, ">") {
                depth -= 1;
                if depth == 0 {
                    return cursor + 1;
                }
            } else if (self.is_punct(cursor, "(")
                || self.is_punct(cursor, "[")
                || self.is_punct(cursor, "{"))
                && let Some(close) = self.close(cursor)
            {
                cursor = close + 1;
                continue;
            }
            cursor += 1;
        }
        if depth != 0 {
            self.problem_at(open, DiagnosticReason::UnbalancedBracket);
        }
        cursor
    }

    pub(super) fn type_args_uses(&mut self, from: usize, to: usize, scope: &Scope) {
        let mut cursor = from;
        while cursor < to && !self.is_punct(cursor, "<") {
            cursor += 1;
        }
        if cursor >= to || !self.is_punct(cursor, "<") {
            return;
        }
        let close = self.matching_angle(cursor, to);
        if close > cursor + 1 {
            self.type_uses(cursor + 1, close, scope);
        }
    }

    fn matching_angle(&self, open: usize, end: usize) -> usize {
        let mut depth = 0i32;
        let mut cursor = open;
        while cursor < end {
            if self.is_punct(cursor, "<") {
                depth += 1;
            } else if self.is_punct(cursor, ">") {
                depth -= 1;
                if depth == 0 {
                    return cursor;
                }
            }
            cursor += 1;
        }
        end
    }

    pub(super) fn declarators(&self, from: usize, to: usize) -> Vec<Declarator> {
        let mut out = Vec::new();
        let mut cursor = from;
        let mut segment = from;
        let mut angle = 0i32;
        while cursor < to {
            if self.is_punct(cursor, "<") {
                angle += 1;
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, ">") && angle > 0 {
                angle -= 1;
                cursor += 1;
                continue;
            }
            if angle == 0
                && (self.is_punct(cursor, "(") || self.is_punct(cursor, "[") || self.is_punct(cursor, "{"))
                && let Some(close) = self.close(cursor)
            {
                cursor = (close + 1).min(to);
                continue;
            }
            if angle == 0 && self.is_punct(cursor, ",") {
                if let Some(declarator) = self.declarator_in(segment, cursor) {
                    out.push(declarator);
                }
                cursor += 1;
                segment = cursor;
                continue;
            }
            cursor += 1;
        }
        if let Some(declarator) = self.declarator_in(segment, to) {
            out.push(declarator);
        }
        out
    }

    fn declarator_in(&self, mut start: usize, end: usize) -> Option<Declarator> {
        let mut annotations = Vec::new();
        while start < end {
            if self.is_annotation(start) {
                annotations.push(self.annotation_name(start));
                let next = self.skip_annotation_tokens(start, end);
                start = if next > start { next } else { start + 1 };
                continue;
            }
            if self.word(start) && is_modifier_word(self.text(start)) {
                start += 1;
                continue;
            }
            break;
        }
        let type_from = start;
        let mut last_word = None;
        let mut cursor = start;
        let mut angle = 0i32;
        let mut prev_dot = false;
        while cursor < end {
            if self.is_punct(cursor, "<") {
                angle += 1;
                prev_dot = false;
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, ">") && angle > 0 {
                angle -= 1;
                prev_dot = false;
                cursor += 1;
                continue;
            }
            if angle > 0 {
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, "[")
                && let Some(close) = self.close(cursor)
            {
                cursor = (close + 1).min(end);
                prev_dot = false;
                continue;
            }
            if self.is_punct(cursor, ".") {
                prev_dot = true;
                cursor += 1;
                continue;
            }
            if self.is_punct(cursor, "...") {
                prev_dot = false;
                cursor += 1;
                continue;
            }
            if self.word(cursor) {
                if !prev_dot {
                    last_word = Some(cursor);
                }
                prev_dot = false;
                cursor += 1;
                continue;
            }
            prev_dot = false;
            cursor += 1;
        }
        let name = last_word?;
        if name <= type_from {
            return None;
        }
        Some(Declarator {
            type_from,
            type_to: name,
            name,
            annotations,
        })
    }
}
