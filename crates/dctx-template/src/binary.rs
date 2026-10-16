//! Versioned ContextPlan binary: magic `DCTXIR`, seven sections, trailing SHA-256.
//! Implements contracts/DTS-v2.md compiled IR and IDENTITY-v1 domain separation.
//! Depends on [`dctx_hash`] and [`crate::model`]. [`crate::lower`] seals plans through [`seal`].
//! Load checks magic and version before the digest, then refuses unknown sections and opcodes.

use dctx_core::FileHandlingDecision;
use dctx_hash::Sha256;

use crate::error::{TemplateError, MAX_INSTRUCTIONS, MAX_PLAN_BYTES, MAX_SECTIONS, MAX_STRING_BYTES};
use crate::model::{
    BudgetPlan, ConfigEntry, ConfigValue, ContextPlan, IndexPlan, Inst, NamedCode, NetworkPolicy, PluginBinding,
    PluginGroup, PluginPlan, PluginState, RenderProgram, SelectRule, Selection, ViewPlan, KNOWN_CAPABILITY_BITS,
    OP_MAX,
};
use crate::opcode::{instruction_ok, validate_program};

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
        plugins: decode_plugins(&payloads[5])?,
        selection_digest: selection,
        extraction_digest: extraction,
        render_digest: render,
        plugin_digest: plugin,
        plan_digest,
    })
}

struct Body {
    select: Vec<u8>,
    index: Vec<u8>,
    budget: Vec<u8>,
    view: Vec<u8>,
    render: Vec<u8>,
    plugin: Vec<u8>,
}

struct Digests {
    selection: [u8; 32],
    extraction: [u8; 32],
    render: [u8; 32],
    plugin: [u8; 32],
    plan: [u8; 32],
}

fn body_of(plan: &ContextPlan) -> Result<Body, TemplateError> {
    Ok(Body {
        select: encode_selections(&plan.selections)?,
        index: encode_indexes(&plan.indexes)?,
        budget: encode_budgets(&plan.budgets)?,
        view: encode_views(&plan.views)?,
        render: encode_render(&plan.render)?,
        plugin: encode_plugins(&plan.plugins)?,
    })
}

fn digests_of(plan: &ContextPlan) -> Result<Digests, TemplateError> {
    hash_body(plan.source_version, &body_of(plan)?)
}

