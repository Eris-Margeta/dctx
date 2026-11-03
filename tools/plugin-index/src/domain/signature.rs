// SPDX-License-Identifier: Apache-2.0
//! Detached index signature and local test pins.
//! Implements distribution.md `dctx-plugin-index-v1` || exact index bytes, and decision 081 verification.
//! Depends on `dctx-ed25519` and `dctx-hash`. [`crate::execute`] and the fixture tests call it.
//! The checker never signs. A pin file is a local test record, not a trust bundle.

use super::index::{Index, validate_index};
use super::json::{Json, parse};
use super::text::check_hex64;
use crate::IndexError;
use dctx_hash::Sha256;

/// Domain separator prepended to the exact `index.json` bytes. No NUL or length follows it.
pub const INDEX_DOMAIN: &[u8] = b"dctx-plugin-index-v1";

/// Local test pins for the index key and known publishers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublisherPins {
    index_key_fingerprint: String,
    publishers: Vec<(String, String)>,
}

impl PublisherPins {
    /// Parses a `local-test-pins` document. Private key fields are refused.
    ///
    /// # Errors
    /// Returns [`IndexError`] when the document is not the closed test-pin shape.
    pub fn parse(bytes: &[u8]) -> Result<Self, IndexError> {
        let json = parse(bytes)?;
        let fields = match &json {
            Json::Object(fields) => fields,
            _ => return Err(IndexError::Schema("pins")),
        };
        expect_pin_fields(fields)?;
        match required(fields, "schema")? {
            Json::Number(1) => {}
            _ => return Err(IndexError::Schema("schema")),
        }
        match required(fields, "role")? {
            Json::String(role) if role == "local-test-pins" => {}
            _ => return Err(IndexError::Schema("role")),
        }
        let index_key_fingerprint = match required(fields, "index_key_fingerprint")? {
            Json::String(value) if check_hex64(value) => value.clone(),
            _ => return Err(IndexError::Schema("index_key_fingerprint")),
        };
        let publishers = match required(fields, "publishers")? {
            Json::Array(items) => parse_publishers(items)?,
            _ => return Err(IndexError::Schema("publishers")),
        };
        Ok(Self {
            index_key_fingerprint,
            publishers,
        })
    }

    fn publisher<'a>(&'a self, id: &str) -> Option<&'a str> {
        self.publishers
            .iter()
            .find(|(pinned, _)| pinned == id)
            .map(|(_, fingerprint)| fingerprint.as_str())
    }
}

/// SHA-256 fingerprint of a 32-byte public key, lowercase hex, via `dctx-hash`.
///
/// # Errors
/// Returns [`IndexError::Signature`] when the key length is not 32 bytes.
pub fn fingerprint_hex(public_key: &[u8]) -> Result<String, IndexError> {
    if public_key.len() != 32 {
        return Err(IndexError::Signature);
    }
    let digest = Sha256::digest(public_key).map_err(|_| IndexError::Signature)?;
    Ok(to_hex(digest.as_bytes()))
}

/// Decodes a 64-character lowercase hex test key. One trailing newline is ignored.
///
/// # Errors
/// Returns [`IndexError::Signature`] when the text is not 32 key bytes.
pub fn decode_index_key(bytes: &[u8]) -> Result<[u8; 32], IndexError> {
    let mut text = bytes;
    if text.last() == Some(&b'\n') {
        text = &text[..text.len() - 1];
    }
    if text.last() == Some(&b'\r') {
        text = &text[..text.len() - 1];
    }
    if text.len() != 64 {
        return Err(IndexError::Signature);
    }
    let mut key = [0u8; 32];
    for index in 0..32 {
        key[index] = (nibble(text[index * 2])? << 4) | nibble(text[index * 2 + 1])?;
    }
    Ok(key)
}

/// Builds `ASCII("dctx-plugin-index-v1") || exact_bytes(index)`.
pub fn index_message(index_bytes: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(INDEX_DOMAIN.len() + index_bytes.len());
    message.extend_from_slice(INDEX_DOMAIN);
    message.extend_from_slice(index_bytes);
    message
}

