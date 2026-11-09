// SPDX-License-Identifier: Apache-2.0
//! Record rendering for bounded answer rendering.

use dctx_core::FileHandlingDecision;

use crate::error::TemplateError;
use crate::model::RenderProgram;
use crate::opcode::execute;
use crate::v1::escape_path;

use super::json::{named, variable};
use super::{Prepared, file_text, file_value, node_value};

pub(super) struct Fragment {
    pub(super) tree: String,
    pub(super) file: String,
}

pub(super) fn render_record(
    program: &RenderProgram,
    item: &Prepared,
    counter: &mut usize,
) -> Result<Fragment, TemplateError> {
    let id = if item.depth == 0 && item.is_dir {
        "ROOT".to_string()
    } else if item.is_dir {
        let id = format!("D{counter:03}");
        *counter += 1;
        id
    } else {
        let id = format!("F{counter:03}");
        *counter += 1;
        id
    };
    let type_char = if item.is_dir { "[D]" } else { "[F]" };
    let attr = variable(program, "attr_size").unwrap_or("SIZE");
    let size_part = if item.is_dir {
        String::new()
    } else {
        format!(", {attr}:{}", item.size)
    };
    let depth = usize::try_from(item.depth).unwrap_or(usize::MAX).min(4096);
    let indent = "  ".repeat(depth);
    let path = escape_path(&item.path);
    let display = if item.decision == FileHandlingDecision::DebugVisible {
        format!("{path} [IGNORED]")
    } else {
        path.to_string()
    };
    let modified = item.modified.to_string();
    let mut tree = String::new();
    if let Some(code) = named(&program.partials, "TREE_ENTRY") {
        let body = execute(
            program,
            code,
            &mut |field| {
                Ok(node_value(
                    field, type_char, &display, &id, &modified, &size_part,
                ))
            },
            0,
        )?;
        tree.push_str(&indent);
        tree.push_str(&body);
        tree.push('\n');
    }
    let mut file = String::new();
    if !item.is_dir
        && item.decision != FileHandlingDecision::DebugVisible
        && let Some(code) = named(&program.partials, "FILE_BLOCK")
    {
        let (lines, text) = file_text(item);
        let size = item.size.to_string();
        let lines = lines.to_string();
        let body = execute(
            program,
            code,
            &mut |field| Ok(file_value(field, &id, &path, &lines, &size, &text)),
            0,
        )?;
        file.push_str(&body);
        file.push('\n');
    }
    Ok(Fragment { tree, file })
}
