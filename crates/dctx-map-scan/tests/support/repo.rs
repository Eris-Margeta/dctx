// SPDX-License-Identifier: Apache-2.0
use dctx_map_scan::{IgnoreEngine, ScannedFile, adapters::rules::FileRuleSource};
use dctx_map_segment::{
    Completion, Coverage, DeclarationDraft, DeclarationKind, DocId, FactDraft, Field, Language,
    NodeDraft, PostingDraft, SegmentBuilder, SummarySource, TermDraft, ViewKeyDraft,
};
use std::{fs, path::Path};

pub const SEED: u64 = 0xd111_3a5e_2026_1113;

/// Stable LCG, file names and bytes: no platform RNG, timestamps or absolute paths.
pub fn generate(root: &Path, count: usize) {
    let mut seed = SEED;
    let samples = [
        ("rs", "pub fn value() -> u64 { 42 }\n"),
        ("js", "export function value() { return 42; }\n"),
        ("ts", "export const value: number = 42;\n"),
        ("py", "def value():\n    return 42\n"),
        ("go", "package sample\nfunc Value() int { return 42 }\n"),
        ("md", "# Sample\nA deterministic repository fixture.\n"),
    ];
    for i in 0..count {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let (mut ext, source) = samples[(seed >> 32) as usize % samples.len()];
        if i % 31 == 0 {
            ext = "secret";
        } else if i % 29 == 0 {
            ext = "lock";
        } else if i % 37 == 0 {
            ext = "bin";
        }
        let dir = root.join(format!("src/pkg{:02}/nested", i % 64));
        fs::create_dir_all(&dir).unwrap();
        let mut body = format!("// fixture {i:05} {seed:016x}\n");
        for _ in 0..24 {
            body.push_str(source);
        }
        fs::write(dir.join(format!("file{i:05}.{ext}")), body).unwrap();
    }
    // Rule setup is outside the measured scan, and is identical for all sizes.
    fs::write(root.join(".gitignore"), "**/*.secret\n**/scratch?.*\n").unwrap();
}

pub fn engine(root: &Path) -> IgnoreEngine {
    // Built-ins plus project globs, with no machine-specific global rules.
    IgnoreEngine::build_with_source(root, None, &FileRuleSource).unwrap()
}

pub fn builders(records: &[ScannedFile]) -> (SegmentBuilder, SegmentBuilder) {
    let mut fact = SegmentBuilder::fact();
    let mut view = SegmentBuilder::view();
    view.set_view_key(ViewKeyDraft {
        trust: "fixture".into(),
        root: "seeded".into(),
        snapshot: [1; 32],
        overlay_generation: 0,
        policy: [2; 32],
        resolver_revision: "v1".into(),
        environment: [3; 32],
    })
    .unwrap();
    let included: Vec<_> = records
        .iter()
        .filter(|r| r.decision.should_read_content())
        .collect();
    let mut hits = Vec::new();
    for (index, record) in included.iter().enumerate() {
        let ext = record
            .path
            .extension()
            .unwrap_or_default()
            .to_str()
            .unwrap();
        let language = match ext {
            "rs" => Language::Rust,
            "js" => Language::JavaScript,
            "ts" => Language::TypeScript,
            "md" => Language::Markdown,
            _ => Language::Opaque,
        };
        let name = record
            .path
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        fact.add_fact(FactDraft {
            digest: record.hash.unwrap(),
            byte_length: record.size,
            language,
            dialect: ext.into(),
            parser_id: "fixture-parser".into(),
            parser_revision: "v1".into(),
            extraction_plan: [4; 32],
            declarations: vec![DeclarationDraft {
                name: name.clone(),
                kind: DeclarationKind::Function,
                span_start: 0,
                span_end: record.size,
                summary: "value()".into(),
                summary_source: SummarySource::Signature,
                public: true,
            }],
            raw_refs: vec![],
            coverage: Coverage::Syntax,
            completion: Completion::Complete,
            completion_reason: String::new(),
            extractor: "fixture-parser".into(),
            generation: 0,
            model: None,
            body_fingerprint: None,
        })
        .unwrap();
        view.add_node(NodeDraft {
            path: record.path.to_str().unwrap().replace('\\', "/"),
            public_id: name,
            digest: record.hash.unwrap(),
            byte_length: record.size,
            language,
            dialect: ext.into(),
            parser_id: "fixture-parser".into(),
            parser_revision: "v1".into(),
            extraction_plan: [4; 32],
            local_fact_id: 0,
            public: true,
            tier0: false,
        })
        .unwrap();
        hits.push(PostingDraft {
            doc: DocId::Index(u32::try_from(index).unwrap()),
            positions: vec![2, 0],
        });
    }
    fact.set_lengths(vec![24; included.len()]).unwrap();
    fact.add_term(TermDraft {
        field: Field::Identifier,
        term: "value".into(),
        hits,
    })
    .unwrap();
    (fact, view)
}

/// Portable scan framing followed by both complete segment envelopes.
pub fn output(records: &[ScannedFile], fact: &[u8], view: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in records {
        let path = record.path.to_str().unwrap().replace('\\', "/");
        bytes.extend_from_slice(&(path.len() as u64).to_le_bytes());
        bytes.extend_from_slice(path.as_bytes());
        bytes.extend_from_slice(format!("{:?}", record.decision).as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&record.size.to_le_bytes());
        bytes.push(u8::from(record.hash.is_some()));
        if let Some(hash) = record.hash {
            bytes.extend_from_slice(&hash);
        }
    }
    bytes.extend_from_slice(&(fact.len() as u64).to_le_bytes());
    bytes.extend_from_slice(fact);
    bytes.extend_from_slice(&(view.len() as u64).to_le_bytes());
    bytes.extend_from_slice(view);
    bytes
}

// SHA-256 of complete pre-optimization outputs; the 1k output is also checked
// byte-for-byte against the committed golden, larger artifacts are optional.
pub fn baseline_digest(count: usize) -> [u8; 32] {
    match count {
        1_000 => [
            0xf4, 0xe1, 0x30, 0xe6, 0xcf, 0xcd, 0x0d, 0x1a, 0xb2, 0x59, 0xd5, 0xc3, 0x41, 0x12,
            0xf2, 0xa1, 0x77, 0x15, 0x6c, 0xf0, 0x04, 0x57, 0x03, 0x28, 0x22, 0xe9, 0x39, 0x9d,
            0xac, 0x46, 0xbe, 0x4c,
        ],
        10_000 => [
            0x4d, 0xca, 0x16, 0x90, 0xd9, 0xb4, 0x25, 0xcc, 0x9d, 0x34, 0xd3, 0x4b, 0x81, 0x2c,
            0x1d, 0x13, 0x93, 0xa5, 0xc8, 0x91, 0x50, 0xf3, 0xe8, 0xd7, 0x81, 0x76, 0x7f, 0x22,
            0x19, 0x5d, 0x44, 0xa1,
        ],
        50_000 => [
            0x42, 0x5b, 0x39, 0x11, 0x4c, 0x2f, 0xc4, 0x33, 0x2a, 0xe0, 0xff, 0x13, 0xf8, 0x72,
            0x90, 0x86, 0xd0, 0x03, 0x20, 0x5c, 0x21, 0x9a, 0x3f, 0x2d, 0xb7, 0x44, 0xd8, 0xb3,
            0x08, 0x1b, 0x4b, 0xa8,
        ],
        _ => panic!("unsupported fixture size"),
    }
}
