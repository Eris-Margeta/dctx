//! Versioned ContextPlan binary: magic `DCTXIR`, seven sections, trailing SHA-256.
//! Implements contracts/DTS-v2.md compiled IR and IDENTITY-v1 domain separation.
//! Depends on [`dctx_hash`] and [`crate::model`]. [`crate::lower`] seals plans through [`seal`].
//! Load checks magic and version before the digest, then refuses unknown sections and opcodes.

use dctx_hash::Sha256;

use crate::error::{TemplateError, MAX_PLAN_BYTES, MAX_SECTIONS};
use crate::model::{ContextPlan, KNOWN_CAPABILITY_BITS};
use crate::opcode::validate_program;

mod digest;
mod io;
mod sections;

use digest::{
    body_of, decode_provenance, digests_of, domain_digest, encode_provenance, hash_body,
    plan_digest,
};
use io::{read_section, Reader};
use sections::{decode_budgets, decode_indexes, decode_render, decode_selections, decode_views};

const MAGIC: &[u8] = b"DCTXIR\0\0";
const IR_MAJOR: u16 = 1;
const IR_MINOR: u16 = 0;
const SCHEMA: u16 = 1;
const SECTION_COUNT: u32 = 7;

/// Fills the five domain digests from the section payloads.
pub(crate) fn seal(plan: &mut ContextPlan) -> Result<(), TemplateError> {
    let digests = digests_of(plan)?;
    plan.selection_digest = digests.selection;
    plan.extraction_digest = digests.extraction;
    plan.render_digest = digests.render;
    plan.plugin_digest = digests.plugin;
    plan.plan_digest = digests.plan;
    Ok(())
}

/// Encodes `plan`. Digests are recomputed. `check_ops` refuses opcodes outside this IR.
pub(crate) fn encode(plan: &ContextPlan, check_ops: bool) -> Result<Vec<u8>, TemplateError> {
    if check_ops {
        validate_program(&plan.render)?;
    }
    let body = body_of(plan)?;
    let digests = hash_body(plan.source_version, &body)?;
    let provenance = encode_provenance(plan.source_version, &digests);
    let slices: [&[u8]; 7] = [
        &body.select,
        &body.index,
        &body.budget,
        &body.view,
        &body.render,
        &body.plugin,
        &provenance,
    ];
    assemble(plan.capabilities, &slices)
}

/// Encodes `plan`, refusing unknown opcodes.
pub fn to_bytes(plan: &ContextPlan) -> Result<Vec<u8>, TemplateError> {
    encode(plan, true)
}

/// Loads a plan. Version is checked before the trailing digest.
pub fn from_bytes(bytes: &[u8]) -> Result<ContextPlan, TemplateError> {
    if bytes.len() > usize::try_from(MAX_PLAN_BYTES).unwrap_or(usize::MAX) {
        return Err(TemplateError::Limit { what: "plan", limit: u64::from(MAX_PLAN_BYTES) });
    }
    if bytes.len() < 64 {
        return Err(TemplateError::Corrupt { reason: "truncated" });
    }
    let hash_at = bytes.len() - 32;
    let (body, hash) = bytes.split_at(hash_at);
    let mut reader = Reader::new(body);
    let magic = reader.take(8)?;
    if magic != MAGIC {
        return Err(TemplateError::Corrupt { reason: "magic" });
    }
    let major = reader.u16()?;
    let minor = reader.u16()?;
    if major != IR_MAJOR || minor != IR_MINOR {
        return Err(TemplateError::UnsupportedVersion { found: u32::from(major) });
    }
    let total = reader.u64()?;
    let file_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if total != file_len {
        return Err(TemplateError::Corrupt { reason: "length" });
    }
    let capabilities = reader.u64()?;
    let section_count = reader.u32()?;
    let digest = Sha256::digest(body).map_err(|_| TemplateError::Limit { what: "hash", limit: u64::MAX })?;
    let mut actual = [0u8; 32];
    actual.copy_from_slice(hash);
    if actual != *digest.as_bytes() {
        return Err(TemplateError::DigestMismatch);
    }
    if capabilities & !KNOWN_CAPABILITY_BITS != 0 {
        return Err(TemplateError::UnknownCapability { name: format!("bits:{capabilities:x}") });
    }
    if section_count > MAX_SECTIONS {
        return Err(TemplateError::Limit { what: "sections", limit: u64::from(MAX_SECTIONS) });
    }
    if section_count != SECTION_COUNT {
        return Err(TemplateError::Corrupt { reason: "section count" });
    }
    let mut payloads = Vec::with_capacity(7);
    for expected in 1u16..=7 {
        let (kind, schema, payload) = read_section(&mut reader)?;
        if kind != expected {
            if !(1..=7).contains(&kind) {
                return Err(TemplateError::UnknownSection { kind });
            }
            return Err(TemplateError::Corrupt { reason: "section order" });
        }
        if schema != SCHEMA {
            return Err(TemplateError::UnsupportedVersion { found: u32::from(schema) });
        }
        payloads.push(payload.to_vec());
    }
    reader.finish("trailing")?;
    let claimed = decode_provenance(&payloads[6])?;
    let selection = domain_digest(4, SCHEMA, &payloads[0])?;
    let extraction = domain_digest(5, SCHEMA, &payloads[1])?;
    let render = domain_digest(8, 2, &payloads[4])?;
    let plugin = domain_digest(8, 3, &payloads[5])?;
    let parts = [
        (1u16, payloads[0].as_slice()),
        (2, payloads[1].as_slice()),
        (3, payloads[2].as_slice()),
        (4, payloads[3].as_slice()),
        (5, payloads[4].as_slice()),
        (6, payloads[5].as_slice()),
    ];
    let plan_digest = plan_digest(claimed.0, &parts)?;
    let digests = claimed.1;
    if selection != digests.selection
        || extraction != digests.extraction
        || render != digests.render
        || plugin != digests.plugin
        || plan_digest != digests.plan
    {
        return Err(TemplateError::DigestMismatch);
    }
    Ok(ContextPlan {
        source_version: claimed.0,
        capabilities,
        selections: decode_selections(&payloads[0])?,
        indexes: decode_indexes(&payloads[1])?,
        budgets: decode_budgets(&payloads[2])?,
        views: decode_views(&payloads[3])?,
        render: decode_render(&payloads[4])?,
        plugins: crate::plugin_codec::decode(&payloads[5])?,
        selection_digest: selection,
        extraction_digest: extraction,
        render_digest: render,
        plugin_digest: plugin,
        plan_digest,
    })
}

