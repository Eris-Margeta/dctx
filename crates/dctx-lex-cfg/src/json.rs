//! JSON and JSONC structural walk.
//! Implements the configuration extractor for `.json` and `.jsonc` (PARSERS-v1).
//! Comments are summaries in JSONC and diagnostics in JSON. Object trailing
//! commas are accepted; array trailing commas are JSONC-only. `path` owns facts.
//! Unescaped strings are borrowed from the source; escapes decode in one pass.
#[cfg(test)]
use std::fmt::Write as _;
use crate::domain::{DiagnosticReason, Dialect, LexError, LexedFile, Limits};
use crate::path::{Cur, Draft, Kind};
use std::borrow::Cow;

pub(crate) fn extract(
    path: &str,
    source: &str,
    jsonc: bool,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    walk(path, source, jsonc, limits, false)
}

#[cfg(test)]
pub(crate) fn extract_oracle(
    path: &str,
    source: &str,
    jsonc: bool,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    walk(path, source, jsonc, limits, true)
}

fn walk(
    path: &str,
    source: &str,
    jsonc: bool,
    limits: &Limits,
    legacy: bool,
) -> Result<LexedFile, LexError> {
    let _ = legacy; // Only test builds enable the legacy string-decoder oracle.
    let mut parser = Parser {
        cur: Cur::new(source),
        draft: Draft::open(path, source, limits.max_facts, limits.max_depth)?,
        jsonc,
        max_depth: limits.max_depth,
        #[cfg(test)]
        legacy_strings: legacy,
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
    /// Test builds keep the pre-change per-character string decoder as an oracle.
    #[cfg(test)]
    legacy_strings: bool,
}

impl<'a> Parser<'a> {
    fn diagnose(&mut self, byte: usize, reason: DiagnosticReason) {
        self.draft.diagnose(byte, reason);
    }

    fn skip(&mut self) -> Option<String> {
        let mut lead: Option<String> = None;
        let bytes = self.cur.src.as_bytes();
        let limit = self.cur.end.min(bytes.len());
        loop {
            if self.cur.i < limit && matches!(bytes[self.cur.i], b' ' | b'\t' | b'\r') {
                while self.cur.i < limit && matches!(bytes[self.cur.i], b' ' | b'\t' | b'\r') {
                    self.cur.i += 1;
                }
                continue;
            }
            match self.cur.byte() {
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

    fn closer_after_space(&self, close: u8) -> bool {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let mut index = self.cur.i;
        while index < end && matches!(bytes[index], b' ' | b'\t' | b'\r' | b'\n') {
            index += 1;
        }
        bytes.get(index) == Some(&close)
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

    fn string(&mut self) -> Option<Cow<'a, str>> {
        #[cfg(test)]
        if self.legacy_strings {
            return self.string_legacy().map(Cow::Owned);
        }
        self.string_scan()
    }

    /// One forward scan. A string with no escape is a slice of the source.
    fn string_scan(&mut self) -> Option<Cow<'a, str>> {
        if !self.cur.eat(b'"') {
            return None;
        }
        let src = self.cur.src;
        let bytes = src.as_bytes();
        let limit = self.cur.end.min(bytes.len());
        let start = self.cur.i;
        let mut index = start;
        while index < limit {
            match bytes[index] {
                b'"' => {
                    self.cur.i = index + 1;
                    return Some(Cow::Borrowed(&src[start..index]));
                }
                b'\\' | b'\n' => break,
                _ => index += 1,
            }
        }
        if index >= limit || bytes[index] == b'\n' {
            self.cur.i = index.min(limit);
            self.diagnose(self.cur.i, DiagnosticReason::UnterminatedString);
            return None;
        }
        let mut out = String::new();
        out.push_str(&src[start..index]);
        self.cur.i = index;
        self.string_escape_tail(&mut out)?;
        Some(Cow::Owned(out))
    }

    fn string_escape_tail(&mut self, out: &mut String) -> Option<()> {
        loop {
            let bytes = self.cur.src.as_bytes();
            let limit = self.cur.end.min(bytes.len());
            if self.cur.i >= limit {
                self.diagnose(self.cur.i, DiagnosticReason::UnterminatedString);
                return None;
            }
            match bytes[self.cur.i] {
                b'\n' => {
                    self.diagnose(self.cur.i, DiagnosticReason::UnterminatedString);
                    return None;
                }
                b'"' => {
                    self.cur.i += 1;
                    return Some(());
                }
                b'\\' => {
                    self.cur.i += 1;
                    if self.cur.i >= limit {
                        self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                        return None;
                    }
                    match bytes[self.cur.i] {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            self.cur.i += 1;
                            let code = self.unicode_unit()?;
                            if let Some(ch) = char::from_u32(code) {
                                out.push(ch);
                                continue;
                            }
                            if (0xD800..=0xDBFF).contains(&code)
                                && let Some(low) = self.low_surrogate()
                            {
                                let scalar = 0x10000 + (((code - 0xD800) << 10) | (low - 0xDC00));
                                if let Some(ch) = char::from_u32(scalar) {
                                    out.push(ch);
                                } else {
                                    self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                                }
                                continue;
                            }
                            // A lone surrogate is invalid, but the rest of the value stays.
                            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                            continue;
                        }
                        _ => {
                            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                            return None;
                        }
                    }
                    self.cur.i += 1;
                }
                _ => {
                    let run = self.cur.i;
                    let mut index = run + 1;
                    while index < limit && !matches!(bytes[index], b'"' | b'\\' | b'\n') {
                        index += 1;
                    }
                    out.push_str(&self.cur.src[run..index]);
                    self.cur.i = index;
                }
            }
        }
    }

    /// Pre-change decoder: one `chars()` step and one `String` push per scalar.
    #[cfg(test)]
    fn string_legacy(&mut self) -> Option<String> {
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
                            let code = self.unicode_unit()?;
                            if let Some(ch) = char::from_u32(code) {
                                out.push(ch);
                                continue;
                            }
                            if (0xD800..=0xDBFF).contains(&code)
                                && let Some(low) = self.low_surrogate()
                            {
                                let scalar = 0x10000 + (((code - 0xD800) << 10) | (low - 0xDC00));
                                if let Some(ch) = char::from_u32(scalar) {
                                    out.push(ch);
                                } else {
                                    self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                                }
                                continue;
                            }
                            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
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

    fn unicode_unit(&mut self) -> Option<u32> {
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
        Some(code)
    }

    fn low_surrogate(&mut self) -> Option<u32> {
        let save = self.cur.i;
        if self.cur.byte() != Some(b'\\') {
            return None;
        }
        self.cur.i += 1;
        if self.cur.byte() != Some(b'u') {
            self.cur.i = save;
            return None;
        }
        self.cur.i += 1;
        let hex = self.cur.slice(self.cur.i, self.cur.i.saturating_add(4));
        if hex.len() != 4 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            self.cur.i = save;
            return None;
        }
        let Ok(code) = u32::from_str_radix(hex, 16) else {
            self.cur.i = save;
            return None;
        };
        if (0xDC00..=0xDFFF).contains(&code) {
            self.cur.i += 4;
            Some(code)
        } else {
            self.cur.i = save;
            None
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

#[cfg(test)]
mod tests;
