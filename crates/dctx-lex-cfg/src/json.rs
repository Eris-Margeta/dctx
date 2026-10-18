//! JSON and JSONC structural walk.
//! Implements the configuration extractor for `.json` and `.jsonc` (PARSERS-v1).
//! Comments are summaries in JSONC and diagnostics in JSON. Trailing commas are
//! accepted in both. The draft in `path` owns facts.
use crate::domain::{DiagnosticReason, Dialect, LexError, LexedFile, Limits};
use crate::path::{Cur, Draft, Kind};

pub(crate) fn extract(
    path: &str,
    source: &str,
    jsonc: bool,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    let mut parser = Parser {
        cur: Cur::new(source),
        draft: Draft::open(path, source, limits.max_facts, limits.max_depth)?,
        jsonc,
        max_depth: limits.max_depth,
    };
    parser.skip();
    if !parser.cur.eof() {
        match parser.cur.byte() {
            Some(b'{') => parser.object(Some(0), 1)?,
            Some(b'[') => parser.array(Some(0), 1)?,
            _ => parser.scalar(None, 1)?,
        }
        let _ = parser.skip();
        if !parser.cur.eof() {
            parser.diagnose(parser.cur.i, DiagnosticReason::InvalidSyntax);
        }
    }
    parser.draft.finish(if jsonc { Dialect::Jsonc } else { Dialect::Json })
}

struct Parser<'a> {
    cur: Cur<'a>,
    draft: Draft<'a>,
    jsonc: bool,
    max_depth: u32,
}

impl<'a> Parser<'a> {
    fn diagnose(&mut self, byte: usize, reason: DiagnosticReason) {
        self.draft.diagnose(byte, reason);
    }

    fn skip(&mut self) -> Option<String> {
        let mut lead: Option<String> = None;
        loop {
            match self.cur.byte() {
                Some(b' ' | b'\t' | b'\r') => self.cur.i += 1,
                Some(b'\n') => {
                    self.cur.i += 1;
                    let save = self.cur.i;
                    self.cur.skip_spaces();
                    if self.cur.byte() == Some(b'\n') {
                        lead = None;
                    }
                    self.cur.i = save;
                }
                Some(b'/') => {
                    if !self.jsonc {
                        self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                    }
                    let Some(text) = self.comment() else { break };
                    if self.jsonc && lead.is_none() && !text.is_empty() {
                        lead = Some(text);
                    }
                }
                _ => break,
            }
        }
        lead
    }

    fn comment(&mut self) -> Option<String> {
        if self.cur.starts("//") {
            self.cur.i += 2;
            let start = self.cur.i;
            while let Some(byte) = self.cur.byte() {
                if byte == b'\n' {
                    break;
                }
                self.cur.i += 1;
            }
            return Some(self.cur.slice(start, self.cur.i).trim().to_string());
        }
        if self.cur.starts("/*") {
            self.cur.i += 2;
            let start = self.cur.i;
            if let Some(rel) = self.cur.rest().find("*/") {
                let end = self.cur.i + rel;
                let text = self.cur.slice(start, end).to_string();
                self.cur.i = end + 2;
                let first = text.lines().next().unwrap_or("").trim().trim_start_matches('*').trim();
                return Some(first.to_string());
            }
            self.diagnose(start, DiagnosticReason::UnterminatedComment);
            self.cur.i = self.cur.end;
            return None;
        }
        self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
        self.cur.i += 1;
        None
    }

    fn trailing(&mut self) -> Option<String> {
        if !self.jsonc {
            return None;
        }
        let save = self.cur.i;
        self.cur.skip_spaces();
        if self.cur.byte() == Some(b'/') { self.comment() } else {
            self.cur.i = save;
            None
        }
    }

    fn object(&mut self, parent: Option<usize>, depth: u32) -> Result<(), LexError> {
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

    fn array(&mut self, parent: Option<usize>, depth: u32) -> Result<(), LexError> {
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
            let seg = index.to_string();
            index = index.saturating_add(1);
            let start = self.cur.i;
            self.value(parent, &seg, depth, start, doc)?;
            let _ = self.skip();
            if self.cur.eat(b',') {
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

    fn value(
        &mut self,
        parent: Option<usize>,
        key: &str,
        depth: u32,
        start: usize,
        doc: Option<String>,
    ) -> Result<(), LexError> {
        let container = matches!(self.cur.byte(), Some(b'{') | Some(b'['));
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

    fn scalar(&mut self, node: Option<usize>, depth: u32) -> Result<(), LexError> {
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

    fn string(&mut self) -> Option<String> {
        if !self.cur.eat(b'"') {
            return None;
        }
        let mut out = String::new();
        loop {
            match self.cur.byte() {
                None | Some(b'\n') => {
                    self.diagnose(self.cur.i, DiagnosticReason::UnterminatedString);
                    return None;
                }
                Some(b'"') => {
                    self.cur.i += 1;
                    return Some(out);
                }
                Some(b'\\') => {
                    self.cur.i += 1;
                    match self.cur.byte() {
                        Some(b'"') => out.push('"'),
                        Some(b'\\') => out.push('\\'),
                        Some(b'/') => out.push('/'),
                        Some(b'b') => out.push('\u{0008}'),
                        Some(b'f') => out.push('\u{000c}'),
                        Some(b'n') => out.push('\n'),
                        Some(b'r') => out.push('\r'),
                        Some(b't') => out.push('\t'),
                        Some(b'u') => {
                            self.cur.i += 1;
                            let hex = self.cur.slice(self.cur.i, self.cur.i.saturating_add(4));
                            if hex.len() != 4 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                                self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                                return None;
                            }
                            let Ok(code) = u32::from_str_radix(hex, 16) else {
                                self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                                return None;
                            };
                            self.cur.i += 4;
                            let Some(ch) = char::from_u32(code) else {
                                self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                                return None;
                            };
                            out.push(ch);
                            continue;
                        }
                        _ => {
                            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                            return None;
                        }
                    }
                    self.cur.i += 1;
                }
                Some(_) => {
                    if let Some(ch) = self.cur.src.get(self.cur.i..).and_then(|text| text.chars().next()) {
                        out.push(ch);
                        self.cur.i += ch.len_utf8();
                    } else {
                        self.cur.i += 1;
                    }
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Completion;

    #[test]
    fn jsonc_comments_are_summaries_and_json_comments_are_partial() {
        let limits = Limits::default();
        let source = "{\n// Name.\n\"n\": \"./a.rs\"\n}\n";
        let jsonc = extract("a.jsonc", source, true, &limits).unwrap();
        assert_eq!(jsonc.completion, Completion::Complete);
        assert_eq!(jsonc.nodes[1].fact.summary.text, "Name.");
        assert_eq!(jsonc.references[0].fact.name, "./a.rs");
        let json = extract("a.json", source, false, &limits).unwrap();
        assert_eq!(json.completion, Completion::Partial);
        assert!(json.nodes.iter().any(|node| node.fact.name == "n"));
    }
}
