//! Strict JSON for one bounded index page.
//! Implements semantics.md S-BOUNDS and distribution.md's 4 MiB / depth 32 / 4096-member cap.
//! Depends on [`crate::IndexError`]. Index and pin parsers consume [`parse`].
//! Duplicate keys, exponents, and trailing values are refused before a map exists.

use crate::IndexError;

/// Default discovery page cap from PLUGINS.md §5.3.
pub(crate) const MAX_INDEX_BYTES: usize = 4 * 1024 * 1024;
/// Root object counts as depth 1.
pub(crate) const MAX_DEPTH: usize = 32;
/// Object members summed across the page.
pub(crate) const MAX_MEMBERS: usize = 4096;
/// Largest array the index schema allows (`plugins`).
pub(crate) const MAX_ARRAY: usize = 2048;
/// Largest integer JSON Schema and S-BOUNDS both accept.
pub(crate) const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// One JSON value. Objects keep source order and already-unique keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Number(u64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    depth: usize,
    members: usize,
}

/// Parses one JSON value. The size cap is applied before any allocation of values.
pub(crate) fn parse(bytes: &[u8]) -> Result<Json, IndexError> {
    if bytes.len() > MAX_INDEX_BYTES {
        return Err(IndexError::TooLarge);
    }
    if std::str::from_utf8(bytes).is_err() || bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(IndexError::Malformed);
    }
    let mut parser = Parser {
        bytes,
        index: 0,
        depth: 0,
        members: 0,
    };
    let value = parser.parse_value()?;
    parser.skip_ws();
    if parser.index != parser.bytes.len() {
        return Err(IndexError::Malformed);
    }
    Ok(value)
}

impl<'a> Parser<'a> {
    fn parse_value(&mut self) -> Result<Json, IndexError> {
        self.skip_ws();
        match self.bytes.get(self.index).copied() {
            Some(b'{') | Some(b'[') => self.parse_container(),
            Some(b'"') => Ok(Json::String(self.parse_string()?)),
            Some(b't') => {
                self.eat_word(b"true")?;
                Ok(Json::Bool(true))
            }
            Some(b'f') => {
                self.eat_word(b"false")?;
                Ok(Json::Bool(false))
            }
            Some(b'n') => {
                self.eat_word(b"null")?;
                Ok(Json::Null)
            }
            Some(byte) if byte.is_ascii_digit() => Ok(Json::Number(self.parse_number()?)),
            Some(b'-' | b'+') => Err(IndexError::Integer),
            _ => Err(IndexError::Malformed),
        }
    }

