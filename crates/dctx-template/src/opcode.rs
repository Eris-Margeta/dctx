// SPDX-License-Identifier: Apache-2.0
//! Render bytecode. Opcodes are data, never calls into the host.
//! Implements contracts/DTS-v2.md RENDER ("no arbitrary evaluation").
//! Depends on [`crate::model`] and [`crate::v1`]. Used by lowering, the binary codec and bounded render.
//! Unknown opcodes are refused. Partial inclusion stops at a fixed depth.

use std::collections::HashMap;

use crate::error::{MAX_STRING_BYTES, TemplateError};
use crate::model::{Inst, NamedCode, OP_DATA, OP_MAX, OP_PARTIAL, OP_TEXT, RenderProgram};
use crate::v1::replace_vars_in_order;

/// Field ids stored in [`OP_DATA`] arguments.
pub fn field_id(name: &str) -> Option<u16> {
    match name {
        "node.type_char" => Some(1),
        "node.path" => Some(2),
        "node.id" => Some(3),
        "node.mod" => Some(4),
        "node.size_part" => Some(5),
        "file.id" => Some(6),
        "file.path" => Some(7),
        "file.lines" => Some(8),
        "file.size" => Some(9),
        "file.content" => Some(10),
        "comp.tree" => Some(11),
        "comp.files" => Some(12),
        "project" => Some(13),
        _ => None,
    }
}

/// True when `opcode` is one this IR executes and `arg` is in range for it.
pub fn instruction_ok(
    opcode: u16,
    arg: u16,
    strings: usize,
    partials: usize,
) -> Result<(), TemplateError> {
    match opcode {
        OP_TEXT => {
            if usize::from(arg) < strings {
                Ok(())
            } else {
                Err(TemplateError::Corrupt {
                    reason: "string index",
                })
            }
        }
        OP_DATA => {
            if (1..=13).contains(&arg) {
                Ok(())
            } else {
                Err(TemplateError::Corrupt {
                    reason: "data field",
                })
            }
        }
        OP_PARTIAL => {
            if usize::from(arg) < partials {
                Ok(())
            } else {
                Err(TemplateError::Corrupt {
                    reason: "partial index",
                })
            }
        }
        _ => Err(TemplateError::UnknownOpcode { opcode }),
    }
}

/// Rejects any opcode outside 1..=[`OP_MAX`] and any dangling index.
pub fn validate_program(program: &RenderProgram) -> Result<(), TemplateError> {
    for code in program.partials.iter().chain(program.layouts.iter()) {
        for inst in &code.instructions {
            if inst.opcode == 0 || inst.opcode > OP_MAX {
                return Err(TemplateError::UnknownOpcode {
                    opcode: inst.opcode,
                });
            }
            instruction_ok(
                inst.opcode,
                inst.arg,
                program.strings.len(),
                program.partials.len(),
            )?;
        }
    }
    Ok(())
}

/// Compiles partial bodies and layouts into one program. `bodies` are already variable-substituted.
pub fn compile_program(
    variables: &HashMap<String, String>,
    partials: &[(String, String)],
    layouts: &[(String, String)],
) -> Result<RenderProgram, TemplateError> {
    let mut strings = Vec::new();
    let mut index = HashMap::new();
    for (offset, (name, _)) in partials.iter().enumerate() {
        let id = u16::try_from(offset).map_err(|_| TemplateError::Limit {
            what: "partials",
            limit: 65535,
        })?;
        index.insert(name.as_str(), id);
    }
    let mut compiled_partials = Vec::new();
    for (name, body) in partials {
        compiled_partials.push(NamedCode {
            name: name.clone(),
            instructions: compile_text(body, &mut strings, &index)?,
        });
    }
    let mut compiled_layouts = Vec::new();
    for (name, body) in layouts {
        compiled_layouts.push(NamedCode {
            name: name.clone(),
            instructions: compile_text(body, &mut strings, &index)?,
        });
    }
    let mut variables: Vec<(String, String)> = variables
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    variables.sort();
    Ok(RenderProgram {
        strings,
        variables,
        partials: compiled_partials,
        layouts: compiled_layouts,
    })
}

