//! Read-continuation cursors and the byte-span slicing behind the read verb.
//! A resume id carries the node id and the absolute line window; `slice_span` serves a window of
//! the node bytes and marks the cap when a prefix was cut. `OMITTED_LINE` and `TRUNCATED_MARK`
//! are the visible marks the read path keeps in the body.

use crate::domain::DctxError;

pub(super) const OMITTED_LINE: &str = "[DCTX_OMITTED]\n";
pub(super) const TRUNCATED_MARK: &str = "[DCTX_TRUNCATED]";

#[derive(Debug)]
pub(super) struct SlicedSource {
    pub(super) text: String,
    pub(super) capped: bool,
}

pub(super) struct ReadResume {
    pub(super) id: String,
    pub(super) start: u32,
    pub(super) end: u32,
    pub(super) beyond: bool,
}

pub(super) fn truncation_line(next_line: u32, end_line: u32) -> String {
    format!("{TRUNCATED_MARK} lines {next_line}-{end_line}\n")
}

pub(super) fn read_cursor(generation: u64, id: &str, start: u32, end: u32, beyond: bool) -> String {
    let flag = if beyond { "1" } else { "0" };
    format!("v1|{generation}|read|{id}|{start}|{end}|{flag}")
}

pub(super) fn parse_resume(id: &str) -> Result<ReadResume, DctxError> {
    let (head, flag) = id.rsplit_once('|').ok_or(DctxError::StaleContinuation)?;
    let (head, end_text) = head.rsplit_once('|').ok_or(DctxError::StaleContinuation)?;
    let (node, start_text) = head.rsplit_once('|').ok_or(DctxError::StaleContinuation)?;
    let start = start_text.parse::<u32>().map_err(|_| DctxError::StaleContinuation)?;
    let end = end_text.parse::<u32>().map_err(|_| DctxError::StaleContinuation)?;
    let beyond = match flag {
        "1" => true,
        "0" => false,
        _ => return Err(DctxError::StaleContinuation),
    };
    if node.is_empty() || start == 0 || end < start {
        return Err(DctxError::StaleContinuation);
    }
    Ok(ReadResume { id: node.to_string(), start, end, beyond })
}

pub(super) fn slice_span(
    bytes: &[u8],
    span: [u64; 2],
    node_lines: [u32; 2],
    lines: u32,
    absolute_start: Option<u32>,
) -> Result<SlicedSource, DctxError> {
    let start = usize::try_from(span[0]).map_err(|_| DctxError::CorruptIndex { reason: "span" })?;
    let end = usize::try_from(span[1]).map_err(|_| DctxError::CorruptIndex { reason: "span" })?;
    if start > end || end > bytes.len() {
        return Err(DctxError::CorruptIndex { reason: "span" });
    }
    let text = std::str::from_utf8(&bytes[start..end]).map_err(|_| DctxError::CorruptIndex { reason: "span" })?;
    if lines == 0 {
        return Ok(SlicedSource { text: text.to_string(), capped: false });
    }
    let (skip, take) = match absolute_start {
        None => (0usize, usize::try_from(lines).unwrap_or(usize::MAX)),
        Some(line) => absolute_window(line, lines, node_lines)?,
    };
    let mut out = String::new();
    let mut index = 0usize;
    let mut taken = 0usize;
    let mut truncated = false;
    for line in text.split_inclusive('\n') {
        if index < skip {
            index += 1;
            continue;
        }
        if taken >= take {
            truncated = true;
            break;
        }
        out.push_str(line);
        taken += 1;
        index += 1;
    }
    if absolute_start.is_some() && taken != take {
        return Err(DctxError::invalid("lines are outside the node span"));
    }
    let capped = truncated && absolute_start.is_none();
    if capped {
        out.push_str(OMITTED_LINE);
    }
    Ok(SlicedSource { text: out, capped })
}

fn absolute_window(line: u32, lines: u32, node_lines: [u32; 2]) -> Result<(usize, usize), DctxError> {
    let last = line.checked_add(lines.saturating_sub(1)).ok_or_else(|| DctxError::invalid("lines are outside the node span"))?;
    if line == 0 || line < node_lines[0] || last > node_lines[1] {
        return Err(DctxError::invalid("lines are outside the node span"));
    }
    let skip = usize::try_from(line - node_lines[0]).unwrap_or(usize::MAX);
    let take = usize::try_from(lines).unwrap_or(usize::MAX);
    Ok((skip, take))
}
