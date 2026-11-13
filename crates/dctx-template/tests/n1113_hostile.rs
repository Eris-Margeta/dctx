// SPDX-License-Identifier: Apache-2.0
//! Public API hostile-input matrix. String APIs receive valid UTF-8 prefixes; byte APIs also
//! receive invalid UTF-8. Structured APIs use empty, singleton, duplicate and reordered values.
//! Limits are exercised at the actual boundary and one beyond. Unbounded legacy APIs use
//! representative large values and their encoded integer boundaries, not multi-gigabyte buffers.
//! Every corpus is fixed or exhaustive, with no clock, network, randomness or timing assertions.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::Barrier;

use dctx_core::FileHandlingDecision::{DebugVisible, Exclude, Include, OmitContent};
use dctx_core::datatypes::NodeType;
use dctx_core::diff::{ChangeType, DiffEntry, DiffReport};
use dctx_core::version::Version;
use dctx_hash::Sha256;
use dctx_template::*;

const PLAN_MAX: usize = 1024 * 1024;
const STRING_MAX: usize = 65_536;
const V1: &str = "[VARIABLES]\na = \"A\"\n[PARTIAL:TREE_ENTRY]\n{{{node.path}}}\n[PARTIAL:FILE_BLOCK]\n{{{file.path}}}:{{{file.content}}}\n[LAYOUT]\n{{a}}|{{{project}}}|{{{comp.tree}}}|{{{comp.files}}}\n";
const V2: &str = "[DTS]\nversion = 2\n[SELECT:s]\nroots = [\"src\"]\n[INDEX:s]\nparser = \"opaque\"\n[BUDGET:b]\nrecords = 4\n[VIEW:pack]\nselection = \"s\"\nbudget = \"b\"\nlayout = \"JSON\"\n";
fn host() -> HostCeiling {
    let mut host = HostCeiling::base();
    host.grants.push(GrantRule {
        pattern: "*".into(),
        decision: Include,
    });
    host
}
fn plan() -> ContextPlan {
    compile(V1, &host()).unwrap()
}
fn record(path: &str, content: &[u8]) -> AnswerRecord {
    AnswerRecord {
        path: path.into(),
        is_dir: false,
        depth: 1,
        size: content.len() as u64,
        modified: u64::MAX,
        content: content.into(),
    }
}
fn serialized(compiled: &CompiledTemplate) -> Vec<u8> {
    let mut bytes = Vec::new();
    compiled.serialize(&mut bytes).unwrap();
    bytes
}
fn outer_hash(bytes: &mut [u8]) {
    let end = bytes.len() - 32;
    let hash = Sha256::digest(&bytes[..end]).unwrap();
    bytes[end..].copy_from_slice(hash.as_bytes());
}
fn sections(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut at = 32;
    (0..7)
        .map(|_| {
            let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            let payload = bytes[at + 8..at + 8 + len].to_vec();
            at += 8 + len;
            payload
        })
        .collect()
}
fn domain(kind: u16, schema: u16, payload: &[u8]) -> [u8; 32] {
    let pre = [
        b"DCTX\0".as_slice(),
        &kind.to_le_bytes(),
        &schema.to_le_bytes(),
        &(payload.len() as u64).to_le_bytes(),
        payload,
    ]
    .concat();
    *Sha256::digest(&pre).unwrap().as_bytes()
}
// Re-sign a hostile payload so integrity checks do not mask decoder coverage.
fn framed(mut payloads: Vec<Vec<u8>>, capabilities: u64) -> Vec<u8> {
    let mut pre = vec![1, 0, 1, 0, 0, 0]; // source v1, IR 1.0
    for (index, payload) in payloads[..6].iter().enumerate() {
        pre.extend_from_slice(&((index + 1) as u16).to_le_bytes());
        pre.extend_from_slice(&1u16.to_le_bytes());
        pre.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        pre.extend_from_slice(payload);
    }
    let digests = [
        domain(4, 1, &payloads[0]),
        domain(5, 1, &payloads[1]),
        domain(8, 2, &payloads[4]),
        domain(8, 3, &payloads[5]),
        domain(8, 1, &pre),
    ];
    let mut provenance = vec![1, 0, 0, 0];
    for digest in digests {
        provenance.extend_from_slice(&digest);
    }
    payloads[6] = provenance;
    let mut bytes = b"DCTXIR\0\0".to_vec();
    bytes.extend_from_slice(&[1, 0, 0, 0]);
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&capabilities.to_le_bytes());
    bytes.extend_from_slice(&7u32.to_le_bytes());
    for (index, payload) in payloads.iter().enumerate() {
        bytes.extend_from_slice(&((index + 1) as u16).to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
    }
    bytes.extend_from_slice(&[0; 32]);
    let len = bytes.len() as u64;
    bytes[12..20].copy_from_slice(&len.to_le_bytes());
    outer_hash(&mut bytes);
    bytes
}

#[test]
fn source_empty_single_byte_and_every_truncation_are_repeatable() {
    let host = host();
    for sample in ["", "x", V1, V2, "[LAYOUT]\né💣{{{project}}}"] {
        for end in 0..=sample.len() {
            if let Ok(prefix) = std::str::from_utf8(&sample.as_bytes()[..end]) {
                assert_eq!(
                    compile(prefix, &host),
                    compile(prefix, &host),
                    "offset {end}"
                );
                let template = Template::parse(prefix);
                assert_eq!(template, Template::parse(prefix));
                assert_eq!(template.validate(), template.validate());
                assert_eq!(template.compile("x"), template.compile("x"));
                let vars = HashMap::from([("x".into(), "value".into())]);
                assert_eq!(replace_vars(prefix, &vars), replace_vars(prefix, &vars));
            }
        }
    }
    for byte in 0u8..=127 {
        let input = char::from(byte).to_string();
        let _ = compile(&input, &host);
    }
}

#[test]
fn source_maximum_and_over_maximum_are_exact() {
    let mut source = String::from(V1);
    source.push('#');
    source.extend(std::iter::repeat_n('x', PLAN_MAX - source.len()));
    assert_eq!(source.len(), PLAN_MAX);
    assert!(compile(&source, &host()).is_ok());
    source.push('x');
    assert_eq!(
        compile(&source, &host()),
        Err(TemplateError::Limit {
            what: "source",
            limit: PLAN_MAX as u64
        })
    );
    assert!(matches!(
        compile("[LAYOUT]\na\0b", &host()),
        Err(TemplateError::InvalidSyntax { .. })
    ));
}

