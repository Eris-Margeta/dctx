//! Shared segment fixtures for publication tests.
//! Builds valid `dctx-map-segment` fact bytes and store options.
//! Integration tests share this module. Labels change the fact so digests differ.
//! Empty labels are not used. The store namespace stays `local`.
#![allow(dead_code)]

use dctx_adapter_store::{MemoryFs, PublishRequest, Revisions, SegmentBlob, SegmentRole, SegmentStore, StoreOptions};
use dctx_map_segment::{
    segment_object_digest, Completion, Coverage, DeclarationDraft, DeclarationKind, FactDraft, Language,
    SegmentBuilder, SummarySource,
};

pub fn segment(label: &str) -> Vec<u8> {
    let mut digest = [3u8; 32];
    let raw = label.as_bytes();
    let width = raw.len().min(32);
    digest[..width].copy_from_slice(&raw[..width]);
    let draft = FactDraft {
        digest,
        byte_length: 4,
        language: Language::Rust,
        dialect: "2024".into(),
        parser_id: "structural".into(),
        parser_revision: "1".into(),
        extraction_plan: [2; 32],
        declarations: vec![DeclarationDraft {
            name: label.into(),
            kind: DeclarationKind::Function,
            span_start: 0,
            span_end: 1,
            summary: "note".into(),
            summary_source: SummarySource::Signature,
            public: true,
        }],
        raw_refs: Vec::new(),
        coverage: Coverage::Syntax,
        completion: Completion::Complete,
        completion_reason: String::new(),
        extractor: "dctx".into(),
        generation: 1,
        model: None,
        body_fingerprint: None,
    };
    let mut builder = SegmentBuilder::fact();
    builder.add_fact(draft).expect("fact");
    builder.build().expect("segment")
}

pub fn req(root: &str, generation: u64, bytes: Vec<u8>) -> PublishRequest {
    PublishRequest {
        root: root.to_string(),
        generation,
        snapshot: [4; 32],
        view: [5; 32],
        policy_epoch: 1,
        segments: vec![SegmentBlob { role: SegmentRole::Fact, bytes }],
        revisions: Revisions::structural(),
    }
}

pub fn open(fs: MemoryFs) -> SegmentStore<MemoryFs> {
    SegmentStore::open(fs, StoreOptions::capped(8 * 1024 * 1024)).expect("open")
}

pub fn digest_of(bytes: &[u8]) -> [u8; 32] {
    segment_object_digest(bytes).expect("digest")
}

pub fn segment_rel(bytes: &[u8]) -> String {
    format!("segments/{}.seg", hex(&digest_of(bytes)))
}

fn hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}
