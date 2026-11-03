// SPDX-License-Identifier: Apache-2.0
//! Objects, arrays, scalars and fact creation for the JSON walker.
use super::*;

impl<'a> Parser<'a> {
    pub(super) fn object(&mut self, parent: Option<usize>, depth: u32) -> Result<(), LexError> {
        if depth > self.max_depth {
            return Err(LexError::TooDeep { line: self.draft.line(self.cur.i), limit: self.max_depth });
        }
        self.cur.bump();
        loop {
            let doc = self.skip();
            if self.cur.eat(b'}') {
                break;
            }
            if self.cur.eof() {
                self.diagnose(self.cur.i, DiagnosticReason::UnbalancedBracket);
                break;
            }
            let start = self.cur.i;
            let Some(key) = self.string() else {
                self.diagnose(start, DiagnosticReason::InvalidSyntax);
                break;
            };
            let _ = self.skip();
            if !self.cur.eat(b':') {
                self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                break;
            }
            let _ = self.skip();
            self.value(parent, &key, depth, start, doc)?;
            let _ = self.skip();
            // Trailing commas are valid in the editor dialects these files use.
            if self.cur.eat(b',') {
                continue;
            }
            if self.cur.eat(b'}') {
                break;
            }
            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
            break;
        }
        Ok(())
    }

    pub(super) fn array(&mut self, parent: Option<usize>, depth: u32) -> Result<(), LexError> {
        if depth > self.max_depth {
            return Err(LexError::TooDeep { line: self.draft.line(self.cur.i), limit: self.max_depth });
        }
        self.cur.bump();
        let mut index = 0u32;
        loop {
            let doc = self.skip();
            if self.cur.eat(b']') {
                break;
            }
            if self.cur.eof() {
                self.diagnose(self.cur.i, DiagnosticReason::UnbalancedBracket);
                break;
            }
            let mut numerals = [0u8; 10];
            let seg = format_index(&mut numerals, index);
            index = index.saturating_add(1);
            let start = self.cur.i;
            self.value(parent, seg, depth, start, doc)?;
            let _ = self.skip();
            if self.cur.eat(b',') {
                // RFC 8259 refuses `[1,]`. JSONC keeps the comma. Objects stay
                // permissive: settings.json locks a trailing comma as complete.
                if !self.jsonc && self.closer_after_space(b']') {
                    self.diagnose(self.cur.i.saturating_sub(1), DiagnosticReason::InvalidSyntax);
                }
                continue;
            }
            if self.cur.eat(b']') {
                break;
            }
            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
            break;
        }
        Ok(())
    }

    #[allow(clippy::needless_pass_by_value, reason = "Pending comment text is transferred at grammar dispatch boundaries and is not reusable by the caller after that item is handled.")]
    fn value(
        &mut self,
        parent: Option<usize>,
        key: &str,
        depth: u32,
        start: usize,
        doc: Option<String>,
    ) -> Result<(), LexError> {
        let container = matches!(self.cur.byte(), Some(b'{' | b'['));
        let node = self.make(parent, key, container, depth, start)?;
        if let (Some(index), Some(text)) = (node, doc.as_deref()) {
            self.draft.set_doc(index, text);
        }
        match self.cur.byte() {
            Some(b'{') => self.object(node, depth + 1)?,
            Some(b'[') => self.array(node, depth + 1)?,
            _ => self.scalar(node, depth)?,
        }
        if let Some(text) = self.trailing()
            && let Some(index) = node
        {
            self.draft.set_doc(index, &text);
        }
        if let Some(index) = node {
            self.draft.extend(index, self.cur.i);
        }
        Ok(())
    }

    pub(super) fn scalar(&mut self, node: Option<usize>, depth: u32) -> Result<(), LexError> {
        if depth > self.max_depth {
            return Err(LexError::TooDeep { line: self.draft.line(self.cur.i), limit: self.max_depth });
        }
        let start = self.cur.i;
        if self.cur.byte() == Some(b'"') {
            if let Some(text) = self.string()
                && let Some(index) = node
            {
                self.draft.set_signature(index, &text);
                self.draft.string_ref(index, &text, start, self.cur.i)?;
            }
            return Ok(());
        }
        if self.literal() {
            if let Some(index) = node {
                self.draft.set_signature(index, self.cur.slice(start, self.cur.i).trim());
            }
            return Ok(());
        }
        self.diagnose(start, DiagnosticReason::InvalidSyntax);
        if self.cur.byte().is_some() {
            self.cur.i += 1;
        }
        Ok(())
    }

    fn literal(&mut self) -> bool {
        let rest = self.cur.rest();
        for word in ["true", "false", "null"] {
            if let Some(tail) = rest.strip_prefix(word) {
                let boundary = tail.chars().next().is_none_or(|ch| !ch.is_ascii_alphanumeric());
                if boundary {
                    self.cur.i += word.len();
                    return true;
                }
            }
        }
        let start = self.cur.i;
        if self.cur.byte() == Some(b'-') {
            self.cur.i += 1;
        }
        let digits = self.cur.i;
        while matches!(self.cur.byte(), Some(b'0'..=b'9')) {
            self.cur.i += 1;
        }
        if self.cur.i == digits {
            self.cur.i = start;
            return false;
        }
        if self.cur.eat(b'.') {
            while matches!(self.cur.byte(), Some(b'0'..=b'9')) {
                self.cur.i += 1;
            }
        }
        if matches!(self.cur.byte(), Some(b'e' | b'E')) {
            self.cur.i += 1;
            if matches!(self.cur.byte(), Some(b'+' | b'-')) {
                self.cur.i += 1;
            }
            while matches!(self.cur.byte(), Some(b'0'..=b'9')) {
                self.cur.i += 1;
            }
        }
        true
    }

    fn make(
        &mut self,
        parent: Option<usize>,
        key: &str,
        container: bool,
        depth: u32,
        start: usize,
    ) -> Result<Option<usize>, LexError> {
        let Some(parent) = parent else {
            if depth > self.max_depth {
                return Err(LexError::TooDeep { line: self.draft.line(start), limit: self.max_depth });
            }
            return Ok(None);
        };
        Ok(self.draft.put(parent, key, Kind::Value(container), start, start, depth)?.index())
    }
}

/// Decimal spelling of an array index. The buffer holds every `u32`.
fn format_index(buf: &mut [u8; 10], mut value: u32) -> &str {
    if value == 0 {
        buf[9] = b'0';
        return match std::str::from_utf8(&buf[9..]) {
            Ok(text) => text,
            Err(_) => "0",
        };
    }
    let mut end = buf.len();
    while value > 0 {
        end -= 1;
        buf[end] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    match std::str::from_utf8(&buf[end..]) {
        Ok(text) => text,
        Err(_) => "0",
    }
}