#[test]
fn malformed_source_duplicate_fields_and_sections_are_refused() {
    for bad in [
        "[DTS]\nversion = 2\nversion = 2",
        "[DTS]\nversion = 2\n[DTS]\nversion = 2",
        "[DTS]\nversion = 4294967296",
        "[DTS]\nversion = 0",
        "[DTS]\nversion = -1",
        "[DTS]\nversion = 2\n[UNKNOWN:x]",
        "[DTS]\nversion = 2\n[BUDGET:b]\nbytes = 18446744073709551616",
        "[DTS]\nversion = 2\n[SELECT:s]\nroots = [\"x\" \"y\"]",
        "[DTS]\nversion = 2\n[SELECT:s]\nroots = [\"../x\"]",
    ] {
        assert!(compile(bad, &host()).is_err(), "{bad}");
    }
    for duplicate in [
        "[SELECT:s]\nroots = [\"src\"]",
        "[BUDGET:b]\nrecords = 1",
        "[VIEW:pack]\nselection = \"s\"\nbudget = \"b\"",
    ] {
        assert!(matches!(
            compile(&format!("{V2}{duplicate}\n"), &host()),
            Err(TemplateError::Duplicate { .. })
        ));
    }
}

#[test]
fn plan_bytes_empty_single_byte_every_truncation_and_replay() {
    let plan = plan();
    let bytes = to_bytes(&plan).unwrap();
    for end in 0..bytes.len() {
        assert!(from_bytes(&bytes[..end]).is_err(), "offset {end}");
        assert_eq!(from_bytes(&bytes[..end]), load(&bytes[..end], &host()));
    }
    for byte in 0u8..=255 {
        assert!(from_bytes(&[byte]).is_err());
    }
    assert_eq!(to_bytes(&from_bytes(&bytes).unwrap()).unwrap(), bytes);
    assert_eq!(to_bytes(&load(&bytes, &host()).unwrap()).unwrap(), bytes);
    assert_eq!(to_bytes(&plan).unwrap(), to_bytes(&plan).unwrap());
    assert_eq!(
        plan.map_facts(),
        (plan.selection_digest, plan.extraction_digest)
    );
}

#[test]
fn plan_byte_mutations_and_trailing_bytes_never_panic() {
    let bytes = to_bytes(&plan()).unwrap();
    for at in 0..bytes.len() {
        let mut bad = bytes.clone();
        bad[at] ^= 0xff;
        assert!(from_bytes(&bad).is_err(), "offset {at}");
        if at < bad.len() - 32 {
            outer_hash(&mut bad);
            let first = from_bytes(&bad);
            assert_eq!(first, from_bytes(&bad));
            assert_eq!(load(&bad, &host()), load(&bad, &host()));
        }
    }
    let mut bad = bytes;
    bad.push(0);
    assert!(from_bytes(&bad).is_err());
}

#[test]
fn plan_string_and_envelope_size_boundaries() {
    let mut plan = plan();
    plan.render.strings.push("x".repeat(STRING_MAX));
    let bytes = to_bytes(&plan).unwrap();
    assert_eq!(
        from_bytes(&bytes)
            .unwrap()
            .render
            .strings
            .last()
            .unwrap()
            .len(),
        STRING_MAX
    );
    plan.render.strings.last_mut().unwrap().push('x');
    assert!(matches!(
        to_bytes(&plan),
        Err(TemplateError::Limit { what: "string", .. })
    ));
    plan.render.strings.pop();
    let baseline = to_bytes(&plan).unwrap().len();
    let extra = PLAN_MAX - baseline;
    let count = extra.div_ceil(STRING_MAX);
    let mut left = extra - 4 * count;
    for _ in 0..count {
        let len = left.min(STRING_MAX);
        plan.render.strings.push("x".repeat(len));
        left -= len;
    }
    assert_eq!(left, 0);
    let bytes = to_bytes(&plan).unwrap();
    assert_eq!(bytes.len(), PLAN_MAX);
    assert!(from_bytes(&bytes).is_ok());
    assert!(load(&bytes, &host()).is_ok());
    plan.render.strings.last_mut().unwrap().push('x');
    assert!(matches!(
        to_bytes(&plan),
        Err(TemplateError::Limit { what: "plan", .. })
    ));
    let mut over = bytes;
    over.push(0);
    assert!(matches!(
        from_bytes(&over),
        Err(TemplateError::Limit { what: "plan", .. })
    ));
}

#[test]
fn encoder_and_decoder_agree_on_instruction_and_collection_caps() {
    let mut plan = plan();
    plan.render.partials.clear();
    plan.render.layouts[0].instructions = vec![Inst { opcode: 2, arg: 13 }; 10_000];
    assert_eq!(plan.render.instruction_count(), 10_000);
    assert!(from_bytes(&to_bytes(&plan).unwrap()).is_ok());
    plan.render.layouts[0]
        .instructions
        .push(Inst { opcode: 2, arg: 13 });
    assert!(matches!(
        to_bytes(&plan),
        Err(TemplateError::Limit {
            what: "instructions",
            limit: 10_000
        })
    ));
    plan.render.layouts[0].instructions.clear();
    plan.render.variables = vec![(String::new(), String::new()); 100_000];
    assert!(from_bytes(&to_bytes(&plan).unwrap()).is_ok());
    plan.render.variables.push((String::new(), String::new()));
    assert!(matches!(
        to_bytes(&plan),
        Err(TemplateError::Limit {
            what: "count",
            limit: 100_000
        })
    ));
    plan.render.variables.clear();
    plan.plugins.order = vec![String::new(); 64];
    assert!(from_bytes(&to_bytes(&plan).unwrap()).is_ok());
    plan.plugins.order.push(String::new());
    assert!(matches!(
        to_bytes(&plan),
        Err(TemplateError::Limit {
            what: "count",
            limit: 64
        })
    ));
}

#[test]
fn host_plan_section_and_instruction_caps_have_positive_controls() {
    let bytes = to_bytes(&plan()).unwrap();
    let mut host = host();
    host.max_plan_bytes = bytes.len() as u32;
    assert!(compile(V1, &host).is_ok());
    host.max_plan_bytes -= 1;
    assert!(matches!(
        compile(V1, &host),
        Err(TemplateError::Limit { what: "plan", .. })
    ));
    host.max_plan_bytes = u32::MAX;
    host.max_sections = 7;
    assert!(compile(V1, &host).is_ok());
    host.max_sections = 6;
    assert!(matches!(
        compile(V1, &host),
        Err(TemplateError::Limit {
            what: "sections",
            ..
        })
    ));
    host.max_sections = u32::MAX;
    host.max_instructions = plan().render.instruction_count() as u32;
    assert!(compile(V1, &host).is_ok());
    host.max_instructions -= 1;
    assert!(matches!(
        compile(V1, &host),
        Err(TemplateError::Limit {
            what: "instructions",
            ..
        })
    ));
}

