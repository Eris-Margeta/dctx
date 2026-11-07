// SPDX-License-Identifier: Apache-2.0
//! Fact keys and file-local records. Spans are checked against the object byte length.
//! Implements SEGMENTS-v1 kind 3 and IDENTITY-v1 `FactKey` field order. Schema inside the key is 1.
//! Depends on [`super::kinds`] and [`super::dictionary`]. The record codec writes these values.
//! A fact key stores no root, absolute path, or resolved target.

use super::cursor::{Cursor, push_bytes, push_u16, push_u32, push_u64};
use super::dictionary::dict_str;
use super::error::SegmentError;
use super::kinds::{Completion, Coverage, DeclarationKind, EdgeKind, Language, SummarySource};

/// Canonical fact identity. Integer fields use the same order as the on-disk key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FactKey {
    /// SHA-256 of the authorized file bytes.
    pub digest: [u8; 32],
    /// Byte length covered by [`Self::digest`].
    pub byte_length: u64,
    /// Language tag.
    pub language: Language,
    /// Dictionary id of the dialect string.
    pub dialect: u32,
    /// Dictionary id of the parser name.
    pub parser_id: u32,
    /// Dictionary id of the parser revision.
    pub parser_revision: u32,
    /// SHA-256 of the extraction plan.
    pub extraction_plan: [u8; 32],
}

/// One declaration inside a fact. Strings are dictionary ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Declaration {
    /// Dictionary id of the local name.
    pub name: u32,
    /// Declaration tag.
    pub kind: DeclarationKind,
    /// Inclusive start byte, counted from the file start.
    pub span_start: u64,
    /// Exclusive end byte. Must be `<=` the fact's byte length.
    pub span_end: u64,
    /// Dictionary id of the summary text.
    pub summary: u32,
    /// Summary provenance.
    pub summary_source: SummarySource,
    /// Whether ranking treats the declaration as public.
    pub public: bool,
}

/// An unresolved reference. The spelling is not a resolved dense id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawRef {
    /// Dictionary id of the source name.
    pub src: u32,
    /// Relation. `contains` is refused.
    pub kind: EdgeKind,
    /// Dictionary id of the unresolved spelling.
    pub spelling: u32,
    /// Dictionary id of the lexical scope. Empty is allowed.
    pub scope: u32,
    /// Dictionary id of an alias, when the reference has one.
    pub alias: Option<u32>,
    /// One-based inclusive start line.
    pub line_start: u32,
    /// One-based inclusive end line.
    pub line_end: u32,
}

/// Extractor provenance stored once per fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Provenance {
    /// Extraction coverage.
    pub coverage: Coverage,
    /// Extraction completion.
    pub completion: Completion,
    /// Dictionary id of the reason. Zero when completion is complete.
    pub completion_reason: u32,
    /// Dictionary id of the extractor name.
    pub extractor: u32,
    /// Caller-supplied generation.
    pub generation: u64,
    /// Dictionary id of an imported model label.
    pub model: Option<u32>,
    /// Body fingerprint digest, when the caller supplied one.
    pub body_fingerprint: Option<[u8; 32]>,
}

/// One file-local fact block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    /// Identity of the extracted bytes.
    pub key: FactKey,
    /// Declarations in encoder order.
    pub declarations: Vec<Declaration>,
    /// Unresolved references in encoder order.
    pub raw_refs: Vec<RawRef>,
    /// Provenance of this fact.
    pub provenance: Provenance,
}

pub(crate) fn write_object_key(out: &mut Vec<u8>, digest: &[u8; 32], byte_length: u64) {
    push_u16(out, 1);
    push_u16(out, 1);
    push_u16(out, 32);
    push_bytes(out, digest);
    push_u64(out, byte_length);
}

pub(crate) fn write_digest(out: &mut Vec<u8>, digest: &[u8; 32]) {
    push_u16(out, 1);
    push_u16(out, 1);
    push_u16(out, 32);
    push_bytes(out, digest);
}

pub(crate) fn encode_key(key: &FactKey) -> Vec<u8> {
    let mut out = Vec::new();
    write_object_key(&mut out, &key.digest, key.byte_length);
    push_u16(&mut out, key.language.code());
    push_u32(&mut out, key.dialect);
    push_u32(&mut out, key.parser_id);
    push_u32(&mut out, key.parser_revision);
    write_digest(&mut out, &key.extraction_plan);
    push_u16(&mut out, 1);
    out
}

pub(crate) fn read_object_key(cursor: &mut Cursor<'_>) -> Result<([u8; 32], u64), SegmentError> {
    expect_framing(cursor)?;
    let digest = cursor.array::<32>()?;
    Ok((digest, cursor.u64()?))
}

