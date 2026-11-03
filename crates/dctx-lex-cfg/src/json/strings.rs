// SPDX-License-Identifier: Apache-2.0
//! String decoding for the JSON walker.
use super::*;

impl<'a> Parser<'a> {
    pub(super) fn string(&mut self) -> Option<Cow<'a, str>> {
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
}
