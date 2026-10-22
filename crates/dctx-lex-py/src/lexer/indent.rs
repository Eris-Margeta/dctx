//! Logical indentation: spaces, tabs, blank lines and comments between statements.
//! Implements PARSERS-v1 indent and dedent tokens, including mixed-tab diagnostics.
//! The code scanner calls this at the beginning of each logical line.
use super::Lx;
use crate::domain::{DiagnosticReason, LexError};
use std::cmp::Ordering;

impl Lx<'_> {
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    pub(super) fn logical_indent(&mut self) -> Result<(), LexError> {
        loop {
            if self.i >= self.src.len() {
                return Ok(());
            }
            let at = self.i;
            let mut col = 0usize;
            let mut alt = 0usize;
            let mut saw_tab = false;
            let mut saw_space = false;
            loop {
                match self.peek() {
                    Some(' ') => {
                        col += 1;
                        alt += 1;
                        saw_space = true;
                        self.bump();
                    }
                    Some('\t') => {
                        saw_tab = true;
                        col = col + 8 - (col % 8);
                        alt += 1;
                        self.bump();
                    }
                    _ => break,
                }
            }
            match self.peek() {
                None => return Ok(()),
                Some('\n' | '\r') => {
                    self.consume_nl();
                    continue;
                }
                Some('#') => {
                    self.comment();
                    if self.at_newline() {
                        self.consume_nl();
                    }
                    continue;
                }
                _ => {}
            }
            if saw_tab && saw_space {
                let top = self.indent_top();
                if relation(col, top) != relation(alt, top) {
                    self.problem(at as u32, DiagnosticReason::MixedTabs);
                }
            }
            return self.apply_indent(col, at as u32);
        }
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    fn apply_indent(&mut self, col: usize, at: u32) -> Result<(), LexError> {
        let top = self.indent_top();
        if col == top {
            return Ok(());
        }
        if col > top {
            if self.indents.len() as u32 > self.max_depth {
                return Err(LexError::TooDeep {
                    line: self.line_now(),
                    limit: self.max_depth,
                });
            }
            self.indents.push(col);
            self.emit(super::Kind::Indent, at, at, false);
            return Ok(());
        }
        while self.indents.len() > 1 && self.indent_top() > col {
            self.indents.pop();
            self.emit(super::Kind::Dedent, at, at, false);
        }
        if self.indent_top() != col {
            self.problem(at, DiagnosticReason::InconsistentIndent);
        }
        Ok(())
    }
}

fn relation(col: usize, top: usize) -> Ordering {
    col.cmp(&top)
}
