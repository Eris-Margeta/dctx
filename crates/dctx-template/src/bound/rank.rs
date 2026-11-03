// SPDX-License-Identifier: Apache-2.0
//! Permission narrowing for bounded answer rendering.

use dctx_core::FileHandlingDecision;

use crate::error::TemplateError;
use crate::model::{HostCeiling, Selection};

use super::policy::{host_decision, template_decision};

/// Keeps `requested` when it is no wider than `host` in content or visibility.
///
/// # Errors
/// Returns a grant-expansion error if the requested content or visibility exceeds the host decision.
pub fn narrow_decision(
    host: FileHandlingDecision,
    requested: FileHandlingDecision,
) -> Result<FileHandlingDecision, TemplateError> {
    if content_rank(requested) > content_rank(host)
        || visibility_rank(requested) > visibility_rank(host)
    {
        Err(TemplateError::GrantExpansion { kind: "decision" })
    } else {
        Ok(requested)
    }
}

/// Intersects the host grant with the template selection. Expansion is an error.
///
/// # Errors
/// Returns a grant-expansion error when template selection widens the host decision.
pub fn authorize_path(
    host: &HostCeiling,
    selection: &Selection,
    path: &str,
) -> Result<FileHandlingDecision, TemplateError> {
    narrow_decision(
        host_decision(host, path),
        template_decision(selection, path),
    )
}

fn content_rank(decision: FileHandlingDecision) -> u8 {
    match decision {
        FileHandlingDecision::Include => 2,
        FileHandlingDecision::OmitContent | FileHandlingDecision::DebugVisible => 1,
        FileHandlingDecision::Exclude => 0,
    }
}

fn visibility_rank(decision: FileHandlingDecision) -> u8 {
    match decision {
        FileHandlingDecision::Include | FileHandlingDecision::OmitContent => 2,
        FileHandlingDecision::DebugVisible => 1,
        FileHandlingDecision::Exclude => 0,
    }
}
