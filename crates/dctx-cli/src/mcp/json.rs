// SPDX-License-Identifier: Apache-2.0
//! Minimal JSON value, parser and serializer for the MCP stdio server (no new dependency).
//! Numbers keep their source text so a request id is echoed back exactly as received.

use std::fmt::Write as _;

/// Nesting limit for parsed documents; deeper input is refused instead of recursing.
const MAX_DEPTH: usize = 64;

/// One JSON value. Object members keep their insertion order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number, kept as the source text.
    Num(String),
    /// A string.
    Str(String),
    /// An array.
    Arr(Vec<Json>),
    /// An object as ordered members.
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// Builds a string value.
    pub(crate) fn str(text: &str) -> Json {
        Json::Str(text.to_string())
    }

    /// Builds an object from literal members.
    pub(crate) fn obj(members: Vec<(&str, Json)>) -> Json {
        Json::Obj(
            members
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    /// Member `key` of an object, if present.
    pub(crate) fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(members) => members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// The text of a string value.
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(text) => Some(text),
            _ => None,
        }
    }

    /// Serializes to compact single-line JSON (control characters and non-ASCII are escaped as needed).
    pub(crate) fn to_line(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Json::Num(text) => out.push_str(text),
            Json::Str(text) => write_string(text, out),
            Json::Arr(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Obj(members) => {
                out.push('{');
                for (index, (key, value)) in members.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_string(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if u32::from(ch) < 0x20 || ch == '\u{2028}' || ch == '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", u32::from(ch));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// Parses one complete JSON document; trailing non-space input is an error.
pub(crate) fn parse(input: &str) -> Result<Json, String> {
    let mut parser = Parser {
        bytes: input.as_bytes(),
        at: 0,
    };
    let value = parser.value(0)?;
    parser.space();
    if parser.at != parser.bytes.len() {
        return Err("trailing characters after the JSON value".into());
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Result<(), String> {
        if self.bytes.get(self.at) == Some(&byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", byte as char, self.at))
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(format!("invalid literal at byte {}", self.at))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err("JSON nesting is too deep".into());
        }
        self.space();
        match self.bytes.get(self.at) {
            None => Err("unexpected end of JSON".into()),
            Some(b'n') => self.word("null", Json::Null),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(format!("unexpected character at byte {}", self.at)),
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        while matches!(
            self.bytes.get(self.at),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.at += 1;
        }
        let text =
            std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_| "invalid number")?;
        text.parse::<f64>()
            .map_err(|_| format!("invalid number at byte {start}"))?;
        Ok(Json::Num(text.to_string()))
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.eat(b'[')?;
        let mut items = Vec::new();
        self.space();
        if self.bytes.get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.space();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(format!("expected ',' or ']' at byte {}", self.at)),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.eat(b'{')?;
        let mut members = Vec::new();
        self.space();
        if self.bytes.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(Json::Obj(members));
        }
        loop {
            self.space();
            let key = self.string()?;
            self.space();
            self.eat(b':')?;
            members.push((key, self.value(depth + 1)?));
            self.space();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Obj(members));
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.at)),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .ok_or("short \\u escape")?;
        let text = std::str::from_utf8(digits).map_err(|_| "invalid \\u escape")?;
        let code = u32::from_str_radix(text, 16).map_err(|_| "invalid \\u escape")?;
        self.at += 4;
        Ok(code)
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let start = self.at;
            while !matches!(self.bytes.get(self.at), None | Some(b'"' | b'\\')) {
                self.at += 1;
            }
            // The input came from a &str and we split only at ASCII bytes, so this stays valid UTF-8.
            out.push_str(
                std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_| "invalid UTF-8")?,
            );
            match self.bytes.get(self.at) {
                None => return Err("unterminated string".into()),
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                _ => {
                    self.at += 1;
                    let escape = *self.bytes.get(self.at).ok_or("unterminated escape")?;
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode()?),
                        _ => return Err("invalid escape".into()),
                    }
                }
            }
        }
    }

    fn unicode(&mut self) -> Result<char, String> {
        let high = self.hex4()?;
        if (0xD800..0xDC00).contains(&high) {
            if self.bytes[self.at..].starts_with(b"\\u") {
                self.at += 2;
                let low = self.hex4()?;
                if (0xDC00..0xE000).contains(&low) {
                    let code = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                    return char::from_u32(code)
                        .ok_or_else(|| "invalid surrogate pair".to_string());
                }
            }
            return Err("unpaired surrogate".into());
        }
        char::from_u32(high).ok_or_else(|| "unpaired surrogate".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_escapes_numbers_and_nesting() {
        let text = r#"{"a":[1,-2.5e3,true,null,"x\n\"\u00e9\ud83d\ude00"],"b":{}}"#;
        let value = parse(text).expect("parse");
        assert_eq!(
            value.get("a"),
            Some(&Json::Arr(vec![
                Json::Num("1".into()),
                Json::Num("-2.5e3".into()),
                Json::Bool(true),
                Json::Null,
                Json::str("x\n\"\u{e9}\u{1f600}"),
            ]))
        );
        assert_eq!(parse(&value.to_line()).expect("again"), value);
        assert_eq!(Json::str("a\u{1}b").to_line(), "\"a\\u0001b\"");
    }

    #[test]
    fn refuses_malformed_and_deep_input() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\" 1}",
            "\"abc",
            "1 2",
            "\"\\ud800\"",
            "tru",
            "{\"a\":}",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        assert!(parse(&"[".repeat(200)).is_err());
    }
}