fn assemble(capabilities: u64, payloads: &[&[u8]; 7]) -> Result<Vec<u8>, TemplateError> {
    let mut sections = Vec::new();
    for (index, payload) in payloads.iter().enumerate() {
        let kind = u16::try_from(index + 1).map_err(|_| TemplateError::Corrupt { reason: "section" })?;
        let len = u32::try_from(payload.len()).map_err(|_| TemplateError::Limit {
            what: "section",
            limit: u64::from(MAX_PLAN_BYTES),
        })?;
        sections.extend_from_slice(&kind.to_le_bytes());
        sections.extend_from_slice(&SCHEMA.to_le_bytes());
        sections.extend_from_slice(&len.to_le_bytes());
        sections.extend_from_slice(payload);
    }
    let total = 32usize.saturating_add(sections.len()).saturating_add(32);
    if total > usize::try_from(MAX_PLAN_BYTES).unwrap_or(usize::MAX) {
        return Err(TemplateError::Limit { what: "plan", limit: u64::from(MAX_PLAN_BYTES) });
    }
    let total_u64 = u64::try_from(total).unwrap_or(u64::MAX);
    let mut file = Vec::with_capacity(total);
    file.extend_from_slice(MAGIC);
    file.extend_from_slice(&IR_MAJOR.to_le_bytes());
    file.extend_from_slice(&IR_MINOR.to_le_bytes());
    file.extend_from_slice(&total_u64.to_le_bytes());
    file.extend_from_slice(&capabilities.to_le_bytes());
    file.extend_from_slice(&SECTION_COUNT.to_le_bytes());
    file.extend_from_slice(&sections);
    let digest = Sha256::digest(&file).map_err(|_| TemplateError::Limit { what: "hash", limit: u64::MAX })?;
    file.extend_from_slice(digest.as_bytes());
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lower::compile;
    use crate::model::HostCeiling;

    fn sample() -> ContextPlan {
        let source = "\
[VARIABLES]
k = \"v\"
[PARTIAL:TREE_ENTRY]
{{{node.path}}}
[PARTIAL:FILE_BLOCK]
{{{file.content}}}
[LAYOUT]
{{{comp.tree}}}
";
        compile(source, &HostCeiling::base()).expect("sample plan")
    }

    #[test]
    fn magic_major_and_roundtrip_keep_bytes() {
        let plan = sample();
        let bytes = to_bytes(&plan).expect("encode");
        assert_eq!(&bytes[..8], b"DCTXIR\0\0");
        assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 1);
        assert_eq!(u16::from_le_bytes([bytes[10], bytes[11]]), 0);
        let loaded = from_bytes(&bytes).expect("decode");
        assert_eq!(loaded, plan);
    }

    #[test]
    fn domain_kind_changes_the_digest() {
        let payload = b"same-payload";
        let selection = domain_digest(4, 1, payload).expect("kind 4");
        let extraction = domain_digest(5, 1, payload).expect("kind 5");
        assert_ne!(selection, extraction);
    }

    #[test]
    fn unknown_opcode_is_refused_and_a_clean_plan_loads() {
        let plan = sample();
        let bytes = to_bytes(&plan).expect("encode");
        assert!(from_bytes(&bytes).is_ok());
        let mut corrupt = plan.clone();
        corrupt.render.layouts[0].instructions[0].opcode = 99;
        let encoded = encode(&corrupt, false).expect("unchecked encode");
        assert!(matches!(from_bytes(&encoded), Err(TemplateError::UnknownOpcode { opcode: 99 })));
    }

    #[test]
    fn flipped_hash_and_future_major_are_refused() {
        let bytes = to_bytes(&sample()).expect("encode");
        let mut flipped = bytes.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 0xff;
        assert!(matches!(from_bytes(&flipped), Err(TemplateError::DigestMismatch)));
        let mut future = bytes;
        future[8] = 2;
        assert!(matches!(from_bytes(&future), Err(TemplateError::UnsupportedVersion { found: 2 })));
    }

    #[test]
    fn unknown_capability_bits_are_refused() {
        let mut plan = sample();
        plan.capabilities |= 1 << 20;
        let bytes = encode(&plan, true).expect("encode");
        assert!(matches!(from_bytes(&bytes), Err(TemplateError::UnknownCapability { .. })));
    }
}
