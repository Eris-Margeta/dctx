//! Heredoc openers and bodies, including several openers on one line.
//! Implements Ruby `<<`, `<<-` and `<<~` forms with quoted and bare terminators.
//! The walker sees a string token at the opener; only `#{}` inside becomes code.
use super::{as_u32, Kind, Lx};
use crate::domain::DiagnosticReason;

pub(super) struct Pending {
    pub term: String,
    pub indent: bool,
    pub interp: bool,
    pub start: u32,
    pub slot: usize,
}

impl Lx<'_> {
    pub(super) fn try_heredoc(&mut self) -> Result<bool, crate::domain::LexError> {
        if self.fname || !self.src.get(self.i..).unwrap_or("").starts_with("<<") {
            return Ok(false);
        }
        let start = self.i;
        let mut j = self.i + 2;
        let mut indent = false;
        if matches!(self.byte_at(j), Some(b'-' | b'~')) {
            indent = true;
            j += 1;
        }
        let Some((term, interp, end)) = self.heredoc_term(j) else {
            return Ok(false);
        };
        self.i = end;
        let slot = self.tokens.len();
        self.emit(Kind::Str, start, end, false);
        self.pending.push(Pending {
            term,
            indent,
            interp,
            start: as_u32(start),
            slot,
        });
        self.expr = false;
        self.fname = false;
        Ok(true)
    }

    fn heredoc_term(&self, j: usize) -> Option<(String, bool, usize)> {
        match self.byte_at(j)? {
            b'\'' => self.quoted_term(j, b'\'').map(|(term, end)| (term, false, end)),
            b'"' => self.quoted_term(j, b'"').map(|(term, end)| (term, true, end)),
            b'`' => self.quoted_term(j, b'`').map(|(term, end)| (term, true, end)),
            byte if is_ident_byte(byte) => {
                let end = ident_end(self.src, j);
                let term = self.src[j..end].to_owned();
                (!term.is_empty()).then_some((term, true, end))
            }
            _ => None,
        }
    }

    fn quoted_term(&self, j: usize, quote: u8) -> Option<(String, usize)> {
        let mut k = j + 1;
        let mut term = String::new();
        while k < self.b.len() {
            let byte = self.b[k];
            if byte == b'\\' && k + 1 < self.b.len() {
                term.push(char::from(self.b[k + 1]));
                k += 2;
                continue;
            }
            if byte == quote {
                return Some((term, k + 1));
            }
            if byte == b'\n' {
                return None;
            }
            term.push(char::from(byte));
            k += 1;
        }
        None
    }

    pub(super) fn on_newline(&mut self) -> Result<(), crate::domain::LexError> {
        let at = self.i;
        self.i += 1;
        if !self.pending.is_empty() {
            self.flush_heredocs()?;
        }
        if self.brackets == 0 {
            self.emit(Kind::Newline, at, at + 1, false);
            self.expr = true;
            self.fname = false;
        }
        Ok(())
    }

    pub(super) fn finish_pending(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for doc in pending {
            self.problem(doc.start as usize, DiagnosticReason::UnterminatedString);
        }
        let open_at = self.closers.first().map(|open| open.at);
        if let Some(at) = open_at {
            self.problem(at as usize, DiagnosticReason::UnbalancedBracket);
        }
    }

    fn flush_heredocs(&mut self) -> Result<(), crate::domain::LexError> {
        let pending = std::mem::take(&mut self.pending);
        for doc in pending {
            self.read_heredoc(doc)?;
        }
        Ok(())
    }

    fn read_heredoc(&mut self, doc: Pending) -> Result<(), crate::domain::LexError> {
        let mut dynamic = false;
        loop {
            if self.i >= self.b.len() {
                self.problem(doc.start as usize, DiagnosticReason::UnterminatedString);
                self.retoken(doc.slot, self.i, false);
                return Ok(());
            }
            let line_start = self.i;
            let mut line_end = self.i;
            while line_end < self.b.len() && self.b[line_end] != b'\n' {
                line_end += 1;
            }
            if line_is_term(self.src, line_start, line_end, &doc.term, doc.indent) {
                self.i = line_end;
                if self.i < self.b.len() && self.b[self.i] == b'\n' {
                    self.i += 1;
                }
                self.retoken(doc.slot, self.i, !dynamic);
                return Ok(());
            }
            if doc.interp {
                dynamic |= self.scan_heredoc_line(line_start, line_end)?;
            } else {
                self.i = line_end;
            }
            if self.i <= line_end {
                self.i = line_end;
                if self.i < self.b.len() && self.b[self.i] == b'\n' {
                    self.i += 1;
                }
            }
        }
    }

    fn scan_heredoc_line(
        &mut self,
        start: usize,
        end: usize,
    ) -> Result<bool, crate::domain::LexError> {
        self.i = start;
        let mut dynamic = false;
        while self.i < end && self.i < self.b.len() {
            if self.b[self.i] == b'\\' && self.i + 1 < self.b.len() {
                self.i += 2;
                continue;
            }
            if self.b[self.i] == b'#' && self.byte_at(self.i + 1) == Some(b'{') {
                dynamic = true;
                self.i += 2;
                self.brackets += 1;
                self.push_depth()?;
                self.code_until_brace()?;
                continue;
            }
            self.i += 1;
        }
        if self.i < end {
            self.i = end;
        }
        Ok(dynamic)
    }

    fn retoken(&mut self, slot: usize, end: usize, plain: bool) {
        if let Some(token) = self.tokens.get_mut(slot) {
            token.end = as_u32(end);
            token.plain = plain;
        }
    }

    pub(super) fn scan_body(
        &mut self,
        start: usize,
        close: u8,
        paired: bool,
        interp: bool,
        plain_kind: bool,
    ) -> Result<(), crate::domain::LexError> {
        let open = super::scan::pair_open(close);
        self.push_depth()?;
        let mut nest = 1u32;
        let mut dynamic = false;
        while self.i < self.b.len() {
            let byte = self.b[self.i];
            if byte == b'\\' {
                self.i += 1;
                if self.i < self.b.len() {
                    self.i += 1;
                }
                continue;
            }
            if interp && byte == b'#' && self.byte_at(self.i + 1) == Some(b'{') {
                dynamic = true;
                self.i += 2;
                self.brackets += 1;
                self.push_depth()?;
                self.code_until_brace()?;
                continue;
            }
            if paired && open == byte {
                nest += 1;
                self.push_depth()?;
                self.i += 1;
                continue;
            }
            if byte == close {
                nest = nest.saturating_sub(1);
                self.depth = self.depth.saturating_sub(1);
                self.i += 1;
                if nest == 0 {
                    self.emit(Kind::Str, start, self.i, plain_kind && !dynamic);
                    self.expr = false;
                    self.fname = false;
                    return Ok(());
                }
                continue;
            }
            if byte == b'\n' || byte < 0x80 {
                self.i += 1;
            } else {
                self.advance_char();
            }
        }
        self.problem(start, DiagnosticReason::UnterminatedString);
        self.depth = self.depth.saturating_sub(nest);
        self.emit(Kind::Str, start, self.i, false);
        self.expr = false;
        self.fname = false;
        Ok(())
    }

    fn code_until_brace(&mut self) -> Result<(), crate::domain::LexError> {
        self.expr = true;
        let target = self.brackets;
        while self.i < self.b.len() {
            if self.b[self.i] == b'}' && self.brackets == target {
                self.i += 1;
                self.brackets = self.brackets.saturating_sub(1);
                self.depth = self.depth.saturating_sub(1);
                return Ok(());
            }
            self.step()?;
        }
        self.problem(self.i, DiagnosticReason::UnterminatedString);
        Ok(())
    }
}

