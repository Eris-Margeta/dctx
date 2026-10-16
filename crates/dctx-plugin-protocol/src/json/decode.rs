//! Strict bounded JSON decoder for frame bodies and other preserved contract documents.
//! Implements plugin-api wire.md §Bytes and semantics.md S-BOUNDS: no BOM, UTF-8 only, one root object,
//! no trailing JSON, no duplicate keys, integer-only numbers, no NUL or unpaired surrogates, depth/member caps.
//! Depends on json::value and refusal; used by records::envelope and by fixture-driven tests.

use std::collections::BTreeSet;

use super::value::Json;
use crate::records::failure::ErrorCode;
use crate::refusal::{Predicate, Refusal, Stage};

/// Size and shape ceilings applied while decoding one document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeLimits {
    /// Maximum document bytes, checked before parsing.
    pub max_bytes: usize,
    /// Maximum container nesting; the root object is depth 1.
    pub max_depth: usize,
    /// Maximum object members summed over the whole document.
    pub max_members: usize,
}

impl DecodeLimits {
    /// Session frame bodies: 1 MiB, depth 32, 4096 members.
    pub const WIRE: Self = Self { max_bytes: 1_048_576, max_depth: 32, max_members: 4096 };
    /// Bootstrap frame bodies before negotiation: 64 KiB, depth 32, 4096 members.
    pub const BOOTSTRAP: Self = Self { max_bytes: 65_536, max_depth: 32, max_members: 4096 };
    /// Manifests (S-MANIFEST): 256 KiB, depth 32, 4096 members.
    pub const MANIFEST: Self = Self { max_bytes: 262_144, max_depth: 32, max_members: 4096 };
}

/// Decodes exactly one JSON object from `bytes` under `limits`.
pub fn decode_object(bytes: &[u8], limits: DecodeLimits) -> Result<Json, Refusal> {
    if bytes.len() > limits.max_bytes {
        return Err(Refusal::new(ErrorCode::ResourceLimit, Stage::Frame, Predicate::WFrame, "document-too-large"));
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(Refusal::frame("byte-order-mark"));
    }
    if std::str::from_utf8(bytes).is_err() {
        return Err(Refusal::frame("invalid-utf8"));
    }
    let mut parser = Parser { input: bytes, pos: 0, members: 0, limits };
    parser.skip_whitespace();
    if parser.peek() != Some(b'{') {
        return Err(Refusal::frame("non-object"));
    }
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.pos != bytes.len() {
        return Err(Refusal::decode("trailing-json"));
    }
    Ok(value)
}

