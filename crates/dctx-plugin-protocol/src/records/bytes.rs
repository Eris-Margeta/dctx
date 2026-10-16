//! Raw byte chunks carried as canonical RFC 4648 base64 (`base64` in common.schema.json).
//! Implements plugin-api wire.md §Bytes: standard alphabet, required padding, no whitespace, zero pad bits,
//! decoded size at most 65536 checked before allocation (W-BYTES). Depends on json and refusal.
//! Used by file/state request and result records.

use crate::json::value::Json;
use crate::refusal::{Predicate, Refusal};

/// Largest decoded chunk in bytes.
pub const MAX_CHUNK_BYTES: usize = 65_536;
/// Largest encoded chunk in characters (schema `maxLength`).
pub const MAX_CHUNK_CHARS: usize = 87_384;

/// Decoded chunk bytes (at most [`MAX_CHUNK_BYTES`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Chunk(Vec<u8>);

impl Chunk {
    /// Wraps raw bytes, refusing more than [`MAX_CHUNK_BYTES`].
    pub fn new(bytes: &[u8]) -> Result<Self, Refusal> {
        if bytes.len() > MAX_CHUNK_BYTES {
            return Err(Refusal::semantic(Predicate::WBytes, "chunk-too-large"));
        }
        Ok(Self(bytes.to_vec()))
    }

    /// The raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Number of raw bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the chunk is empty (valid at EOF and as an empty final upload chunk).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Decodes canonical base64 text.
    pub fn decode_base64(text: &str) -> Result<Self, Refusal> {
        let bytes = text.as_bytes();
        if bytes.len() > MAX_CHUNK_CHARS || bytes.len() & 3 != 0 {
            return Err(Refusal::schema("invalid-base64"));
        }
        let padding = bytes.iter().rev().take_while(|b| **b == b'=').count();
        if padding > 2 {
            return Err(Refusal::schema("invalid-base64"));
        }
        let decoded_len = bytes.len() / 4 * 3 - padding;
        if decoded_len > MAX_CHUNK_BYTES {
            return Err(Refusal::semantic(Predicate::WBytes, "chunk-too-large"));
        }
        let body = &bytes[..bytes.len() - padding];
        let mut sextets = Vec::with_capacity(body.len());
        for byte in body {
            sextets.push(sextet(*byte).ok_or(Refusal::schema("invalid-base64"))?);
        }
        let mut out = Vec::with_capacity(decoded_len);
        for group in sextets.chunks(4) {
            let mut acc = 0u32;
            for (index, value) in group.iter().enumerate() {
                acc |= u32::from(*value) << (18 - 6 * index);
            }
            let produced = group.len() * 6 / 8;
            for index in 0..produced {
                out.push(((acc >> (16 - 8 * index)) & 0xFF) as u8);
            }
            let used_bits = 24 - 8 * produced;
            if group.len() < 4 && acc & ((1 << used_bits) - 1) != 0 {
                return Err(Refusal::semantic(Predicate::WBytes, "nonzero-pad-bits"));
            }
        }
        Ok(Self(out))
    }

    /// Canonical base64 text.
    pub fn encode_base64(&self) -> String {
        const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(self.0.len().div_ceil(3) * 4);
        for group in self.0.chunks(3) {
            let mut acc = 0u32;
            for (index, byte) in group.iter().enumerate() {
                acc |= u32::from(*byte) << (16 - 8 * index);
            }
            for index in 0..4 {
                if index <= group.len() {
                    out.push(char::from(ALPHABET[((acc >> (18 - 6 * index)) & 0x3F) as usize]));
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        Self::decode_base64(value.as_str().ok_or(Refusal::schema("expected-string"))?)
    }

    pub(crate) fn encode(&self) -> Json {
        Json::Str(self.encode_base64())
    }
}

fn sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refusal::Stage;

    #[test]
    fn decodes_canonical_rfc4648_vectors() {
        for (plain, encoded) in
            [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]
        {
            let chunk = Chunk::decode_base64(encoded).expect("valid");
            assert_eq!(chunk.as_bytes(), plain.as_bytes());
            assert_eq!(chunk.encode_base64(), encoded);
        }
    }

    #[test]
    fn refuses_noncanonical_spellings() {
        for bad in ["YW Jj", "YWJ", "Zg", "Zg=", "Z===", "Zm9v\n", "Zm-v", "Zg==Zg==", "=Zm9"] {
            let refusal = Chunk::decode_base64(bad).unwrap_err();
            assert_eq!(refusal.stage, Stage::Schema, "{bad}");
        }
        let pad = Chunk::decode_base64("YR==").unwrap_err();
        assert_eq!((pad.stage, pad.reason), (Stage::Semantic, "nonzero-pad-bits"));
        assert_eq!(Chunk::decode_base64("Zm9=").unwrap_err().reason, "nonzero-pad-bits");
    }

    #[test]
    fn size_is_bounded_before_allocation() {
        let max = Chunk::new(&vec![7u8; MAX_CHUNK_BYTES]).expect("max");
        let text = max.encode_base64();
        assert_eq!(Chunk::decode_base64(&text).map(|c| c.len()), Ok(MAX_CHUNK_BYTES));
        assert!(Chunk::new(&vec![0u8; MAX_CHUNK_BYTES + 1]).is_err());
        assert_eq!(text.len(), MAX_CHUNK_CHARS);
        let over = "AAAA".repeat(MAX_CHUNK_CHARS / 4);
        assert_eq!(Chunk::decode_base64(&over).unwrap_err().reason, "chunk-too-large");
        assert_eq!(Chunk::decode_base64(&format!("{text}AAAA")).unwrap_err().reason, "invalid-base64");
        assert!(Chunk::default().is_empty());
    }
}