fn line_is_term(src: &str, start: usize, end: usize, term: &str, indent: bool) -> bool {
    let mut line = src.get(start..end).unwrap_or("");
    if let Some(stripped) = line.strip_suffix('\r') {
        line = stripped;
    }
    if indent {
        line = line.trim_start_matches([' ', '\t']);
    }
    line == term
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn ident_end(src: &str, start: usize) -> usize {
    let mut k = start;
    let bytes = src.as_bytes();
    while k < bytes.len() && (bytes[k].is_ascii_alphanumeric() || bytes[k] == b'_') {
        k += 1;
    }
    k
}

#[cfg(test)]
mod tests {
    use crate::domain::Limits;
    use crate::lexer::lex;

    fn idents(source: &str) -> Vec<String> {
        let lexed = lex(source, &Limits::default()).expect("lex");
        lexed
            .tokens
            .iter()
            .filter(|token| token.kind == crate::lexer::Kind::Ident)
            .map(|token| source[token.start as usize..token.end as usize].to_owned())
            .collect()
    }

    #[test]
    fn heredoc_bodies_and_end_marker_do_not_leak_identifiers() {
        let source = "puts <<~EOS, <<-'PLAIN'\n  alpha()\nEOS\n  beta()\nPLAIN\ndef real\nend\n__END__\ndef ghost\nend\n";
        let names = idents(source);
        assert!(names.contains(&"puts".to_owned()), "{names:?}");
        assert!(names.contains(&"real".to_owned()), "{names:?}");
        assert!(!names.iter().any(|name| name == "alpha" || name == "beta" || name == "ghost"));
    }
}