/// Depth and member caps are resource limits (catalog json-depth/json-members).
fn limit(reason: &'static str) -> Refusal {
    Refusal::new(ErrorCode::ResourceLimit, Stage::Decode, Predicate::WJson, reason)
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
    members: usize,
    limits: DecodeLimits,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn next(&mut self) -> Result<u8, Refusal> {
        let byte = self.peek().ok_or(Refusal::decode("unexpected-end"))?;
        self.pos += 1;
        Ok(byte)
    }

    fn expect(&mut self, byte: u8) -> Result<(), Refusal> {
        if self.next()? == byte { Ok(()) } else { Err(Refusal::decode("unexpected-token")) }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, Refusal> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') => self.literal(b"true", Json::Bool(true)),
            Some(b'f') => self.literal(b"false", Json::Bool(false)),
            Some(b'n') => self.literal(b"null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(Refusal::decode("unexpected-token")),
            None => Err(Refusal::decode("unexpected-end")),
        }
    }

    fn enter(&self, depth: usize) -> Result<(), Refusal> {
        if depth > self.limits.max_depth { Err(limit("depth-limit")) } else { Ok(()) }
    }

    fn object(&mut self, depth: usize) -> Result<Json, Refusal> {
        self.enter(depth)?;
        self.expect(b'{')?;
        let mut members = Vec::new();
        let mut keys = BTreeSet::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                return Err(Refusal::decode("expected-key"));
            }
            let key = self.string()?;
            self.members += 1;
            if self.members > self.limits.max_members {
                return Err(limit("member-limit"));
            }
            if !keys.insert(key.clone()) {
                return Err(Refusal::decode("duplicate-key"));
            }
            self.skip_whitespace();
            self.expect(b':')?;
            let value = self.value(depth)?;
            members.push((key, value));
            self.skip_whitespace();
            match self.next()? {
                b',' => continue,
                b'}' => return Ok(Json::Object(members)),
                _ => return Err(Refusal::decode("unexpected-token")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, Refusal> {
        self.enter(depth)?;
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value(depth)?);
            self.skip_whitespace();
            match self.next()? {
                b',' => continue,
                b']' => return Ok(Json::Array(items)),
                _ => return Err(Refusal::decode("unexpected-token")),
            }
        }
    }

    fn literal(&mut self, word: &[u8], value: Json) -> Result<Json, Refusal> {
        if self.input[self.pos..].starts_with(word) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(Refusal::decode("unexpected-token"))
        }
    }

    fn number(&mut self) -> Result<Json, Refusal> {
        let negative = self.peek() == Some(b'-');
        if negative {
            self.pos += 1;
        }
        let first = self.next()?;
        if !first.is_ascii_digit() {
            return Err(Refusal::decode("invalid-number"));
        }
        let mut magnitude = i64::from(first - b'0');
        if first == b'0' && matches!(self.peek(), Some(b'0'..=b'9')) {
            return Err(Refusal::decode("leading-zero"));
        }
        while let Some(digit @ b'0'..=b'9') = self.peek() {
            self.pos += 1;
            magnitude = magnitude
                .checked_mul(10)
                .and_then(|value| value.checked_add(i64::from(digit - b'0')))
                .ok_or(Refusal::decode("integer-range"))?;
        }
        match self.peek() {
            Some(b'.') => return Err(Refusal::decode("fraction")),
            Some(b'e' | b'E') => return Err(Refusal::decode("exponent")),
            _ => {}
        }
        if negative && magnitude == 0 {
            return Err(Refusal::decode("negative-zero"));
        }
        Ok(Json::Int(if negative { -magnitude } else { magnitude }))
    }

    fn string(&mut self) -> Result<String, Refusal> {
        self.expect(b'"')?;
        let mut out = Vec::new();
        loop {
            match self.next()? {
                b'"' => break,
                b'\\' => self.escape(&mut out)?,
                0x00..=0x1F => return Err(Refusal::decode("control-in-string")),
                byte => out.push(byte),
            }
        }
        String::from_utf8(out).map_err(|_| Refusal::decode("invalid-utf8"))
    }

    fn escape(&mut self, out: &mut Vec<u8>) -> Result<(), Refusal> {
        let simple = match self.next()? {
            b'"' => b'"',
            b'\\' => b'\\',
            b'/' => b'/',
            b'b' => 0x08,
            b'f' => 0x0C,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'u' => return self.unicode_escape(out),
            _ => return Err(Refusal::decode("invalid-escape")),
        };
        out.push(simple);
        Ok(())
    }

    fn unicode_escape(&mut self, out: &mut Vec<u8>) -> Result<(), Refusal> {
        let unit = self.hex4()?;
        let scalar = match unit {
            0xD800..=0xDBFF => {
                if !self.input[self.pos..].starts_with(b"\\u") {
                    return Err(Refusal::decode("unpaired-surrogate"));
                }
                self.pos += 2;
                let low = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(Refusal::decode("unpaired-surrogate"));
                }
                0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00)
            }
            0xDC00..=0xDFFF => return Err(Refusal::decode("unpaired-surrogate")),
            0 => return Err(Refusal::decode("embedded-nul")),
            other => other,
        };
        let ch = char::from_u32(scalar).ok_or(Refusal::decode("unpaired-surrogate"))?;
        let mut buf = [0u8; 4];
        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, Refusal> {
        let mut value = 0u32;
        for _ in 0..4 {
            let digit = char::from(self.next()?).to_digit(16).ok_or(Refusal::decode("invalid-escape"))?;
            value = value * 16 + digit;
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(text: &str) -> &'static str {
        decode_object(text.as_bytes(), DecodeLimits::WIRE).map(|_| "accepted").unwrap_or_else(|r| r.reason)
    }

    #[test]
    fn accepts_objects_with_whitespace_and_escapes() {
        let value = decode_object(b" {\"a\" : [1, true, null, \"x\\u00e9\\ud83d\\ude00\\n\"]}\n", DecodeLimits::WIRE)
            .expect("valid");
        let items = value.get("a").and_then(Json::as_array).expect("array");
        assert_eq!(items[0], Json::Int(1));
        assert_eq!(items[3], Json::str("x\u{e9}\u{1F600}\n"));
    }

    #[test]
    fn number_spellings_are_integer_only() {
        assert_eq!(reason("{\"a\":0}"), "accepted");
        assert_eq!(reason("{\"a\":-1}"), "accepted");
        assert_eq!(reason("{\"a\":-0}"), "negative-zero");
        assert_eq!(reason("{\"a\":0.0}"), "fraction");
        assert_eq!(reason("{\"a\":1e3}"), "exponent");
        assert_eq!(reason("{\"a\":1E3}"), "exponent");
        assert_eq!(reason("{\"a\":01}"), "leading-zero");
        assert_eq!(reason("{\"a\":-}"), "invalid-number");
        assert_eq!(reason("{\"a\":99999999999999999999}"), "integer-range");
    }

    #[test]
    fn grammar_and_string_refusals() {
        assert_eq!(reason("{\"a\":1,\"a\":2}"), "duplicate-key");
        assert_eq!(reason("{\"a\":{\"b\":1,\"b\":1}}"), "duplicate-key");
        assert_eq!(reason("{\"a\":{\"b\":1},\"c\":{\"b\":1}}"), "accepted");
        assert_eq!(reason("{\"a\":1}{}"), "trailing-json");
        assert_eq!(reason("{\"a\":1,}"), "expected-key");
        assert_eq!(reason("{\"a\":\"\\ud800\"}"), "unpaired-surrogate");
        assert_eq!(reason("{\"a\":\"\\udc00\"}"), "unpaired-surrogate");
        assert_eq!(reason("{\"a\":\"\\ud800\\u0041\"}"), "unpaired-surrogate");
        assert_eq!(reason("{\"a\":\"\\u0000\"}"), "embedded-nul");
        assert_eq!(reason("{\"a\":\"\u{1}\"}"), "control-in-string");
        assert_eq!(reason("{\"a\":\"\\x\"}"), "invalid-escape");
        assert_eq!(reason("{\"a\":tru}"), "unexpected-token");
        assert_eq!(reason("{\"a\":"), "unexpected-end");
    }

    #[test]
    fn frame_layer_refusals() {
        assert_eq!(reason("[]"), "non-object");
        assert_eq!(reason("1"), "non-object");
        assert_eq!(reason(""), "non-object");
        let bom = decode_object(b"\xEF\xBB\xBF{}", DecodeLimits::WIRE).unwrap_err();
        assert_eq!((bom.stage, bom.reason), (Stage::Frame, "byte-order-mark"));
        let bad = decode_object(b"{\"a\":\"\xC3\x28\"}", DecodeLimits::WIRE).unwrap_err();
        assert_eq!(bad.reason, "invalid-utf8");
        let limits = DecodeLimits { max_bytes: 4, ..DecodeLimits::WIRE };
        let big = decode_object(b"{\"a\":1}", limits).unwrap_err();
        assert_eq!(big.code, ErrorCode::ResourceLimit);
    }

    #[test]
    fn depth_and_member_limits_are_exact() {
        let nest = |n: usize| format!("{{\"a\":{}0{}}}", "[".repeat(n), "]".repeat(n));
        assert_eq!(reason(&nest(31)), "accepted");
        assert_eq!(reason(&nest(32)), "depth-limit");
        let members = |n: usize| {
            let body: Vec<String> = (0..n).map(|i| format!("\"k{i}\":0")).collect();
            format!("{{{}}}", body.join(","))
        };
        assert_eq!(reason(&members(4096)), "accepted");
        assert_eq!(reason(&members(4097)), "member-limit");
    }
}