fn compile_text(
    text: &str,
    strings: &mut Vec<String>,
    partials: &HashMap<&str, u16>,
) -> Result<Vec<Inst>, TemplateError> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut lit = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"{{{")
            && let Some(end) = text[i + 3..].find("}}}")
        {
            push_text(&mut out, strings, &text[lit..i])?;
            let name = &text[i + 3..i + 3 + end];
            let id = field_id(name).ok_or_else(|| TemplateError::UnresolvedVariable {
                name: name.to_string(),
            })?;
            out.push(Inst {
                opcode: OP_DATA,
                arg: id,
            });
            i += 3 + end + 3;
            lit = i;
            continue;
        }
        if bytes[i..].starts_with(b"{{>")
            && let Some(end) = text[i + 3..].find("}}")
        {
            let name = &text[i + 3..i + 3 + end];
            let Some(&id) = partials.get(name) else {
                return Err(TemplateError::UnresolvedRef {
                    kind: "partial",
                    name: name.to_string(),
                });
            };
            push_text(&mut out, strings, &text[lit..i])?;
            out.push(Inst {
                opcode: OP_PARTIAL,
                arg: id,
            });
            i += 3 + end + 2;
            lit = i;
            continue;
        }
        if bytes[i..].starts_with(b"{{")
            && let Some(end) = text[i + 2..].find("}}")
        {
            let name = &text[i + 2..i + 2 + end];
            if !name.is_empty() && !name.contains('{') && !name.starts_with('>') {
                return Err(TemplateError::UnresolvedVariable {
                    name: name.to_string(),
                });
            }
        }
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    push_text(&mut out, strings, &text[lit..])?;
    Ok(out)
}

fn push_text(
    out: &mut Vec<Inst>,
    strings: &mut Vec<String>,
    text: &str,
) -> Result<(), TemplateError> {
    if text.is_empty() {
        return Ok(());
    }
    if text.len() > MAX_STRING_BYTES || strings.len() >= usize::from(u16::MAX) {
        return Err(TemplateError::Limit {
            what: "strings",
            limit: MAX_STRING_BYTES as u64,
        });
    }
    let arg = u16::try_from(strings.len()).map_err(|_| TemplateError::Limit {
        what: "strings",
        limit: 65535,
    })?;
    strings.push(text.to_string());
    out.push(Inst {
        opcode: OP_TEXT,
        arg,
    });
    Ok(())
}

/// Executes `code`. `data` supplies field values. Inclusion depth is capped at 32.
pub fn execute(
    program: &RenderProgram,
    code: &[Inst],
    data: &mut dyn FnMut(u16) -> Result<String, TemplateError>,
    depth: u32,
) -> Result<String, TemplateError> {
    if depth > 32 {
        return Err(TemplateError::Limit {
            what: "include-depth",
            limit: 32,
        });
    }
    let mut buf = String::new();
    for inst in code {
        match inst.opcode {
            OP_TEXT => {
                let text =
                    program
                        .strings
                        .get(usize::from(inst.arg))
                        .ok_or(TemplateError::Corrupt {
                            reason: "string index",
                        })?;
                buf.push_str(text);
            }
            OP_DATA => buf.push_str(&data(inst.arg)?),
            OP_PARTIAL => {
                let partial =
                    program
                        .partials
                        .get(usize::from(inst.arg))
                        .ok_or(TemplateError::Corrupt {
                            reason: "partial index",
                        })?;
                let nested = execute(program, &partial.instructions, data, depth + 1)?;
                buf.push_str(&nested);
            }
            opcode => return Err(TemplateError::UnknownOpcode { opcode }),
        }
    }
    Ok(replace_vars_in_order(&buf, &program.variables))
}

/// `{{>name}}` targets in `text`, without the braces.
pub fn include_targets(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut names = Vec::new();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if bytes[i..].starts_with(b"{{>")
            && let Some(end) = text[i + 3..].find("}}")
        {
            let name = &text[i + 3..i + 3 + end];
            if !name.is_empty() && !name.contains('{') {
                names.push(name.to_string());
            }
            i += 3 + end + 2;
            continue;
        }
        i += 1;
    }
    names
}