/// Escapes an engine byte stream inside one MCP JSON string. The two-byte carry
/// preserves U+2028/U+2029 escaping even when a UTF-8 scalar crosses writes.
pub(super) struct TextWriter<'a> {
    output: &'a mut dyn std::io::Write,
    pending: Vec<u8>,
    buffer: Vec<u8>,
    pub(super) written: bool,
    pub(super) failure: Option<std::io::Error>,
}

impl<'a> TextWriter<'a> {
    pub(super) fn new(output: &'a mut dyn std::io::Write) -> Self {
        Self {
            output,
            pending: Vec::with_capacity(2),
            buffer: Vec::with_capacity(64 * 1024),
            written: false,
            failure: None,
        }
    }

    fn emit(&mut self, mut bytes: &[u8]) -> std::io::Result<()> {
        while !bytes.is_empty() {
            let count = bytes.len().min(64 * 1024 - self.buffer.len());
            self.buffer.extend_from_slice(&bytes[..count]);
            bytes = &bytes[count..];
            if self.buffer.len() == 64 * 1024 {
                self.flush_buffer(false)?;
            }
        }
        Ok(())
    }

    // Keep incomplete scalars until the next input write. Console writers on Windows
    // require each physical write to contain valid UTF-8, unlike byte-oriented pipes.
    fn flush_buffer(&mut self, complete: bool) -> std::io::Result<()> {
        let count = match std::str::from_utf8(&self.buffer) {
            Ok(_) => self.buffer.len(),
            Err(error) if !complete && error.error_len().is_none() => error.valid_up_to(),
            Err(error) => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        };
        if count > 0 {
            self.output.write_all(&self.buffer[..count])?;
        }
        self.buffer.copy_within(count.., 0);
        self.buffer.truncate(self.buffer.len() - count);
        Ok(())
    }

    fn escaped(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let mut start = 0;
        for (index, byte) in bytes.iter().copied().enumerate() {
            let special = byte < 0x20 || matches!(byte, b'"' | b'\\' | 0xe2);
            if !self.pending.is_empty() || special {
                self.emit(&bytes[start..index])?;
                start = index + 1;
                if self.pending == [0xe2] && byte == 0x80 {
                    self.pending.push(byte);
                    continue;
                }
                if self.pending == [0xe2, 0x80] && matches!(byte, 0xa8 | 0xa9) {
                    self.pending.clear();
                    self.emit(if byte == 0xa8 { b"\\u2028" } else { b"\\u2029" })?;
                    continue;
                }
                self.emit_pending()?;
                match byte {
                    0xe2 => self.pending.push(byte),
                    b'"' => self.emit(b"\\\"")?,
                    b'\\' => self.emit(b"\\\\")?,
                    b'\n' => self.emit(b"\\n")?,
                    b'\r' => self.emit(b"\\r")?,
                    b'\t' => self.emit(b"\\t")?,
                    control if control < 0x20 => {
                        self.emit(format!("\\u{control:04x}").as_bytes())?
                    }
                    other => self.emit(&[other])?,
                }
            }
        }
        self.emit(&bytes[start..])
    }

    pub(super) fn finish(&mut self) -> std::io::Result<()> {
        self.emit_pending()?;
        self.flush_buffer(true)
    }

    fn emit_pending(&mut self) -> std::io::Result<()> {
        let mut pending = [0; 2];
        let len = self.pending.len();
        pending[..len].copy_from_slice(&self.pending);
        self.pending.clear();
        self.emit(&pending[..len])
    }
}

impl std::io::Write for TextWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Some(error) = &self.failure {
            return Err(std::io::Error::new(error.kind(), error.to_string()));
        }
        if let Err(error) = self.escaped(bytes) {
            let returned = std::io::Error::new(error.kind(), error.to_string());
            self.failure = Some(error);
            return Err(returned);
        }
        self.written |= !bytes.is_empty();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        let result = self.flush_buffer(false).and_then(|()| self.output.flush());
        if let Err(error) = result {
            let returned = std::io::Error::new(error.kind(), error.to_string());
            if self.failure.is_none() {
                self.failure = Some(error);
            }
            Err(returned)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn physical_writes_keep_utf8_scalars_intact_for_windows_consoles() {
        struct Console(Vec<u8>);
        impl std::io::Write for Console {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                assert!(bytes.len() <= 64 * 1024);
                assert!(std::str::from_utf8(bytes).is_ok());
                self.0.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let sample = "α€\u{2028}\u{2029}".repeat(20_000);
        let expected = Json::str(&sample).to_line();
        for width in [1, 2, 3, 65_535, 65_536, 65_537] {
            let mut console = Console(Vec::new());
            let mut writer = TextWriter::new(&mut console);
            for chunk in sample.as_bytes().chunks(width) {
                writer.write_all(chunk).unwrap();
            }
            writer.finish().unwrap();
            assert_eq!(console.0, expected.as_bytes()[1..expected.len() - 1]);
        }
    }

    #[test]
    fn streaming_escape_matches_captured_json_at_every_chunk_boundary() {
        let sample = "quote\" slash\\ newline\n\r\t\0\u{0008}\u{000c} α€\u{2028}\u{2029}\u{2003}";
        for width in 1..=sample.len() {
            let mut bytes = Vec::new();
            let mut stream = TextWriter::new(&mut bytes);
            for chunk in sample.as_bytes().chunks(width) {
                stream.write_all(chunk).unwrap();
            }
            stream.finish().unwrap();
            assert_eq!(
                format!("\"{}\"", String::from_utf8(bytes).unwrap()),
                Json::str(sample).to_line(),
                "width {width}"
            );
        }
    }
}