/// Verifies a detached 64-byte signature over the exact index bytes.
///
/// # Errors
/// An empty signature is [`IndexError::UnsignedMetadata`]. Any other failure is [`IndexError::Signature`].
pub fn verify_index_signature(
    index_bytes: &[u8],
    public_key: &[u8],
    signature: &[u8],
) -> Result<(), IndexError> {
    if signature.is_empty() {
        return Err(IndexError::UnsignedMetadata);
    }
    if signature.len() != 64 || public_key.len() != 32 {
        return Err(IndexError::Signature);
    }
    dctx_ed25519::verify(public_key, &index_message(index_bytes), signature).map_err(|_| IndexError::Signature)
}

/// Checks the pinned index key, the detached signature, the schema, and publisher pins.
///
/// A valid signature does not excuse duplicate keys, alias claims, or hostile text.
///
/// # Errors
/// Returns the first refusal from key pin, signature, schema, or publisher comparison.
pub fn check_signed_index(
    index_bytes: &[u8],
    signature: &[u8],
    index_public_key: &[u8],
    pins: &PublisherPins,
) -> Result<Index, IndexError> {
    let actual = fingerprint_hex(index_public_key)?;
    if actual != pins.index_key_fingerprint {
        return Err(IndexError::UnsignedMetadata);
    }
    verify_index_signature(index_bytes, index_public_key, signature)?;
    let index = validate_index(index_bytes)?;
    for plugin in &index.plugins {
        match pins.publisher(&plugin.id) {
            Some(expected) if expected == plugin.publisher_key_fingerprint => {}
            Some(_) => return Err(IndexError::ChangedPublisher),
            None => return Err(IndexError::UnknownPublisher),
        }
    }
    Ok(index)
}

fn parse_publishers(items: &[Json]) -> Result<Vec<(String, String)>, IndexError> {
    let mut publishers = Vec::with_capacity(items.len());
    for item in items {
        let Json::Object(fields) = item else {
            return Err(IndexError::Schema("publisher"));
        };
        expect_pin_fields_publisher(fields)?;
        let Json::String(id) = required(fields, "id")? else {
            return Err(IndexError::Schema("id"));
        };
        let Json::String(fingerprint) = required(fields, "publisher_key_fingerprint")? else {
            return Err(IndexError::Schema("publisher_key_fingerprint"));
        };
        if !check_hex64(fingerprint) {
            return Err(IndexError::Schema("publisher_key_fingerprint"));
        }
        if publishers.iter().any(|(pinned, _)| pinned == id) {
            return Err(IndexError::DuplicateId);
        }
        publishers.push((id.clone(), fingerprint.clone()));
    }
    Ok(publishers)
}

fn expect_pin_fields(fields: &[(String, Json)]) -> Result<(), IndexError> {
    refuse_secret_or_extra(
        fields,
        &["schema", "role", "index_key_fingerprint", "publishers"],
    )
}

fn expect_pin_fields_publisher(fields: &[(String, Json)]) -> Result<(), IndexError> {
    refuse_secret_or_extra(fields, &["id", "publisher_key_fingerprint"])
}

fn refuse_secret_or_extra(fields: &[(String, Json)], allowed: &[&str]) -> Result<(), IndexError> {
    for (key, _) in fields {
        if matches!(key.as_str(), "secret" | "private_key" | "seed") {
            return Err(IndexError::SecretMaterial);
        }
        if !allowed.iter().any(|name| *name == key) {
            return Err(IndexError::UnsignedMetadata);
        }
    }
    if fields.len() != allowed.len() {
        return Err(IndexError::Schema("missing field"));
    }
    Ok(())
}

fn required<'a>(fields: &'a [(String, Json)], name: &str) -> Result<&'a Json, IndexError> {
    fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
        .ok_or(IndexError::Schema("missing field"))
}

fn nibble(byte: u8) -> Result<u8, IndexError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(IndexError::Signature),
    }
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_the_ascii_prefix_with_no_separator() {
        assert!(!INDEX_DOMAIN.contains(&0));
        assert_eq!(index_message(b"{}\n"), b"dctx-plugin-index-v1{}\n");
    }

    #[test]
    fn rfc_test_key_fingerprint_matches_the_pinned_fixture() {
        let key = decode_index_key(
            b"d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a\n",
        )
        .unwrap();
        assert_eq!(
            fingerprint_hex(&key).unwrap(),
            "21fe31dfa154a261626bf854046fd2271b7bed4b6abe45aa58877ef47f9721b9"
        );
    }

    #[test]
    fn empty_signature_is_unsigned_metadata() {
        let key = [0u8; 32];
        assert_eq!(
            verify_index_signature(b"{}", &key, b""),
            Err(IndexError::UnsignedMetadata)
        );
    }
}