fn hash_body(source_version: u16, body: &Body) -> Result<Digests, TemplateError> {
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

fn plan_digest(source_version: u16, parts: &[(u16, &[u8])]) -> Result<[u8; 32], TemplateError> {
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

fn domain_digest(kind: u16, schema: u16, payload: &[u8]) -> Result<[u8; 32], TemplateError> {
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

fn encode_provenance(source_version: u16, digests: &Digests) -> Vec<u8> {
    let mut out = Vec::with_capacity(164);
    out.extend_from_slice(&source_version.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    for digest in [digests.selection, digests.extraction, digests.render, digests.plugin, digests.plan] {
        out.extend_from_slice(&digest);
    }
    out
}

fn decode_provenance(payload: &[u8]) -> Result<(u16, Digests), TemplateError> {
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

fn encode_selections(items: &[Selection]) -> Result<Vec<u8>, TemplateError> {
    let mut buf = Buf::new();
    buf.u32(count(items.len())?);
    for item in items {
        buf.str(&item.name)?;
        buf.strs(&item.roots)?;
        buf.strs(&item.languages)?;
        buf.strs(&item.kinds)?;
        buf.u32(count(item.rules.len())?);
        for rule in &item.rules {
            buf.u8(decision_code(rule.decision));
            buf.str(&rule.pattern)?;
        }
    }
    Ok(buf.0)
}

fn decode_selections(payload: &[u8]) -> Result<Vec<Selection>, TemplateError> {
    let mut reader = Reader::new(payload);
    let n = reader.count()?;
    let mut items = Vec::with_capacity(n);
    for _ in 0..n {
        let name = reader.string()?;
        let roots = reader.string_list()?;
        let languages = reader.string_list()?;
        let kinds = reader.string_list()?;
        let rules_n = reader.count()?;
        let mut rules = Vec::with_capacity(rules_n);
        for _ in 0..rules_n {
            rules.push(SelectRule { decision: decision_from(reader.u8()?)?, pattern: reader.string()? });
        }
        items.push(Selection { name, roots, languages, kinds, rules });
    }
    reader.finish("payload")?;
    Ok(items)
}

fn encode_indexes(items: &[IndexPlan]) -> Result<Vec<u8>, TemplateError> {
    let mut buf = Buf::new();
    buf.u32(count(items.len())?);
    for item in items {
        buf.str(&item.name)?;
        buf.strs(&item.facts)?;
        buf.str(&item.parser)?;
        buf.str(&item.language)?;
        buf.str(&item.dialect)?;
        buf.u64(item.max_bytes);
    }
    Ok(buf.0)
}

fn decode_indexes(payload: &[u8]) -> Result<Vec<IndexPlan>, TemplateError> {
    let mut reader = Reader::new(payload);
    let n = reader.count()?;
    let mut items = Vec::with_capacity(n);
    for _ in 0..n {
        items.push(IndexPlan {
            name: reader.string()?,
            facts: reader.string_list()?,
            parser: reader.string()?,
            language: reader.string()?,
            dialect: reader.string()?,
            max_bytes: reader.u64()?,
        });
    }
    reader.finish("payload")?;
    Ok(items)
}

fn encode_budgets(items: &[BudgetPlan]) -> Result<Vec<u8>, TemplateError> {
    let mut buf = Buf::new();
    buf.u32(count(items.len())?);
    for item in items {
        buf.str(&item.name)?;
        buf.u64(item.bytes);
        buf.u64(item.estimated_tokens);
        buf.u64(item.hard_estimated_tokens);
        buf.u64(item.records);
        buf.u32(item.depth);
        buf.u64(item.parse_time_ms);
        buf.u64(item.parse_memory);
        buf.u64(item.work);
    }
    Ok(buf.0)
}

fn decode_budgets(payload: &[u8]) -> Result<Vec<BudgetPlan>, TemplateError> {
    let mut reader = Reader::new(payload);
    let n = reader.count()?;
    let mut items = Vec::with_capacity(n);
    for _ in 0..n {
        items.push(BudgetPlan {
            name: reader.string()?,
            bytes: reader.u64()?,
            estimated_tokens: reader.u64()?,
            hard_estimated_tokens: reader.u64()?,
            records: reader.u64()?,
            depth: reader.u32()?,
            parse_time_ms: reader.u64()?,
            parse_memory: reader.u64()?,
            work: reader.u64()?,
        });
    }
    reader.finish("payload")?;
    Ok(items)
}

fn encode_views(items: &[ViewPlan]) -> Result<Vec<u8>, TemplateError> {
    let mut buf = Buf::new();
    buf.u32(count(items.len())?);
    for item in items {
        buf.str(&item.name)?;
        buf.str(&item.mode)?;
        buf.str(&item.selection)?;
        buf.str(&item.index)?;
        buf.str(&item.budget)?;
        buf.str(&item.render)?;
        buf.str(&item.layout_label)?;
    }
    Ok(buf.0)
}

fn decode_views(payload: &[u8]) -> Result<Vec<ViewPlan>, TemplateError> {
    let mut reader = Reader::new(payload);
    let n = reader.count()?;
    let mut items = Vec::with_capacity(n);
    for _ in 0..n {
        items.push(ViewPlan {
            name: reader.string()?,
            mode: reader.string()?,
            selection: reader.string()?,
            index: reader.string()?,
            budget: reader.string()?,
            render: reader.string()?,
            layout_label: reader.string()?,
        });
    }
    reader.finish("payload")?;
    Ok(items)
}

fn encode_render(program: &RenderProgram) -> Result<Vec<u8>, TemplateError> {
    let mut buf = Buf::new();
    buf.strs(&program.strings)?;
    buf.u32(count(program.variables.len())?);
    for (key, value) in &program.variables {
        buf.str(key)?;
        buf.str(value)?;
    }
    write_codes(&mut buf, &program.partials)?;
    write_codes(&mut buf, &program.layouts)?;
    Ok(buf.0)
}

fn write_codes(buf: &mut Buf, items: &[NamedCode]) -> Result<(), TemplateError> {
    buf.u32(count(items.len())?);
    for item in items {
        buf.str(&item.name)?;
        buf.u32(count(item.instructions.len())?);
        for inst in &item.instructions {
            buf.u16(inst.opcode);
            buf.u16(inst.arg);
        }
    }
    Ok(())
}

fn decode_render(payload: &[u8]) -> Result<RenderProgram, TemplateError> {
    let mut reader = Reader::new(payload);
    let strings = reader.string_list()?;
    let var_count = reader.count()?;
    let mut variables = Vec::with_capacity(var_count);
    for _ in 0..var_count {
        variables.push((reader.string()?, reader.string()?));
    }
    let mut instructions = 0u64;
    let partials = read_codes(&mut reader, &mut instructions)?;
    let layouts = read_codes(&mut reader, &mut instructions)?;
    reader.finish("payload")?;
    let program = RenderProgram { strings, variables, partials, layouts };
    for code in program.partials.iter().chain(program.layouts.iter()) {
        for inst in &code.instructions {
            instruction_ok(inst.opcode, inst.arg, program.strings.len(), program.partials.len())?;
        }
    }
    Ok(program)
}

fn read_codes(reader: &mut Reader<'_>, instructions: &mut u64) -> Result<Vec<NamedCode>, TemplateError> {
    let n = reader.count()?;
    let mut items = Vec::with_capacity(n);
    for _ in 0..n {
        let name = reader.string()?;
        let count = reader.count()?;
        let mut body = Vec::with_capacity(count);
        for _ in 0..count {
            *instructions = instructions.saturating_add(1);
            if *instructions > u64::from(MAX_INSTRUCTIONS) {
                return Err(TemplateError::Limit { what: "instructions", limit: u64::from(MAX_INSTRUCTIONS) });
            }
            let opcode = reader.u16()?;
            let arg = reader.u16()?;
            if opcode == 0 || opcode > OP_MAX {
                return Err(TemplateError::UnknownOpcode { opcode });
            }
            body.push(Inst { opcode, arg });
        }
        items.push(NamedCode { name, instructions: body });
    }
    Ok(items)
}

fn encode_plugins(plan: &PluginPlan) -> Result<Vec<u8>, TemplateError> {
    let mut buf = Buf::new();
    buf.u32(count(plan.groups.len())?);
    for group in &plan.groups {
        buf.str(&group.name)?;
        buf.u8(network_code(group.network));
        buf.strs(&group.enabled)?;
        buf.strs(&group.order)?;
    }
    buf.u32(count(plan.plugins.len())?);
    for plugin in &plan.plugins {
        buf.str(&plugin.id)?;
        buf.str(&plugin.version)?;
        match plugin.digest {
            None => buf.u8(0),
            Some(digest) => {
                buf.u8(1);
                buf.raw(&digest);
            }
        }
        buf.u8(u8::from(plugin.required));
        buf.u8(state_code(plugin.state));
        buf.str(&plugin.alias)?;
        buf.strs(&plugin.hooks)?;
        buf.strs(&plugin.allow)?;
        buf.u32(count(plugin.config.len())?);
        for entry in &plugin.config {
            buf.str(&entry.key)?;
            write_config(&mut buf, &entry.value)?;
        }
        buf.strs(&plugin.before)?;
        buf.strs(&plugin.after)?;
    }
    buf.strs(&plan.order)?;
    Ok(buf.0)
}

fn decode_plugins(payload: &[u8]) -> Result<PluginPlan, TemplateError> {
    let mut reader = Reader::new(payload);
    let group_count = reader.count()?;
    let mut groups = Vec::with_capacity(group_count);
    for _ in 0..group_count {
        groups.push(PluginGroup {
            name: reader.string()?,
            network: network_from(reader.u8()?)?,
            enabled: reader.string_list()?,
            order: reader.string_list()?,
        });
    }
    let plugin_count = reader.count()?;
    let mut plugins = Vec::with_capacity(plugin_count);
    for _ in 0..plugin_count {
        let id = reader.string()?;
        let version = reader.string()?;
        let digest = match reader.u8()? {
            0 => None,
            1 => {
                let mut digest = [0u8; 32];
                digest.copy_from_slice(reader.take(32)?);
                Some(digest)
            }
            _ => return Err(TemplateError::Corrupt { reason: "digest" }),
        };
        let required = match reader.u8()? {
            0 => false,
            1 => true,
            _ => return Err(TemplateError::Corrupt { reason: "required" }),
        };
        let state = state_from(reader.u8()?)?;
        let alias = reader.string()?;
        let hooks = reader.string_list()?;
        let allow = reader.string_list()?;
        let config_count = reader.count()?;
        let mut config = Vec::with_capacity(config_count);
        for _ in 0..config_count {
            config.push(ConfigEntry { key: reader.string()?, value: read_config(&mut reader)? });
        }
        plugins.push(PluginBinding {
            id,
            version,
            digest,
            required,
            state,
            alias,
            hooks,
            allow,
            config,
            before: reader.string_list()?,
            after: reader.string_list()?,
        });
    }
    let order = reader.string_list()?;
    reader.finish("payload")?;
    Ok(PluginPlan { groups, plugins, order })
}

fn write_config(buf: &mut Buf, value: &ConfigValue) -> Result<(), TemplateError> {
    match value {
        ConfigValue::Bool(value) => {
            buf.u8(1);
            buf.u8(u8::from(*value));
        }
        ConfigValue::Int(value) => {
            buf.u8(2);
            buf.u64(*value);
        }
        ConfigValue::Str(value) => {
            buf.u8(3);
            buf.str(value)?;
        }
        ConfigValue::List(items) => {
            buf.u8(4);
            buf.strs(items)?;
        }
        ConfigValue::Enum(value) => {
            buf.u8(5);
            buf.str(value)?;
        }
    }
    Ok(())
}

fn read_config(reader: &mut Reader<'_>) -> Result<ConfigValue, TemplateError> {
    match reader.u8()? {
        1 => match reader.u8()? {
            0 => Ok(ConfigValue::Bool(false)),
            1 => Ok(ConfigValue::Bool(true)),
            _ => Err(TemplateError::Corrupt { reason: "bool" }),
        },
        2 => Ok(ConfigValue::Int(reader.u64()?)),
        3 => Ok(ConfigValue::Str(reader.string()?)),
        4 => Ok(ConfigValue::List(reader.string_list()?)),
        5 => Ok(ConfigValue::Enum(reader.string()?)),
        _ => Err(TemplateError::Corrupt { reason: "config" }),
    }
}

fn decision_code(decision: FileHandlingDecision) -> u8 {
    match decision {
        FileHandlingDecision::Include => 1,
        FileHandlingDecision::Exclude => 2,
        FileHandlingDecision::OmitContent => 3,
        FileHandlingDecision::DebugVisible => 4,
    }
}

fn decision_from(code: u8) -> Result<FileHandlingDecision, TemplateError> {
    match code {
        1 => Ok(FileHandlingDecision::Include),
        2 => Ok(FileHandlingDecision::Exclude),
        3 => Ok(FileHandlingDecision::OmitContent),
        4 => Ok(FileHandlingDecision::DebugVisible),
        _ => Err(TemplateError::Corrupt { reason: "decision" }),
    }
}

fn network_code(policy: NetworkPolicy) -> u8 {
    match policy {
        NetworkPolicy::None => 0,
        NetworkPolicy::Loopback => 1,
        NetworkPolicy::External => 2,
    }
}

fn network_from(code: u8) -> Result<NetworkPolicy, TemplateError> {
    match code {
        0 => Ok(NetworkPolicy::None),
        1 => Ok(NetworkPolicy::Loopback),
        2 => Ok(NetworkPolicy::External),
        _ => Err(TemplateError::Corrupt { reason: "network" }),
    }
}

fn state_code(state: PluginState) -> u8 {
    match state {
        PluginState::Enabled => 1,
        PluginState::Disabled => 2,
        PluginState::Unavailable => 3,
    }
}

fn state_from(code: u8) -> Result<PluginState, TemplateError> {
    match code {
        1 => Ok(PluginState::Enabled),
        2 => Ok(PluginState::Disabled),
        3 => Ok(PluginState::Unavailable),
        _ => Err(TemplateError::Corrupt { reason: "plugin state" }),
    }
}

fn count(len: usize) -> Result<u32, TemplateError> {
    u32::try_from(len).map_err(|_| TemplateError::Limit { what: "count", limit: 100_000 })
}

fn read_section<'a>(reader: &mut Reader<'a>) -> Result<(u16, u16, &'a [u8]), TemplateError> {
    let kind = reader.u16()?;
    let schema = reader.u16()?;
    let len = usize::try_from(reader.u32()?).unwrap_or(usize::MAX);
    if len > usize::try_from(MAX_PLAN_BYTES).unwrap_or(usize::MAX) {
        return Err(TemplateError::Limit { what: "section", limit: u64::from(MAX_PLAN_BYTES) });
    }
    Ok((kind, schema, reader.take(len)?))
}

struct Buf(Vec<u8>);

impl Buf {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn raw(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    fn str(&mut self, text: &str) -> Result<(), TemplateError> {
        if text.len() > MAX_STRING_BYTES {
            return Err(TemplateError::Limit { what: "string", limit: MAX_STRING_BYTES as u64 });
        }
        self.u32(u32::try_from(text.len()).map_err(|_| TemplateError::Limit {
            what: "string",
            limit: MAX_STRING_BYTES as u64,
        })?);
        self.raw(text.as_bytes());
        Ok(())
    }

    fn strs(&mut self, items: &[String]) -> Result<(), TemplateError> {
        self.u32(count(items.len())?);
        for item in items {
            self.str(item)?;
        }
        Ok(())
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    fn rest(&self) -> usize {
        self.data.len().saturating_sub(self.at)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], TemplateError> {
        let end = self.at.checked_add(len).ok_or(TemplateError::Corrupt { reason: "truncated" })?;
        if end > self.data.len() {
            return Err(TemplateError::Corrupt { reason: "truncated" });
        }
        let out = &self.data[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, TemplateError> {
        let bytes = self.take(1)?;
        Ok(bytes[0])
    }

    fn u16(&mut self) -> Result<u16, TemplateError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, TemplateError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, TemplateError> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]]))
    }

    fn string(&mut self) -> Result<String, TemplateError> {
        let len = usize::try_from(self.u32()?).unwrap_or(usize::MAX);
        if len > MAX_STRING_BYTES {
            return Err(TemplateError::Limit { what: "string", limit: MAX_STRING_BYTES as u64 });
        }
        let bytes = self.take(len)?;
        std::str::from_utf8(bytes).map(str::to_string).map_err(|_| TemplateError::Corrupt { reason: "utf8" })
    }

    fn string_list(&mut self) -> Result<Vec<String>, TemplateError> {
        let n = self.count()?;
        let mut items = Vec::with_capacity(n);
        for _ in 0..n {
            items.push(self.string()?);
        }
        Ok(items)
    }

    fn count(&mut self) -> Result<usize, TemplateError> {
        let n = usize::try_from(self.u32()?).unwrap_or(usize::MAX);
        if n > 100_000 {
            return Err(TemplateError::Limit { what: "count", limit: 100_000 });
        }
        Ok(n)
    }

    fn finish(&self, reason: &'static str) -> Result<(), TemplateError> {
        if self.rest() == 0 {
            Ok(())
        } else {
            Err(TemplateError::Corrupt { reason })
        }
    }
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
