// SPDX-License-Identifier: Apache-2.0
//! `ContextPlan` section bodies, domain digests and provenance.
//! Implements contracts/DTS-v2.md compiled IR and IDENTITY-v1.
//! Depends on dctx-hash, section codecs and byte readers; used by the `ContextPlan` binary codec.

use dctx_hash::Sha256;

use crate::error::{TemplateError, MAX_PLAN_BYTES};
use crate::model::ContextPlan;

use super::io::Reader;
use super::sections::{encode_selections, encode_indexes, encode_budgets, encode_views, encode_render};
use super::{IR_MAJOR, IR_MINOR, SCHEMA};

pub(super) struct Body {
    pub(super) select: Vec<u8>,
    pub(super) index: Vec<u8>,
    pub(super) budget: Vec<u8>,
    pub(super) view: Vec<u8>,
    pub(super) render: Vec<u8>,
    pub(super) plugin: Vec<u8>,
}

pub(super) struct Digests {
    pub(super) selection: [u8; 32],
    pub(super) extraction: [u8; 32],
    pub(super) render: [u8; 32],
    pub(super) plugin: [u8; 32],
    pub(super) plan: [u8; 32],
}

pub(super) fn body_of(plan: &ContextPlan) -> Result<Body, TemplateError> {
    Ok(Body {
        select: encode_selections(&plan.selections)?,
        index: encode_indexes(&plan.indexes)?,
        budget: encode_budgets(&plan.budgets)?,
        view: encode_views(&plan.views)?,
        render: encode_render(&plan.render)?,
        plugin: crate::plugin_codec::encode(&plan.plugins)?,
    })
}

pub(super) fn digests_of(plan: &ContextPlan) -> Result<Digests, TemplateError> {
    hash_body(plan.source_version, &body_of(plan)?)
}

pub(super) fn hash_body(source_version: u16, body: &Body) -> Result<Digests, TemplateError> {
    // Domain kinds differ from section kinds: selection is kind 4, extraction kind 5,
    // render kind 8 schema 2, plugin kind 8 schema 3, plan kind 8 schema 1.
    let selection = domain_digest(4, SCHEMA, &body.select)?;
    let extraction = domain_digest(5, SCHEMA, &body.index)?;
    let render = domain_digest(8, 2, &body.render)?;
    let plugin = domain_digest(8, 3, &body.plugin)?;
    let parts = [
        (1u16, body.select.as_slice()),
        (2, body.index.as_slice()),
        (3, body.budget.as_slice()),
        (4, body.view.as_slice()),
        (5, body.render.as_slice()),
        (6, body.plugin.as_slice()),
    ];
    let plan = plan_digest(source_version, &parts)?;
    Ok(Digests { selection, extraction, render, plugin, plan })
}

pub(super) fn plan_digest(source_version: u16, parts: &[(u16, &[u8])]) -> Result<[u8; 32], TemplateError> {
    let mut pre = Vec::new();
    pre.extend_from_slice(&source_version.to_le_bytes());
    pre.extend_from_slice(&IR_MAJOR.to_le_bytes());
    pre.extend_from_slice(&IR_MINOR.to_le_bytes());
    for (kind, payload) in parts {
        let len = u32::try_from(payload.len()).map_err(|_| TemplateError::Limit {
            what: "section",
            limit: u64::from(MAX_PLAN_BYTES),
        })?;
        pre.extend_from_slice(&kind.to_le_bytes());
        pre.extend_from_slice(&SCHEMA.to_le_bytes());
        pre.extend_from_slice(&len.to_le_bytes());
        pre.extend_from_slice(payload);
    }
    domain_digest(8, SCHEMA, &pre)
}

pub(super) fn domain_digest(kind: u16, schema: u16, payload: &[u8]) -> Result<[u8; 32], TemplateError> {
    let mut message = Vec::with_capacity(17 + payload.len());
    message.extend_from_slice(b"DCTX\0");
    message.extend_from_slice(&kind.to_le_bytes());
    message.extend_from_slice(&schema.to_le_bytes());
    let len = u64::try_from(payload.len()).map_err(|_| TemplateError::Limit { what: "hash", limit: u64::MAX })?;
    message.extend_from_slice(&len.to_le_bytes());
    message.extend_from_slice(payload);
    let digest = Sha256::digest(&message).map_err(|_| TemplateError::Limit { what: "hash", limit: u64::MAX })?;
    Ok(*digest.as_bytes())
}

pub(super) fn encode_provenance(source_version: u16, digests: &Digests) -> Vec<u8> {
    let mut out = Vec::with_capacity(164);
    out.extend_from_slice(&source_version.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    for digest in [digests.selection, digests.extraction, digests.render, digests.plugin, digests.plan] {
        out.extend_from_slice(&digest);
    }
    out
}

pub(super) fn decode_provenance(payload: &[u8]) -> Result<(u16, Digests), TemplateError> {
    if payload.len() != 164 {
        return Err(TemplateError::Corrupt { reason: "provenance" });
    }
    let mut reader = Reader::new(payload);
    let source_version = reader.u16()?;
    if reader.u16()? != 0 {
        return Err(TemplateError::Corrupt { reason: "provenance" });
    }
    let mut raw = [[0u8; 32]; 5];
    for slot in &mut raw {
        slot.copy_from_slice(reader.take(32)?);
    }
    reader.finish("provenance")?;
    Ok((
        source_version,
        Digests { selection: raw[0], extraction: raw[1], render: raw[2], plugin: raw[3], plan: raw[4] },
    ))
}

