// SPDX-License-Identifier: Apache-2.0
//! Selection, extraction, budget, view and render section payload codecs.
//! Implements contracts/DTS-v2.md compiled IR and IDENTITY-v1.
//! Depends on plan model types, opcode validation and bounded byte I/O; used by the `ContextPlan` binary codec.

use dctx_core::FileHandlingDecision;

use crate::error::{MAX_INSTRUCTIONS, TemplateError};
use crate::model::{
    BudgetPlan, IndexPlan, Inst, NamedCode, OP_MAX, RenderProgram, SelectRule, Selection, ViewPlan,
};
use crate::opcode::instruction_ok;

use super::io::{Buf, Reader, count};

pub(super) fn encode_selections(items: &[Selection]) -> Result<Vec<u8>, TemplateError> {
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

pub(super) fn decode_selections(payload: &[u8]) -> Result<Vec<Selection>, TemplateError> {
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
            rules.push(SelectRule {
                decision: decision_from(reader.u8()?)?,
                pattern: reader.string()?,
            });
        }
        items.push(Selection {
            name,
            roots,
            languages,
            kinds,
            rules,
        });
    }
    reader.finish("payload")?;
    Ok(items)
}

pub(super) fn encode_indexes(items: &[IndexPlan]) -> Result<Vec<u8>, TemplateError> {
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

pub(super) fn decode_indexes(payload: &[u8]) -> Result<Vec<IndexPlan>, TemplateError> {
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

pub(super) fn encode_budgets(items: &[BudgetPlan]) -> Result<Vec<u8>, TemplateError> {
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

pub(super) fn decode_budgets(payload: &[u8]) -> Result<Vec<BudgetPlan>, TemplateError> {
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

pub(super) fn encode_views(items: &[ViewPlan]) -> Result<Vec<u8>, TemplateError> {
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

pub(super) fn decode_views(payload: &[u8]) -> Result<Vec<ViewPlan>, TemplateError> {
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

pub(super) fn encode_render(program: &RenderProgram) -> Result<Vec<u8>, TemplateError> {
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

pub(super) fn decode_render(payload: &[u8]) -> Result<RenderProgram, TemplateError> {
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
    let program = RenderProgram {
        strings,
        variables,
        partials,
        layouts,
    };
    for code in program.partials.iter().chain(program.layouts.iter()) {
        for inst in &code.instructions {
            instruction_ok(
                inst.opcode,
                inst.arg,
                program.strings.len(),
                program.partials.len(),
            )?;
        }
    }
    Ok(program)
}

fn read_codes(
    reader: &mut Reader<'_>,
    instructions: &mut u64,
) -> Result<Vec<NamedCode>, TemplateError> {
    let n = reader.count()?;
    let mut items = Vec::with_capacity(n);
    for _ in 0..n {
        let name = reader.string()?;
        let count = reader.count()?;
        let mut body = Vec::with_capacity(count);
        for _ in 0..count {
            *instructions = instructions.saturating_add(1);
            if *instructions > u64::from(MAX_INSTRUCTIONS) {
                return Err(TemplateError::Limit {
                    what: "instructions",
                    limit: u64::from(MAX_INSTRUCTIONS),
                });
            }
            let opcode = reader.u16()?;
            let arg = reader.u16()?;
            if opcode == 0 || opcode > OP_MAX {
                return Err(TemplateError::UnknownOpcode { opcode });
            }
            body.push(Inst { opcode, arg });
        }
        items.push(NamedCode {
            name,
            instructions: body,
        });
    }
    Ok(items)
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