#[test]
fn signed_hostile_payloads_reach_utf8_length_count_and_opcode_checks() {
    let plan = plan();
    let bytes = to_bytes(&plan).unwrap();
    let payloads = sections(&bytes);
    assert_eq!(framed(payloads.clone(), plan.capabilities), bytes);
    let mut bad = payloads.clone();
    // Selection: one name with one invalid UTF-8 byte, followed by empty lists.
    bad[0] = [
        1u32.to_le_bytes().as_slice(),
        &1u32.to_le_bytes(),
        &[0xff],
        &[0; 16],
    ]
    .concat();
    assert_eq!(
        from_bytes(&framed(bad, plan.capabilities)),
        Err(TemplateError::Corrupt { reason: "utf8" })
    );
    for count in [100_001u32, u32::MAX] {
        let mut bad = payloads.clone();
        bad[0] = count.to_le_bytes().to_vec();
        assert!(matches!(
            from_bytes(&framed(bad, plan.capabilities)),
            Err(TemplateError::Limit { what: "count", .. })
        ));
    }
    for len in [65_537u32, u32::MAX] {
        let mut bad = payloads.clone();
        bad[0] = [1u32.to_le_bytes(), len.to_le_bytes()].concat();
        assert!(matches!(
            from_bytes(&framed(bad, plan.capabilities)),
            Err(TemplateError::Limit { what: "string", .. })
        ));
    }
    for opcode in [0u16, 4, u16::MAX] {
        let mut bad = payloads.clone();
        let len = bad[4].len();
        bad[4][len - 4..len - 2].copy_from_slice(&opcode.to_le_bytes());
        assert_eq!(
            from_bytes(&framed(bad, plan.capabilities)),
            Err(TemplateError::UnknownOpcode { opcode })
        );
    }
    for index in [0usize, 1, 2, 3, 4, 5] {
        for end in 0..payloads[index].len() {
            let mut bad = payloads.clone();
            bad[index].truncate(end);
            assert!(
                from_bytes(&framed(bad, plan.capabilities)).is_err(),
                "section {index} offset {end}"
            );
        }
    }
}

#[test]
fn duplicated_reordered_unknown_sections_and_schemas_are_refused() {
    let bytes = to_bytes(&plan()).unwrap();
    for (kind, expected) in [
        (2u16, "order"),
        (0, "unknown"),
        (8, "unknown"),
        (u16::MAX, "unknown"),
    ] {
        let mut bad = bytes.clone();
        bad[32..34].copy_from_slice(&kind.to_le_bytes());
        outer_hash(&mut bad);
        match (expected, from_bytes(&bad)) {
            (
                "order",
                Err(TemplateError::Corrupt {
                    reason: "section order",
                }),
            )
            | ("unknown", Err(TemplateError::UnknownSection { .. })) => {}
            (_, other) => panic!("unexpected {other:?}"),
        }
    }
    let mut bad = bytes.clone();
    bad[34..36].copy_from_slice(&2u16.to_le_bytes());
    outer_hash(&mut bad);
    assert!(matches!(
        from_bytes(&bad),
        Err(TemplateError::UnsupportedVersion { found: 2 })
    ));
    for count in [0u32, 6, 8, 64, 65, u32::MAX] {
        let mut bad = bytes.clone();
        bad[28..32].copy_from_slice(&count.to_le_bytes());
        outer_hash(&mut bad);
        assert!(from_bytes(&bad).is_err());
    }
    let first_len = u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize;
    let mut bad = bytes;
    let second = 40 + first_len;
    bad[second..second + 2].copy_from_slice(&1u16.to_le_bytes());
    outer_hash(&mut bad);
    assert_eq!(
        from_bytes(&bad),
        Err(TemplateError::Corrupt {
            reason: "section order"
        })
    );
}

#[test]
fn legacy_decode_empty_single_byte_and_every_truncation() {
    let compiled = Template::parse(V1).compile("é").unwrap();
    let bytes = serialized(&compiled);
    for end in 0..bytes.len() {
        assert!(
            CompiledTemplate::deserialize(&mut &bytes[..end]).is_err(),
            "offset {end}"
        );
    }
    for byte in 0u8..=255 {
        assert!(CompiledTemplate::deserialize(&mut &[byte][..]).is_err());
    }
    let decoded = CompiledTemplate::deserialize(&mut bytes.as_slice()).unwrap();
    assert_eq!(decoded, compiled);
    assert_eq!(serialized(&decoded), bytes);
    assert_eq!(decoded.to_template(), compiled.to_template());
    assert!(decoded.to_template().validate().is_empty());
}

struct BoundedRead<'a>(&'a [u8]);
impl Read for BoundedRead<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        assert!(
            buffer.len() <= 8192,
            "untrusted length reached reader: {}",
            buffer.len()
        );
        self.0.read(buffer)
    }
}

#[test]
fn legacy_huge_claimed_lengths_fail_without_preallocating() {
    // The reader probes allocation behavior without allocating a multi-gigabyte sample.
    for len in [0u32, 1, 65_536, u32::MAX] {
        let bytes = [
            b"DCTXTMPL".as_slice(),
            &1u32.to_le_bytes(),
            &len.to_le_bytes(),
        ]
        .concat();
        let error = CompiledTemplate::deserialize(&mut BoundedRead(&bytes)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }
    let mut bytes = b"DCTXTMPL".to_vec();
    for value in [1u32, 0, u32::MAX] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        CompiledTemplate::deserialize(&mut BoundedRead(&bytes))
            .unwrap_err()
            .kind(),
        io::ErrorKind::UnexpectedEof
    );
}