pub(crate) fn read_digest(cursor: &mut Cursor<'_>) -> Result<[u8; 32], SegmentError> {
    expect_framing(cursor)?;
    cursor.array::<32>()
}

fn expect_framing(cursor: &mut Cursor<'_>) -> Result<(), SegmentError> {
    let version = cursor.u16()?;
    let algorithm = cursor.u16()?;
    let len = cursor.u16()?;
    if version != 1 || algorithm != 1 || len != 32 {
        return Err(SegmentError::Facts("key"));
    }
    Ok(())
}

pub(crate) fn parse_key(cursor: &mut Cursor<'_>) -> Result<FactKey, SegmentError> {
    let (digest, byte_length) = read_object_key(cursor)?;
    let language = Language::from_code(cursor.u16()?).ok_or(SegmentError::Facts("kind"))?;
    let dialect = cursor.u32()?;
    let parser_id = cursor.u32()?;
    let parser_revision = cursor.u32()?;
    let extraction_plan = read_digest(cursor)?;
    if cursor.u16()? != 1 {
        return Err(SegmentError::Facts("schema"));
    }
    Ok(FactKey {
        digest,
        byte_length,
        language,
        dialect,
        parser_id,
        parser_revision,
        extraction_plan,
    })
}

pub(crate) fn validate_fact<S: AsRef<str>>(
    fact: &Fact,
    dictionary: &[S],
) -> Result<(), SegmentError> {
    dict_str(dictionary, fact.key.dialect)?;
    dict_str(dictionary, fact.key.parser_id)?;
    dict_str(dictionary, fact.key.parser_revision)?;
    for declaration in &fact.declarations {
        dict_str(dictionary, declaration.name)?;
        dict_str(dictionary, declaration.summary)?;
        if declaration.span_end < declaration.span_start
            || declaration.span_end > fact.key.byte_length
        {
            return Err(SegmentError::Facts("span"));
        }
    }
    for reference in &fact.raw_refs {
        dict_str(dictionary, reference.src)?;
        dict_str(dictionary, reference.spelling)?;
        dict_str(dictionary, reference.scope)?;
        if reference.kind == EdgeKind::Contains {
            return Err(SegmentError::Facts("edge"));
        }
        if let Some(alias) = reference.alias {
            dict_str(dictionary, alias)?;
        }
        if reference.line_start == 0 || reference.line_end < reference.line_start {
            return Err(SegmentError::Facts("lines"));
        }
    }
    dict_str(dictionary, fact.provenance.extractor)?;
    match fact.provenance.completion {
        Completion::Complete => {
            if fact.provenance.completion_reason != 0 {
                return Err(SegmentError::Facts("reason"));
            }
        }
        Completion::Partial | Completion::Refused => {
            if dict_str(dictionary, fact.provenance.completion_reason)?.is_empty() {
                return Err(SegmentError::Facts("reason"));
            }
        }
    }
    if let Some(model) = fact.provenance.model {
        dict_str(dictionary, model)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fact_key_is_one_hundred_bytes_and_spans_must_fit() {
        let key = FactKey {
            digest: [9; 32],
            byte_length: 4,
            language: Language::Rust,
            dialect: 0,
            parser_id: 0,
            parser_revision: 0,
            extraction_plan: [8; 32],
        };
        let encoded = encode_key(&key);
        assert_eq!(encoded.len(), 100);
        assert_eq!(&encoded[..6], &[1, 0, 1, 0, 32, 0]);
        let mut cursor = Cursor::new(&encoded, 0);
        assert_eq!(parse_key(&mut cursor).unwrap(), key);
        assert_eq!(cursor.rest(), []);
        let fact = Fact {
            key,
            declarations: vec![Declaration {
                name: 0,
                kind: DeclarationKind::Function,
                span_start: 0,
                span_end: 4,
                summary: 0,
                summary_source: SummarySource::Signature,
                public: true,
            }],
            raw_refs: vec![],
            provenance: Provenance {
                coverage: Coverage::Syntax,
                completion: Completion::Complete,
                completion_reason: 0,
                extractor: 0,
                generation: 1,
                model: None,
                body_fingerprint: None,
            },
        };
        let dictionary = ["name"];
        validate_fact(&fact, &dictionary).unwrap();
        let mut wide = fact.clone();
        wide.declarations[0].span_end = 5;
        assert_eq!(
            validate_fact(&wide, &dictionary).unwrap_err(),
            SegmentError::Facts("span")
        );
    }
}
