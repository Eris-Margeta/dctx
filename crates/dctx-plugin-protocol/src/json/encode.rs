//! Canonical JSON writer used for every encoded frame body and every digest input.
//! Implements plugin-api wire.md §Bytes "Canonical bytes": keys sorted by byte order, no whitespace,
//! decimal integers, `\"`/`\\` and lowercase `\u00xx` control escapes, other Unicode as raw UTF-8.
//! Depends on `json::value`; used by `records::envelope`, frame encoding and action argument digests.

use super::value::Json;

/// Canonical bytes of `value`.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn to_canonical(value: &Json) -> Vec<u8> {
    let mut out = Vec::new();
    write_canonical(value, &mut out);
    out
}

/// Appends the canonical bytes of `value` to `out`.
pub fn write_canonical(value: &Json, out: &mut Vec<u8>) {
    match value {
        Json::Null => out.extend_from_slice(b"null"),
        Json::Bool(true) => out.extend_from_slice(b"true"),
        Json::Bool(false) => out.extend_from_slice(b"false"),
        Json::Int(number) => out.extend_from_slice(number.to_string().as_bytes()),
        Json::Str(text) => write_string(text, out),
        Json::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        Json::Object(members) => {
            let mut sorted: Vec<&(String, Json)> = members.iter().collect();
            sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            out.push(b'{');
            for (index, (key, item)) in sorted.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_string(key, out);
                out.push(b':');
                write_canonical(item, out);
            }
            out.push(b'}');
        }
    }
}

fn write_string(text: &str, out: &mut Vec<u8>) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push(b'"');
    for &byte in text.as_bytes() {
        match byte {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            0x00..=0x1F => {
                out.extend_from_slice(b"\\u00");
                out.push(HEX[usize::from(byte >> 4)]);
                out.push(HEX[usize::from(byte & 0x0F)]);
            }
            other => out.push(other),
        }
    }
    out.push(b'"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::decode::{DecodeLimits, decode_object};

    #[test]
    fn sorts_keys_by_bytes_and_escapes_minimally() {
        let value = Json::Object(vec![
            ("b".into(), Json::str("q\"\\\n\u{1f}/\u{e9}")),
            ("a".into(), Json::Array(vec![Json::Int(-3), Json::Null, Json::Bool(false)])),
            ("B".into(), Json::Object(vec![])),
        ]);
        let text = String::from_utf8(to_canonical(&value)).expect("utf8");
        assert_eq!(text, "{\"B\":{},\"a\":[-3,null,false],\"b\":\"q\\\"\\\\\\u000a\\u001f/\u{e9}\"}");
    }

    #[test]
    fn canonical_output_decodes_back_to_the_same_value() {
        let source = b"{ \"z\": {\"y\": [1, 2]}, \"a\": \"\\u00e9\\t\" }";
        let value = decode_object(source, DecodeLimits::WIRE).expect("valid");
        let again = decode_object(&to_canonical(&value), DecodeLimits::WIRE).expect("valid");
        assert!(value.same_value(&again));
        assert_eq!(to_canonical(&again), to_canonical(&value));
    }
}