#[test]
fn legacy_invalid_utf8_magic_versions_and_duplicate_variables_are_refused() {
    for invalid in [
        vec![0xff],
        vec![0xc0, 0x80],
        vec![0xe2, 0x82],
        vec![0xed, 0xa0, 0x80],
    ] {
        let bytes = [
            b"DCTXTMPL".as_slice(),
            &1u32.to_le_bytes(),
            &(invalid.len() as u32).to_le_bytes(),
            &invalid,
        ]
        .concat();
        assert_eq!(
            CompiledTemplate::deserialize(&mut bytes.as_slice())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
    let bytes = serialized(&Template::parse(V1).compile("").unwrap());
    for version in [0u32, 2, u32::MAX] {
        let mut bad = bytes.clone();
        bad[8..12].copy_from_slice(&version.to_le_bytes());
        assert_eq!(
            CompiledTemplate::deserialize(&mut bad.as_slice())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
    let mut bad = bytes;
    bad[0] ^= 0xff;
    assert!(CompiledTemplate::deserialize(&mut bad.as_slice()).is_err());
    let mut duplicate = b"DCTXTMPL".to_vec();
    for value in [1u32, 0, 2] {
        duplicate.extend_from_slice(&value.to_le_bytes());
    }
    for value in [b"x", b"y"] {
        duplicate.extend_from_slice(&1u16.to_le_bytes());
        duplicate.push(b'a');
        duplicate.extend_from_slice(&1u32.to_le_bytes());
        duplicate.extend_from_slice(value);
    }
    duplicate.extend_from_slice(&[0; 12]);
    assert_eq!(
        CompiledTemplate::deserialize(&mut duplicate.as_slice())
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn legacy_variable_name_size_and_reordered_serialization() {
    let mut compiled = Template::parse(V1).compile("").unwrap();
    compiled.variables.clear();
    compiled
        .variables
        .insert("x".repeat(u16::MAX as usize), "é".repeat(STRING_MAX));
    let bytes = serialized(&compiled);
    assert_eq!(
        CompiledTemplate::deserialize(&mut bytes.as_slice()).unwrap(),
        compiled
    );
    compiled
        .variables
        .insert("y".repeat(u16::MAX as usize + 1), String::new());
    assert_eq!(
        compiled.serialize(&mut Vec::new()).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    let mut left = Template::parse(V1).compile("x").unwrap();
    let mut right = left.clone();
    left.variables = HashMap::from([("b".into(), "B".into()), ("a".into(), "A".into())]);
    right.variables = HashMap::from([("a".into(), "A".into()), ("b".into(), "B".into())]);
    assert_eq!(serialized(&left), serialized(&right));
}

#[test]
fn variable_substitution_uses_lexical_order_and_preserves_runtime_fields() {
    let vars = HashMap::from([("a".into(), "{{b}}".into()), ("b".into(), "END".into())]);
    assert_eq!(replace_vars("{{a}}", &vars), "END");
    let left = "[VARIABLES]\na = \"{{b}}\"\nb = \"{{c}}\"\nc = \"C\"\n[LAYOUT]\n{{a}}";
    let right = "[VARIABLES]\nc = \"C\"\nb = \"{{c}}\"\na = \"{{b}}\"\n[LAYOUT]\n{{a}}";
    assert_eq!(Template::parse(left), Template::parse(right));
    assert_eq!(
        to_bytes(&compile(left, &host()).unwrap()).unwrap(),
        to_bytes(&compile(right, &host()).unwrap()).unwrap()
    );
    let vars = HashMap::from([("project".into(), "VALUE".into())]);
    assert_eq!(
        replace_vars("{{project}} {{{project}}} {{{project", &vars),
        "VALUE {{{project}}} {{{project"
    );
    assert_eq!(replace_vars("", &vars), "");
    assert_eq!(replace_vars("x", &HashMap::new()), "x");
}

#[test]
fn cyclic_variables_are_detected_before_amplification() {
    let source = "[VARIABLES]\na = \"{{a}}{{a}}{{a}}\"\n[LAYOUT]\n{{a}}";
    let template = Template::parse(source);
    assert_eq!(template.variables["a"], "{{a}}{{a}}{{a}}");
    assert_eq!(template.cycles, ["a"]);
    assert!(template.compile("").is_err());
    assert!(matches!(
        compile(source, &host()),
        Err(TemplateError::Cycle {
            kind: "variable",
            ..
        })
    ));
    let mut template = Template::parse("[LAYOUT]\n{{missing}}");
    assert!(!template.validate().is_empty());
    template.insert_variable("missing", "x");
    template.insert_variable("missing", "y");
    assert!(template.validate().is_empty());
    assert_eq!(template.compile("").unwrap().layout, "y");
}

#[test]
fn plugin_state_grants_cannot_bypass_denials_or_quota_with_source_content() {
    let mut host = host();
    let mut plugin = KnownPlugin::named("ex/state", "1.0.0");
    plugin.capabilities = vec!["state:read".into()];
    plugin.records = vec![Capability::State {
        read: true,
        write: false,
        reclaim: false,
        max_bytes: 8,
        source_content: true,
    }];
    host.plugins.push(plugin);
    let original = compile(V1, &host).unwrap();
    let bytes = to_bytes(&original).unwrap();
    assert!(load(&bytes, &host).is_ok());
    for source_content in [false, true] {
        for max_bytes in [0, 9, u64::MAX] {
            let mut bad = original.clone();
            bad.plugins.plugins[0].allow = vec![Capability::State {
                read: true,
                write: false,
                reclaim: false,
                max_bytes,
                source_content,
            }];
            assert!(matches!(
                load(&to_bytes(&bad).unwrap(), &host),
                Err(TemplateError::GrantExpansion { kind: "plugin" })
            ));
        }
    }
    for cap in [
        Capability::State {
            read: false,
            write: true,
            reclaim: false,
            max_bytes: 1,
            source_content: true,
        },
        Capability::State {
            read: false,
            write: false,
            reclaim: false,
            max_bytes: 1,
            source_content: true,
        },
    ] {
        let mut bad = original.clone();
        bad.plugins.plugins[0].allow = vec![cap];
        assert!(load(&to_bytes(&bad).unwrap(), &host).is_err());
    }
    host.denied_capabilities
        .insert("*".into(), ["state:read".into()].into());
    assert!(matches!(
        load(&bytes, &host),
        Err(TemplateError::GrantExpansion { kind: "plugin" })
    ));
}

#[test]
fn plugin_counts_duplicate_declarations_and_catalog_reordering() {
    let mut host = host();
    for index in 0..64 {
        host.plugins
            .push(KnownPlugin::named(format!("ex/p{index}"), "1.0.0"));
    }
    let bytes = to_bytes(&compile(V1, &host).unwrap()).unwrap();
    assert_eq!(load(&bytes, &host).unwrap().plugins.plugins.len(), 64);
    host.plugins.reverse();
    assert_eq!(to_bytes(&compile(V1, &host).unwrap()).unwrap(), bytes);
    host.plugins.push(KnownPlugin::named("ex/over", "1.0.0"));
    assert!(matches!(
        compile(V1, &host),
        Err(TemplateError::Limit { .. })
    ));
    let duplicate =
        format!("{V2}[PLUGIN:ex/p0]\nversion = \"1.0.0\"\n[PLUGIN:ex/p0]\nversion = \"1.0.0\"\n");
    assert!(matches!(
        compile(&duplicate, &host),
        Err(TemplateError::Duplicate { .. })
    ));
    for text in ["", "x", &"x".repeat(257)] {
        let row = KnownPlugin::named(text, text);
        assert_eq!(row.id, text);
        assert_eq!(row.version, text);
    }
}

#[test]
fn plugin_config_fields_empty_singleton_max_over_and_reordered_values() {
    let mut host = host();
    let mut plugin = KnownPlugin::named("ex/config", "1.0.0");
    plugin.fields = vec![
        FieldSpec {
            name: "text".into(),
            kind: FieldKind::Str { max_bytes: 4 },
            default_value: None,
        },
        FieldSpec {
            name: "list".into(),
            kind: FieldKind::List {
                max_items: 2,
                item_max_bytes: 1,
            },
            default_value: None,
        },
        FieldSpec {
            name: "number".into(),
            kind: FieldKind::Int { min: 0, max: 1 },
            default_value: None,
        },
    ];
    host.plugins.push(plugin);
    for text in ["", "x", "éé"] {
        let source = format!(
            "{V2}[PLUGIN:ex/config]\nversion = \"1.0.0\"\nconfig.text = \"{text}\"\nconfig.list = []\nconfig.number = 0\n"
        );
        let plan = compile(&source, &host).unwrap();
        assert_eq!(load(&to_bytes(&plan).unwrap(), &host).unwrap(), plan);
    }
    for value in ["\"ééx\"", "\"${X}\"", "\"/program\""] {
        let source =
            format!("{V2}[PLUGIN:ex/config]\nversion = \"1.0.0\"\nconfig.text = {value}\n");
        assert!(compile(&source, &host).is_err());
    }
    for value in ["[\"x\", \"y\", \"z\"]", "[\"é\"]"] {
        assert!(
            compile(
                &format!("{V2}[PLUGIN:ex/config]\nversion = \"1.0.0\"\nconfig.list = {value}\n"),
                &host
            )
            .is_err()
        );
    }
    let left = format!(
        "{V2}[PLUGIN:ex/config]\nversion = \"1.0.0\"\nconfig.list = [\"x\", \"y\"]\nconfig.number = 1\n"
    );
    let right = format!(
        "{V2}[PLUGIN:ex/config]\nversion = \"1.0.0\"\nconfig.number = 1\nconfig.list = [\"x\", \"y\"]\n"
    );
    assert_eq!(
        compile(&left, &host).unwrap(),
        compile(&right, &host).unwrap()
    );
    assert!(
        compile(
            &left.replace("config.number = 1", "config.number = 2"),
            &host
        )
        .is_err()
    );
}

#[test]
fn source_list_boundary_and_budget_integer_boundaries() {
    let roots = std::iter::repeat_n("\"src\"", 1024)
        .collect::<Vec<_>>()
        .join(",");
    let source = V2.replace("roots = [\"src\"]", &format!("roots = [{roots}]"));
    assert!(compile(&source, &host()).is_ok());
    assert!(matches!(
        compile(
            &source.replace(&roots, &format!("{roots},\"src\"")),
            &host()
        ),
        Err(TemplateError::Limit { .. })
    ));
    for count in [0u64, 1, u64::MAX] {
        let source = V2.replace("records = 4", &format!("records = {count}"));
        let mut host = host();
        host.max_records = count;
        assert_eq!(compile(&source, &host).unwrap().budgets[0].records, count);
        if count != u64::MAX {
            assert!(matches!(
                compile(
                    &source.replace(
                        &format!("records = {count}"),
                        &format!("records = {}", count + 1)
                    ),
                    &host
                ),
                Err(TemplateError::BudgetAboveCeiling { .. })
            ));
        }
    }
}

#[test]
fn records_empty_singleton_duplicates_and_reordering_are_canonical() {
    let plan = plan();
    let host = host();
    let a = record("a", b"A");
    let b = record("b", b"B");
    for records in [
        vec![],
        vec![a.clone()],
        vec![a.clone(), a.clone()],
        vec![b.clone(), a.clone(), a.clone()],
    ] {
        let expected = render_plan(&plan, "pack", &records, "é", &host).unwrap();
        let mut reversed = records.clone();
        reversed.reverse();
        assert_eq!(
            expected,
            render_plan(&plan, "pack", &records, "é", &host).unwrap()
        );
        assert_eq!(
            expected,
            render_plan(&plan, "pack", &reversed, "é", &host).unwrap()
        );
    }
    for missing in ["", "x", &"x".repeat(STRING_MAX + 1)] {
        assert!(matches!(
            render_plan(&plan, missing, &[], "", &host),
            Err(TemplateError::UnresolvedRef { kind: "view", .. })
        ));
    }
    for project in ["", "x", &"é".repeat(STRING_MAX)] {
        assert_eq!(
            render_plan(&plan, "pack", &[], project, &host).unwrap(),
            render_plan(&plan, "pack", &[], project, &host).unwrap()
        );
    }
}

#[test]
fn record_count_maximum_and_over_maximum() {
    let plan = plan();
    let records = vec![record("x", b""); 100_000];
    // Denied records keep the boundary test small while still traversing the full input.
    let denied = HostCeiling::base();
    assert!(render_plan(&plan, "pack", &records, "", &denied).is_ok());
    let mut over = records;
    over.push(record("x", b""));
    assert_eq!(
        render_plan(&plan, "pack", &over, "", &denied),
        Err(TemplateError::Limit {
            what: "records",
            limit: 100_000
        })
    );
}

#[test]
fn rendered_envelopes_fit_at_exact_bytes_and_refuse_one_less() {
    for source in [V1, V2] {
        let mut plan = compile(source, &host()).unwrap();
        let records = [record("src/x", b"\xff\"\n")];
        let text = render_plan(&plan, "pack", &records, "", &host()).unwrap();
        plan.budgets[0].bytes = text.len() as u64;
        assert_eq!(
            render_plan(&plan, "pack", &records, "", &host()).unwrap(),
            text
        );
        plan.budgets[0].bytes -= 1;
        match render_plan(&plan, "pack", &records, "", &host()) {
            Ok(limited) => {
                assert!(limited.len() < text.len());
                assert!(!limited.contains('�'));
            }
            Err(TemplateError::BudgetTooSmall { .. }) => {}
            other => panic!("unexpected {other:?}"),
        }
        plan.budgets[0].bytes = 0;
        assert!(matches!(
            render_plan(&plan, "pack", &records, "", &host()),
            Err(TemplateError::BudgetTooSmall { .. })
        ));
    }
}

#[test]
fn authorization_all_decisions_path_prefixes_and_ordered_rules() {
    let decisions = [Exclude, DebugVisible, OmitContent, Include];
    let ranks = [(0, 0), (1, 1), (1, 2), (2, 2)];
    for (hi, host) in decisions.iter().copied().enumerate() {
        for (ri, requested) in decisions.iter().copied().enumerate() {
            let result = narrow_decision(host, requested);
            assert_eq!(
                result.is_ok(),
                ranks[ri].0 <= ranks[hi].0 && ranks[ri].1 <= ranks[hi].1
            );
            if let Ok(decision) = result {
                assert_eq!(decision, requested);
            }
        }
    }
    let mut host = host();
    let mut selection = Selection {
        name: "s".into(),
        roots: vec![],
        languages: vec![],
        kinds: vec![],
        rules: vec![],
    };
    for text in ["", "x", "é", "src/x", &"x".repeat(4096), &"x".repeat(4097)] {
        assert_eq!(
            authorize_path(&host, &selection, text),
            authorize_path(&host, &selection, text)
        );
    }
    host.grants.push(GrantRule {
        pattern: "secret".into(),
        decision: Exclude,
    });
    assert!(authorize_path(&host, &selection, "secret").is_err());
    selection.rules.push(SelectRule {
        pattern: "secret".into(),
        decision: Exclude,
    });
    assert_eq!(authorize_path(&host, &selection, "secret"), Ok(Exclude));
    selection.roots = vec!["src".into()];
    assert_eq!(authorize_path(&host, &selection, "src2/x"), Ok(Exclude));
    assert_eq!(authorize_path(&host, &selection, "src/x"), Ok(Include));
}

fn diff(path: &str) -> DiffReport {
    DiffReport {
        entries: vec![DiffEntry {
            change_type: ChangeType::Modified,
            node_type: NodeType::File,
            relative_path: path.into(),
        }],
        ..DiffReport::default()
    }
}
fn legacy_equivalence(template: &Template, root: &MarkerNode, tree_only: bool) {
    let version = Version::new_v1();
    let report = diff(&root.relative_path);
    let capture = render_legacy_snapshot(template, root, &version, tree_only, false);
    assert_eq!(
        render_legacy_snapshot(template, root, &version, tree_only, false),
        capture
    );
    let mut output = Vec::new();
    write_legacy_snapshot(&mut output, template, root, &version, tree_only, false).unwrap();
    assert_eq!(output, capture.as_bytes());
    assert_eq!(
        template
            .legacy_snapshot_within(root, &version, tree_only, false, capture.len() as u64)
            .unwrap(),
        capture
    );
    if !capture.is_empty() {
        assert!(matches!(
            template.legacy_snapshot_within(
                root,
                &version,
                tree_only,
                false,
                capture.len() as u64 - 1
            ),
            Err(TemplateError::BudgetTooSmall { .. })
        ));
    }
    assert!(matches!(
        template.legacy_snapshot_within(root, &version, tree_only, false, 0),
        Err(TemplateError::BudgetTooSmall { .. })
    ));
    let changed = render_legacy_diff(template, root, &report, &version, &version, false);
    assert_eq!(
        render_legacy_diff(template, root, &report, &version, &version, false),
        changed
    );
    let mut written = Vec::new();
    write_legacy_diff(
        &mut written,
        template,
        root,
        &report,
        &version,
        &version,
        false,
    )
    .unwrap();
    assert_eq!(written, changed.as_bytes());
    assert_eq!(
        template
            .legacy_diff_within(
                root,
                &report,
                &version,
                &version,
                false,
                changed.len() as u64
            )
            .unwrap(),
        changed
    );
    assert!(matches!(
        template.legacy_diff_within(
            root,
            &report,
            &version,
            &version,
            false,
            changed.len() as u64 - 1
        ),
        Err(TemplateError::BudgetTooSmall { .. })
    ));
}

#[test]
fn marker_and_legacy_renderers_every_content_prefix_and_invalid_utf8() {
    let template = Template::parse(V1);
    let sample = b"\0\xff\xc0\x80a\n\xe2\x82\xac{{a}}{{{comp.files}}}";
    let plan = plan();
    for end in 0..=sample.len() {
        let root = MarkerNode::file("é<&\n", &sample[..end], u64::MAX);
        legacy_equivalence(&template, &root, false);
        legacy_equivalence(&template, &root, true);
        assert_eq!(
            render_markers(&template, &root, false),
            render_markers(&template, &root, false)
        );
        let records = [record("src/x", &sample[..end])];
        assert_eq!(
            render_plan(&plan, "pack", &records, "", &host()).unwrap(),
            render_plan(&plan, "pack", &records, "", &host()).unwrap()
        );
    }
    legacy_equivalence(&template, &MarkerNode::dir("", 0), false);
    // Empty structured input and empty string fields are legal for the legacy APIs.
    let empty = Template::parse("");
    legacy_equivalence(&empty, &MarkerNode::dir("", 0), false);
}

#[test]
fn marker_flags_withheld_bytes_duplicates_and_caller_order() {
    let template = Template::parse(V1);
    let mut root = MarkerNode::dir("", 0);
    let visible = MarkerNode::file("visible", b"VISIBLE", 1);
    let mut omitted = MarkerNode::file("omitted", b"SECRET", 1);
    omitted.omit_content = true;
    let mut ignored = MarkerNode::file("ignored", b"SECRET", 1);
    ignored.is_ignored = true;
    let mut binary = MarkerNode::file("binary", b"SECRET", 1);
    binary.binary_label = Some("opaque".into());
    root.children = vec![visible.clone(), omitted, ignored, binary, visible];
    let text = render_markers(&template, &root, false);
    assert!(!text.contains("SECRET"));
    assert_eq!(text.matches("VISIBLE").count(), 2);
    assert!(!render_markers(&template, &root, true).contains("VISIBLE"));
    root.children.reverse();
    let reversed = render_markers(&template, &root, false);
    assert_eq!(reversed, render_markers(&template, &root, false));
    assert!(!reversed.contains("SECRET"));
    legacy_equivalence(&template, &root, false);
    for layout in [
        LegacyLayout::Snapshot,
        LegacyLayout::TreeOnly,
        LegacyLayout::Diff,
    ] {
        let options = LegacyOutput::new(template.clone(), layout);
        assert_eq!(options.layout, layout);
        assert!(options.omit_svg_content);
        assert!(!options.debug_ignored);
    }
}

#[test]
fn legacy_renderers_large_unicode_and_boundary_budgets() {
    let template = Template::parse(V1);
    for len in [0, 1, STRING_MAX, STRING_MAX + 1] {
        let bytes = "é".repeat(len);
        let root = MarkerNode::file("x", bytes.as_bytes(), 0);
        legacy_equivalence(&template, &root, false);
    }
    for text in ["", "x", "on", "off", &"x".repeat(STRING_MAX)] {
        let result = TreeStamps::parse(text);
        assert_eq!(result, TreeStamps::parse(text));
        assert_eq!(result.is_ok(), matches!(text, "on" | "off"));
    }
    assert_eq!(TreeStamps::default(), TreeStamps::Off);
    for stamps in [TreeStamps::On, TreeStamps::Off] {
        assert_eq!(stamps.template(), stamps.template());
    }
}

struct FailAt {
    remaining: usize,
    bytes: Vec<u8>,
}
impl Write for FailAt {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        // Deliberate one-byte writes also exercise UTF-8 transport boundaries.
        let count = bytes.len().min(self.remaining).min(1);
        self.bytes.extend_from_slice(&bytes[..count]);
        self.remaining -= count;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn writers_propagate_failure_at_every_output_offset_and_allow_short_writes() {
    let template = Template::parse(V1);
    let compiled = template.compile("é").unwrap();
    let root = MarkerNode::file("é", b"\xff", 0);
    let version = Version::new_v1();
    let diff = diff("é");
    let snapshot = render_legacy_snapshot(&template, &root, &version, false, false);
    let changed = render_legacy_diff(&template, &root, &diff, &version, &version, false);
    let encoded = serialized(&compiled);
    for (which, expected) in [snapshot.as_bytes(), changed.as_bytes(), encoded.as_slice()]
        .iter()
        .enumerate()
    {
        for end in 0..=expected.len() {
            let mut writer = FailAt {
                remaining: end,
                bytes: vec![],
            };
            let result = match which {
                0 => write_legacy_snapshot(&mut writer, &template, &root, &version, false, false),
                1 => write_legacy_diff(
                    &mut writer,
                    &template,
                    &root,
                    &diff,
                    &version,
                    &version,
                    false,
                ),
                _ => compiled.serialize(&mut writer),
            };
            assert_eq!(writer.bytes, expected[..end]);
            if end == expected.len() {
                result.unwrap();
            } else {
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
            }
        }
    }
}

#[test]
fn external_body_loader_failure_empty_withheld_and_literal_bytes() {
    let template = Template::parse(V1);
    let root = MarkerNode::file("x", b"CAPTURE", 0);
    let version = Version::new_v1();
    for body in [
        None,
        Some(vec![]),
        Some(vec![0xff]),
        Some(b"{{a}}{{{comp.files}}}".to_vec()),
    ] {
        let mut output = Vec::new();
        write_legacy_snapshot_with(
            &mut output,
            &template,
            &root,
            &version,
            false,
            false,
            &|_| Ok(body.clone()),
        )
        .unwrap();
        let mut twin = root.clone();
        twin.content.clone_from(&body);
        assert_eq!(
            output,
            render_legacy_snapshot(&template, &twin, &version, false, false).as_bytes()
        );
    }
    let mut output = Vec::new();
    assert_eq!(
        write_legacy_snapshot_with(
            &mut output,
            &template,
            &root,
            &version,
            false,
            false,
            &|_| Err(io::ErrorKind::PermissionDenied.into())
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::PermissionDenied
    );
    write_legacy_snapshot_with(
        &mut Vec::new(),
        &template,
        &root,
        &version,
        true,
        false,
        &|_| panic!("tree-only output must not load content"),
    )
    .unwrap();
}

#[test]
fn resource_limits_policy_helpers_and_concurrent_api_replay() {
    let limits = ResourceLimits::standard();
    assert!(limits.within(&limits).is_ok());
    let mut raised = limits.clone();
    raised.frame_bytes += 1;
    assert!(matches!(
        raised.within(&limits),
        Err(TemplateError::BudgetAboveCeiling {
            field: "frame_bytes",
            ..
        })
    ));
    for (policy, rank, token) in [
        (NetworkPolicy::None, 0, "none"),
        (NetworkPolicy::Loopback, 1, "loopback"),
        (NetworkPolicy::External, 2, "external"),
    ] {
        assert_eq!((policy.rank(), policy.as_str()), (rank, token));
    }
    assert_eq!(Liveness::fail_closed(), Liveness::fail_closed());
    assert_eq!(bundled_git_plugin(), bundled_git_plugin());
    let plan = plan();
    let bytes = to_bytes(&plan).unwrap();
    let host = host();
    let template = Template::parse(V1);
    let root = MarkerNode::file("é", b"\xff", 1);
    let barrier = Barrier::new(4);
    let expected = render_plan(&plan, "pack", &[record("x", b"\xff")], "", &host).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                barrier.wait();
                assert_eq!(compile(V1, &host).unwrap(), plan);
                assert_eq!(to_bytes(&plan).unwrap(), bytes);
                assert_eq!(load(&bytes, &host).unwrap(), plan);
                assert_eq!(
                    render_plan(&plan, "pack", &[record("x", b"\xff")], "", &host).unwrap(),
                    expected
                );
                assert_eq!(Template::parse(V1), template);
                let compiled = template.compile("x").unwrap();
                assert_eq!(
                    CompiledTemplate::deserialize(&mut serialized(&compiled).as_slice()).unwrap(),
                    compiled
                );
                legacy_equivalence(&template, &root, false);
            });
        }
    });
}

#[test]
fn every_template_refusal_names_a_corrective_step() {
    let errors = vec![
        TemplateError::MissingSection {
            name: "LAYOUT".into(),
        },
        TemplateError::EmptySection {
            name: "LAYOUT".into(),
        },
        TemplateError::UnresolvedVariable { name: "x".into() },
        TemplateError::InvalidSyntax {
            line: 1,
            detail: "key".into(),
        },
        TemplateError::Cycle {
            kind: "variable",
            name: "x".into(),
        },
        TemplateError::UnknownCapability {
            name: "parser:x".into(),
        },
        TemplateError::BudgetAboveCeiling {
            field: "bytes",
            requested: 2,
            ceiling: 1,
        },
        TemplateError::GrantExpansion { kind: "plugin" },
        TemplateError::UnsupportedVersion { found: 3 },
        TemplateError::UnknownOpcode { opcode: 99 },
        TemplateError::UnknownSection { kind: 99 },
        TemplateError::Corrupt { reason: "length" },
        TemplateError::DigestMismatch,
        TemplateError::BudgetTooSmall { minimum_bytes: 1 },
        TemplateError::Limit {
            what: "string",
            limit: 1,
        },
        TemplateError::Duplicate {
            kind: "field",
            name: "x".into(),
        },
        TemplateError::UnresolvedRef {
            kind: "view",
            name: "x".into(),
        },
        TemplateError::RequiredPluginMissing { id: "ex/x".into() },
        TemplateError::UserDisabled { id: "ex/x".into() },
        TemplateError::UnknownField {
            plugin: "ex/x".into(),
            field: "x".into(),
        },
        TemplateError::ReservedAlias {
            alias: "help".into(),
        },
    ];
    for error in errors {
        let message = error.to_string();
        let (_, step) = message.split_once("; ").expect("corrective step");
        assert!(step.split_whitespace().count() >= 4, "{message}");
        assert_eq!(message, error.to_string());
    }
    for error in [
        TemplateValidationError::MissingSection("LAYOUT".into()),
        TemplateValidationError::EmptySection("LAYOUT".into()),
        TemplateValidationError::UnresolvedVariable("x".into()),
        TemplateValidationError::InvalidSyntax("x".into()),
        TemplateValidationError::Cycle("x".into()),
    ] {
        assert!(error.to_string().contains("; "));
    }
}

#[test]
fn render_rejects_mutated_instruction_arguments_even_without_records() {
    for inst in [
        Inst { opcode: 0, arg: 0 },
        Inst {
            opcode: u16::MAX,
            arg: 0,
        },
        Inst {
            opcode: 1,
            arg: u16::MAX,
        },
        Inst { opcode: 2, arg: 0 },
        Inst { opcode: 2, arg: 14 },
        Inst {
            opcode: 2,
            arg: u16::MAX,
        },
        Inst {
            opcode: 3,
            arg: u16::MAX,
        },
    ] {
        let mut plan = plan();
        plan.render.layouts[0].instructions = vec![inst];
        assert!(to_bytes(&plan).is_err());
        assert!(render_plan(&plan, "pack", &[], "", &host()).is_err());
        assert!(render_plan(&plan, "pack", &[record("x", b"x")], "", &host()).is_err());
    }
}

#[test]
fn every_resource_limit_checks_zero_one_maximum_and_over_ceiling() {
    macro_rules! field {
        ($field:ident) => {{
            for amount in [0, 1, u64::MAX] {
                let mut ceiling = ResourceLimits::standard();
                ceiling.$field = amount;
                assert!(ceiling.within(&ceiling).is_ok());
                if amount < u64::MAX {
                    let mut request = ceiling.clone();
                    request.$field += 1;
                    assert!(matches!(
                        request.within(&ceiling),
                        Err(TemplateError::BudgetAboveCeiling {
                            field: stringify!($field),
                            ..
                        })
                    ));
                }
            }
        }};
    }
    field!(frame_bytes);
    field!(read_bytes);
    field!(actions);
    field!(outstanding_requests);
    field!(diagnostics_bytes);
    field!(chunk_bytes);
    field!(state_bytes);
    field!(upload_bytes);
    field!(queue_bytes);
    field!(memory_bytes);
    field!(work_units);
}

#[test]
fn v2_duplicate_partial_and_render_names_are_refused() {
    for (body, kind) in [
        ("[PARTIAL:p]\nfirst\n[PARTIAL:p]\nsecond\n", "partial"),
        ("[RENDER:main]\nfirst\n[LAYOUT]\nsecond\n", "render"),
    ] {
        assert!(
            matches!(compile(&format!("{V2}{body}"), &host()), Err(TemplateError::Duplicate { kind: found, .. }) if found == kind)
        );
    }
    assert!(
        compile(
            &format!("{V2}[PARTIAL:p]\nfirst\n[PARTIAL:q]\nsecond\n"),
            &host()
        )
        .is_ok()
    );
}

#[test]
fn deep_partial_chains_use_heap_state_and_cycles_still_refuse() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut source = String::from(V2);
            for index in 0..4096 {
                source.push_str(&format!(
                    "[PARTIAL:p{index:04}]\n{{{{>p{:04}}}}}\n",
                    index + 1
                ));
            }
            source.push_str("[PARTIAL:p4096]\nend\n");
            let plan = compile(&source, &host()).unwrap();
            assert_eq!(plan.render.partials.len(), 4097);
            assert!(load(&to_bytes(&plan).unwrap(), &host()).is_ok());
            let cyclic = source.replace("[PARTIAL:p4096]\nend", "[PARTIAL:p4096]\n{{>p0000}}");
            assert!(matches!(
                compile(&cyclic, &host()),
                Err(TemplateError::Cycle {
                    kind: "partial",
                    ..
                })
            ));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn compiler_refuses_exponential_variable_expansion_before_materialization() {
    let mut variables = String::from("[VARIABLES]\n");
    for index in 0..40 {
        variables.push_str(&format!(
            "v{index:02} = \"{{{{v{:02}}}}}{{{{v{:02}}}}}\"\n",
            index + 1,
            index + 1
        ));
    }
    variables.push_str("v40 = \"x\"\n[LAYOUT]\n{{v00}}\n");
    for prefix in ["", V2] {
        let error = compile(&format!("{prefix}{variables}"), &host()).unwrap_err();
        assert_eq!(
            error,
            TemplateError::Limit {
                what: "variable expansion",
                limit: STRING_MAX as u64
            }
        );
    }
    for len in [0, 1, STRING_MAX] {
        let source = format!(
            "[VARIABLES]\na = \"{}\"\n[LAYOUT]\n{{{{a}}}}x\n",
            "x".repeat(len)
        );
        // Keep the final literal within the per-string limit in this positive control.
        let source = if len == STRING_MAX {
            source.replace("{{a}}x", "{{a}}")
        } else {
            source
        };
        assert!(compile(&source, &host()).is_ok());
    }
}

#[test]
fn compiler_bounds_aggregate_variable_storage_and_repeated_body_expansion() {
    let mut source = format!("[VARIABLES]\na = \"{}\"\n", "x".repeat(STRING_MAX / 2));
    for index in 0..64 {
        source.push_str(&format!("v{index:02} = \"{{{{a}}}}\"\n"));
    }
    source.push_str("[LAYOUT]\nx\n");
    for prefix in ["", V2] {
        assert_eq!(
            compile(&format!("{prefix}{source}"), &host()),
            Err(TemplateError::Limit {
                what: "variable expansion",
                limit: PLAN_MAX as u64
            })
        );
    }
    let variables = format!("[VARIABLES]\na = \"{}\"\n", "x".repeat(1024));
    let repeated = "{{a}}".repeat(1025);
    for prefix in ["", V2] {
        let source = format!("{prefix}{variables}[LAYOUT]\n{repeated}\n");
        assert_eq!(
            compile(&source, &host()),
            Err(TemplateError::Limit {
                what: "variable expansion",
                limit: PLAN_MAX as u64
            })
        );
    }
    let mut partials = format!("{V2}{variables}");
    for index in 0..1025 {
        partials.push_str(&format!("[PARTIAL:p{index:04}]\n{{{{a}}}}\n"));
    }
    assert_eq!(
        compile(&partials, &host()),
        Err(TemplateError::Limit {
            what: "variable expansion",
            limit: PLAN_MAX as u64
        })
    );
}

#[test]
fn bounded_compilation_keeps_nested_and_runtime_variable_semantics() {
    for body in [
        "{{a}}",
        "{{{a}}}",
        "{{{a",
        "{{{project}}} {{a}}",
        "{{a}}é{{a}}",
    ] {
        let source = format!("[VARIABLES]\na = \"{{{{b}}}}\"\nb = \"é\"\n[LAYOUT]\n{body}\n");
        let legacy = Template::parse(&source);
        let compiled = legacy.compile("x").unwrap();
        let plan = compile(&source, &host());
        // Unknown triple-braced fields are refused by bytecode compilation, while legacy
        // marker rendering deliberately preserves them. Valid cases must preserve substitution.
        if body == "{{{a}}}" {
            assert!(plan.is_err());
        } else {
            let plan = plan.unwrap();
            let text = render_plan(&plan, "pack", &[], "{{a}}", &host()).unwrap();
            let mut template = compiled.to_template();
            template.insert_variable("project", "{{a}}");
            assert_eq!(
                text,
                render_markers(&template, &MarkerNode::dir("", 0), false)
            );
        }
    }
}

#[test]
fn legacy_parse_and_compile_refuse_amplification_through_validation() {
    let mut source = String::from("[VARIABLES]\n");
    for index in 0..40 {
        source.push_str(&format!(
            "v{index:02} = \"{{{{v{:02}}}}}{{{{v{:02}}}}}\"\n",
            index + 1,
            index + 1
        ));
    }
    source.push_str("v40 = \"x\"\n[LAYOUT]\n{{v00}}\n");
    let template = Template::parse(&source);
    assert_eq!(template, Template::parse(&source));
    assert!(template.validate().iter().any(|error| matches!(error, TemplateValidationError::InvalidSyntax(detail) if detail.contains("variable expansion"))));
    assert!(template.compile("x").is_err());
    let source = format!(
        "[VARIABLES]\na = \"{}\"\n[LAYOUT]\n{}\n",
        "x".repeat(1024),
        "{{a}}".repeat(1025)
    );
    let template = Template::parse(&source);
    assert!(template.compile("x").is_err());
}