    fn parse_container(&mut self) -> Result<Json, IndexError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(IndexError::Depth);
        }
        let value = if self.bytes.get(self.index) == Some(&b'{') {
            Json::Object(self.parse_object()?)
        } else {
            Json::Array(self.parse_array()?)
        };
        self.depth -= 1;
        Ok(value)
    }

    fn parse_object(&mut self) -> Result<Vec<(String, Json)>, IndexError> {
        self.expect(b'{')?;
        self.skip_ws();
        let mut fields = Vec::new();
        if self.eat(b'}') {
            return Ok(fields);
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            let value = self.parse_value()?;
            if fields.iter().any(|(existing, _)| existing == &key) {
                return Err(IndexError::DuplicateKey);
            }
            self.members = self.members.checked_add(1).ok_or(IndexError::TooManyMembers)?;
            if self.members > MAX_MEMBERS {
                return Err(IndexError::TooManyMembers);
            }
            fields.push((key, value));
            self.skip_ws();
            if self.eat(b'}') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(fields)
    }

    fn parse_array(&mut self) -> Result<Vec<Json>, IndexError> {
        self.expect(b'[')?;
        self.skip_ws();
        let mut items = Vec::new();
        if self.eat(b']') {
            return Ok(items);
        }
        loop {
            if items.len() == MAX_ARRAY {
                return Err(IndexError::TooManyMembers);
            }
            items.push(self.parse_value()?);
            self.skip_ws();
            if self.eat(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(items)
    }

    fn parse_string(&mut self) -> Result<String, IndexError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(byte) = self.bytes.get(self.index).copied() else {
                return Err(IndexError::Malformed);
            };
            if byte == b'"' {
                self.index += 1;
                break;
            }
            if byte == b'\\' {
                self.index += 1;
                self.push_escape(&mut out)?;
                continue;
            }
            if byte < 0x20 {
                return Err(IndexError::Malformed);
            }
            out.push(self.next_char()?);
        }
        if out.as_bytes().contains(&0) {
            return Err(IndexError::Malformed);
        }
        Ok(out)
    }

    fn push_escape(&mut self, out: &mut String) -> Result<(), IndexError> {
        let Some(escape) = self.bytes.get(self.index).copied() else {
            return Err(IndexError::Malformed);
        };
        self.index += 1;
        let ch = match escape {
            b'"' | b'\\' | b'/' => escape as char,
            b'b' => '\u{0008}',
            b'f' => '\u{000c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => self.decode_unit(self.hex4()?)?,
            _ => return Err(IndexError::Malformed),
        };
        out.push(ch);
        Ok(())
    }

    fn decode_unit(&mut self, unit: u16) -> Result<char, IndexError> {
        if (0xDC00..=0xDFFF).contains(&unit) {
            return Err(IndexError::Malformed);
        }
        if !(0xD800..=0xDBFF).contains(&unit) {
            return char::from_u32(u32::from(unit)).ok_or(IndexError::Malformed);
        }
        if self.bytes.get(self.index) != Some(&b'\\') || self.bytes.get(self.index + 1) != Some(&b'u')
        {
            return Err(IndexError::Malformed);
        }
        self.index += 2;
        let low = self.hex4()?;
        if !(0xDC00..=0xDFFF).contains(&low) {
            return Err(IndexError::Malformed);
        }
        let code = 0x10000 + ((u32::from(unit) - 0xD800) << 10) + (u32::from(low) - 0xDC00);
        char::from_u32(code).ok_or(IndexError::Malformed)
    }

    fn hex4(&mut self) -> Result<u16, IndexError> {
        let mut value = 0u16;
        for _ in 0..4 {
            let Some(byte) = self.bytes.get(self.index).copied() else {
                return Err(IndexError::Malformed);
            };
            self.index += 1;
            let nibble = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err(IndexError::Malformed),
            };
            value = (value << 4) | u16::from(nibble);
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<u64, IndexError> {
        let start = self.index;
        let Some(first) = self.bytes.get(self.index).copied() else {
            return Err(IndexError::Malformed);
        };
        if first == b'0' {
            self.index += 1;
            if self
                .bytes
                .get(self.index)
                .is_some_and(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E'))
            {
                return Err(IndexError::Integer);
            }
            return Ok(0);
        }
        if !first.is_ascii_digit() {
            return Err(IndexError::Malformed);
        }
        while self
            .bytes
            .get(self.index)
            .is_some_and(|byte| byte.is_ascii_digit())
        {
            self.index += 1;
        }
        if self
            .bytes
            .get(self.index)
            .is_some_and(|byte| matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-'))
        {
            return Err(IndexError::Integer);
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| IndexError::Malformed)?;
        let value: u64 = text.parse().map_err(|_| IndexError::Integer)?;
        if value > MAX_SAFE_INTEGER {
            return Err(IndexError::Integer);
        }
        Ok(value)
    }

    fn next_char(&mut self) -> Result<char, IndexError> {
        let rest = std::str::from_utf8(&self.bytes[self.index..]).map_err(|_| IndexError::Malformed)?;
        let Some(ch) = rest.chars().next() else {
            return Err(IndexError::Malformed);
        };
        self.index += ch.len_utf8();
        Ok(ch)
    }

    fn eat_word(&mut self, word: &[u8]) -> Result<(), IndexError> {
        if self.bytes[self.index..].starts_with(word) {
            self.index += word.len();
            Ok(())
        } else {
            Err(IndexError::Malformed)
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), IndexError> {
        if self.eat(byte) {
            Ok(())
        } else {
            Err(IndexError::Malformed)
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.bytes.get(self.index) == Some(&byte) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while self
            .bytes
            .get(self.index)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_key_exponent_and_trailing_value_are_refused() {
        assert_eq!(parse(br#"{"a":1,"a":2}"#), Err(IndexError::DuplicateKey));
        assert_eq!(parse(br#"{"a":"\u0061","a":1}"#), Err(IndexError::DuplicateKey));
        assert_eq!(parse(br#"{"v":0e0}"#), Err(IndexError::Integer));
        assert_eq!(parse(br#"{"v":01}"#), Err(IndexError::Integer));
        assert_eq!(parse(br#"{"v":-1}"#), Err(IndexError::Integer));
        assert_eq!(parse(br#"{"a":1}{}"#), Err(IndexError::Malformed));
        assert_eq!(parse(br#""\u0000""#), Err(IndexError::Malformed));
        assert_eq!(parse(br#""\uD800""#), Err(IndexError::Malformed));
        assert!(parse(br#""\uD800\uDC00""#).is_ok());
    }

    #[test]
    fn depth_and_member_caps_match_the_contract_bounds() {
        assert!(parse(nested(31).as_bytes()).is_ok());
        assert_eq!(parse(nested(32).as_bytes()), Err(IndexError::Depth));
        assert!(parse(members(4096).as_bytes()).is_ok());
        assert_eq!(parse(members(4097).as_bytes()), Err(IndexError::TooManyMembers));
    }

    fn nested(arrays: usize) -> String {
        let mut text = String::from("{\"a\":");
        text.push_str(&"[".repeat(arrays));
        text.push('0');
        text.push_str(&"]".repeat(arrays));
        text.push('}');
        text
    }

    fn members(count: usize) -> String {
        let mut text = String::from("{");
        for index in 0..count {
            if index > 0 {
                text.push(',');
            }
            text.push_str(&format!("\"k{index}\":0"));
        }
        text.push('}');
        text
    }
}
