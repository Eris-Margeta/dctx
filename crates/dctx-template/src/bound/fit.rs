// SPDX-License-Identifier: Apache-2.0
//! Budget checks and output fitting for bounded answer rendering.

use dctx_core::budget::estimate_tokens;

use crate::error::TemplateError;
use crate::model::{BudgetPlan, HostCeiling};

use super::CONTINUED;

pub(super) fn budget_within(budget: &BudgetPlan, host: &HostCeiling) -> Result<(), TemplateError> {
    within(budget.bytes, host.max_bytes, "bytes")?;
    within(
        budget.estimated_tokens,
        host.max_estimated_tokens,
        "estimated_tokens",
    )?;
    within(
        budget.hard_estimated_tokens,
        host.max_hard_estimated_tokens,
        "hard_estimated_tokens",
    )?;
    within(budget.records, host.max_records, "records")?;
    within(u64::from(budget.depth), u64::from(host.max_depth), "depth")?;
    within(
        budget.parse_time_ms,
        host.max_parse_time_ms,
        "parse_time_ms",
    )?;
    within(budget.parse_memory, host.max_parse_memory, "parse_memory")?;
    within(budget.work, host.max_work, "work")?;
    if budget.hard_estimated_tokens < budget.estimated_tokens {
        return Err(TemplateError::InvalidSyntax {
            line: 0,
            detail: "hard budget".into(),
        });
    }
    Ok(())
}

fn within(requested: u64, ceiling: u64, field: &'static str) -> Result<(), TemplateError> {
    if requested > ceiling {
        Err(TemplateError::BudgetAboveCeiling {
            field,
            requested,
            ceiling,
        })
    } else {
        Ok(())
    }
}

pub(super) fn choose(
    total: usize,
    budget: &BudgetPlan,
    mut render: impl FnMut(usize, bool) -> Result<String, TemplateError>,
) -> Result<String, TemplateError> {
    let cap = usize::try_from(budget.records).unwrap_or(usize::MAX);
    let max_count = total.min(cap);
    for count in (0..=max_count).rev() {
        let truncated = count < total;
        let text = render(count, truncated)?;
        if fits(&text, budget) {
            return Ok(text);
        }
    }
    let probe = render(0, total > 0)?;
    let minimum_bytes = u64::try_from(probe.len()).unwrap_or(u64::MAX);
    Err(TemplateError::BudgetTooSmall { minimum_bytes })
}

fn fits(text: &str, budget: &BudgetPlan) -> bool {
    let bytes = u64::try_from(text.len()).unwrap_or(u64::MAX);
    if bytes > budget.bytes {
        return false;
    }
    let tokens = estimate_tokens(bytes);
    tokens <= budget.estimated_tokens && tokens <= budget.hard_estimated_tokens
}

pub(super) fn finish_marker(body: String, truncated: bool) -> String {
    if truncated {
        let mut out = body;
        out.push_str(CONTINUED);
        out
    } else {
        body
    }
}
