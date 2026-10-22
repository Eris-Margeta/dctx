//! Budget-aware pagination around the model's whole-record renderer.
//! Implements SPEC-MAP-MODE section 3 without skipping budget-cut records.
//! Used by library and CLI; source records are shortened only at line boundaries.
use std::fmt::Write as _;
use crate::{Answer, AnswerBudget, Error, Format, Request, Result, Verb};
use dctx_map_model::adapters::{RenderError, render as model_render};

/// Renders complete records and an exact continuation in the selected format.
///
/// # Errors
/// Propagates invalid-answer, continuation, byte-budget and JSON rendering errors.
pub fn render_answer(answer: &Answer, request: &Request, format: Format) -> Result<String> {
    render_answer_at(answer, request, format, None)
}

/// Renders with an explicit place retained in every continuation command.
///
/// # Errors
/// Propagates invalid-answer, continuation, byte-budget and JSON rendering errors.
pub fn render_answer_at(
    answer: &Answer,
    request: &Request,
    format: Format,
    place: Option<&str>,
) -> Result<String> {
    let mut page = answer.clone();
    if page.verb == Verb::Read && !page.records.is_empty() {
        return read_page(&mut page, request, format, place);
    }
    if page.verb == Verb::Explain && !page.records.is_empty() {
        return explain_page(&mut page, request, format, place);
    }
    let total = page.records.len();
    for keep in (0..=total).rev() {
        page.records.truncate(keep);
        page.more = answer.more + total - keep;
        if page.more > 0 {
            page.next = if keep == 0 {
                let mut next = request.clone();
                next.budget = AnswerBudget::new(2000).map_err(|e| Error::Request(e.to_string()))?;
                next.command(request.offset)
            } else {
                request.command(request.offset.saturating_add(keep))
            };
        }
        if page.identity.is_some()
            && page.records.is_empty()
            && !matches!(
                page.identity,
                Some(dctx_map_model::IdentityStatus::Gone { .. })
            )
        {
            page.identity = None;
        }
        let output = render(&page, request.budget, format, place)?;
        if retained(&output, &page, format)? == keep {
            return Ok(output);
        }
    }
    Err(RenderError::BudgetTooSmall.into())
}
fn render(
    page: &Answer,
    budget: AnswerBudget,
    format: Format,
    place: Option<&str>,
) -> Result<String> {
    let mut page = page.clone();
    if let Some(place) = place {
        write!(page.next," --place {}", crate::domain::quote(place)).expect("writing to a String cannot fail");
    }
    if format == Format::Json && !page.next.ends_with(" --json") {
        page.next.push_str(" --json");
    }
    Ok(model_render(&page, budget, format)?)
}
fn retained(output: &str, page: &Answer, format: Format) -> Result<usize> {
    if format == Format::Json {
        let value: serde_json::Value = serde_json::from_str(output)?;
        Ok(value["records"].as_array().map_or(0, Vec::len))
    } else {
        Ok(output
            .lines()
            .filter(|line| {
                page.records.iter().any(|r| {
                    line.starts_with(&format!("{}  ", r.id))
                        || line.contains(&format!("  {}  ", r.id))
                        || *line == format!("id: {}", r.id)
                })
            })
            .count())
    }
}
fn explain_page(
    page: &mut Answer,
    request: &Request,
    format: Format,
    place: Option<&str>,
) -> Result<String> {
    loop {
        let output = render(page, request.budget, format, place)?;
        if retained(&output, page, format)? > 0 {
            return Ok(output);
        }
        let record = &mut page.records[0];
        if !record.documents.is_empty() {
            record.documents.pop();
        } else if !record.tests.is_empty() {
            record.tests.pop();
        } else if !record.outgoing.is_empty() {
            record.outgoing.pop();
        } else if !record.incoming.is_empty() {
            record.incoming.pop();
        } else {
            page.reason = Some("node details exceed budget".into());
            page.next = format!(
                "dctx map read {} --budget 2000",
                crate::domain::quote(record.id.as_str())
            );
            return render(page, request.budget, format, place);
        }
    }
}
#[allow(clippy::cast_possible_truncation, reason = "Head, tail and maximum are derived from a page ceiling of 120 or 400 lines, so they fit u32.")]
fn read_page(
    page: &mut Answer,
    request: &Request,
    format: Format,
    place: Option<&str>,
) -> Result<String> {
    let original = page.records[0].clone();
    let lines: Vec<_> = original
        .body
        .as_deref()
        .unwrap_or("")
        .split_inclusive('\n')
        .collect();
    let maximum = if request.full { 400 } else { 120 };
    if lines.len() <= maximum {
        let output = render(page, request.budget, format, place)?;
        if retained(&output, page, format)? > 0 {
            return Ok(output);
        }
    }
    for keep in (2..=lines.len().min(maximum)).rev() {
        if keep < lines.len() {
            let head = keep.div_ceil(2);
            let tail = (keep / 2).saturating_sub(1);
            let omitted = lines.len() - head - tail;
            let start = original.l[0] + head as u32;
            let mut next = request.clone();
            next.lines = Some([
                start,
                (original.l[1] - tail as u32).min(start + maximum as u32 - 1),
            ]);
            page.next = next.command(0);
            page.more = 1;
            let mut body = lines[..head].concat();
            if !body.ends_with('\n') {
                body.push('\n');
            }
            writeln!(body, "… {omitted} lines ({})", page.next).expect("writing to a String cannot fail");
            if tail > 0 {
                body.push_str(&lines[lines.len() - tail..].concat());
            }
            page.records[0].body = Some(body);
        }
        let output = render(page, request.budget, format, place)?;
        if retained(&output, page, format)? > 0 {
            return Ok(output);
        }
    }
    page.records[0] = original;
    page.reason = Some("source exceeds budget; narrow --lines or increase --budget".into());
    let mut next = request.clone();
    next.lines = Some([page.records[0].l[0], page.records[0].l[0]]);
    next.budget = AnswerBudget::new(2000).map_err(|e| Error::Request(e.to_string()))?;
    page.next = next.command(0);
    render(page, request.budget, format, place)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_wire_records() {
        let page = Answer {
            place: "p".into(),
            rev: "abcdef0".into(),
            dirty: 0,
            age_seconds: 0,
            verb: Verb::Find,
            records: vec![],
            more: 0,
            next: "dctx map map".into(),
            identity: None,
            reason: None,
        };
        assert_eq!(
            retained("{\"records\":[]}", &page, Format::Json).unwrap(),
            0
        );
    }
}
