// SPDX-License-Identifier: Apache-2.0
//! Typed refusals for DTS parse, compile, load and render.
//! Implements contracts/DTS-v2.md ("typed errors; no fallback rendering") and API error names.
//! Depends on nothing. Used by every module in this crate and by callers of [`TemplateError`].
//! Messages name the rule that failed and do not carry file contents.

use std::fmt;

/// Spec ceilings a caller may narrow but not raise (DTS-v2 compiled IR envelope).
pub const MAX_PLAN_BYTES: u32 = 1024 * 1024;
/// Most sections in one plan. This IR always writes seven and rejects a lower host cap.
pub const MAX_SECTIONS: u32 = 64;
/// Most render instructions in one plan.
pub const MAX_INSTRUCTIONS: u32 = 10_000;
/// Longest single string stored in a plan, in bytes.
pub const MAX_STRING_BYTES: usize = 65_536;

/// Failure while compiling, loading or rendering a template. Each variant is a refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    /// A required section is absent.
    MissingSection {
        /// Section name.
        name: String,
    },
    /// A required section has no body.
    EmptySection {
        /// Section name.
        name: String,
    },
    /// A `{{name}}` reference has no definition. Expressions are not evaluated.
    UnresolvedVariable {
        /// The missing name.
        name: String,
    },
    /// The source or a field breaks the frozen grammar.
    InvalidSyntax {
        /// 1-based line, or 0 when the source has no line.
        line: u32,
        /// What is wrong.
        detail: String,
    },
    /// Variables, partials or plugin hooks refer to each other in a loop.
    Cycle {
        /// `variable`, `partial` or `plugin`.
        kind: &'static str,
        /// A name on the cycle.
        name: String,
    },
    /// The plan needs a parser, mode, fact or hook the host did not grant.
    UnknownCapability {
        /// Capability token, such as `parser:tree-sitter`.
        name: String,
    },
    /// A requested budget is higher than the host ceiling.
    BudgetAboveCeiling {
        /// Budget field.
        field: &'static str,
        /// Requested amount.
        requested: u64,
        /// Host ceiling.
        ceiling: u64,
    },
    /// A template tried to widen a host permission.
    GrantExpansion {
        /// `decision`, `network` or `plugin`.
        kind: &'static str,
    },
    /// The IR major or minor version is not 1.0.
    UnsupportedVersion {
        /// Major version found, or the source version when the source was rejected.
        found: u32,
    },
    /// A render instruction code is not in this IR.
    UnknownOpcode {
        /// The code.
        opcode: u16,
    },
    /// A section kind is not in this IR.
    UnknownSection {
        /// The kind.
        kind: u16,
    },
    /// The bytes are not a well-formed plan.
    Corrupt {
        /// Which check failed.
        reason: &'static str,
    },
    /// The trailing SHA-256 or a domain digest does not match.
    DigestMismatch,
    /// Even the smallest answer envelope does not fit.
    BudgetTooSmall {
        /// Smallest encoded size that would fit, in bytes.
        minimum_bytes: u64,
    },
    /// A count or size exceeds its cap.
    Limit {
        /// What was limited.
        what: &'static str,
        /// The cap.
        limit: u64,
    },
    /// Two definitions share one name.
    Duplicate {
        /// Section kind.
        kind: &'static str,
        /// The repeated name.
        name: String,
    },
    /// A view or plugin points at a name that is not defined.
    UnresolvedRef {
        /// `selection`, `index`, `budget`, `render`, `partial` or `plugin`.
        kind: &'static str,
        /// The missing name.
        name: String,
    },
    /// A required plugin is not in the host catalog.
    RequiredPluginMissing {
        /// Plugin id.
        id: String,
    },
    /// A user disable blocks a plugin the template marked required.
    UserDisabled {
        /// Plugin id.
        id: String,
    },
    /// A plugin config key is not in the signed manifest.
    UnknownField {
        /// Plugin id.
        plugin: String,
        /// Field name.
        field: String,
    },
    /// A plugin alias collides with a built-in command.
    ReservedAlias {
        /// The alias.
        alias: String,
    },
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSection { name } => write!(f, "missing section {name}"),
            Self::EmptySection { name } => write!(f, "empty section {name}"),
            Self::UnresolvedVariable { name } => write!(f, "unresolved variable {name}"),
            Self::InvalidSyntax { line, detail } => write!(f, "invalid syntax at {line}: {detail}"),
            Self::Cycle { kind, name } => write!(f, "cycle in {kind} {name}"),
            Self::UnknownCapability { name } => write!(f, "unknown capability {name}"),
            Self::BudgetAboveCeiling { field, requested, ceiling } => {
                write!(f, "budget {field} {requested} exceeds ceiling {ceiling}")
            }
            Self::GrantExpansion { kind } => write!(f, "grant expansion ({kind})"),
            Self::UnsupportedVersion { found } => write!(f, "unsupported version {found}"),
            Self::UnknownOpcode { opcode } => write!(f, "unknown opcode {opcode}"),
            Self::UnknownSection { kind } => write!(f, "unknown section {kind}"),
            Self::Corrupt { reason } => write!(f, "corrupt plan: {reason}"),
            Self::DigestMismatch => write!(f, "digest mismatch"),
            Self::BudgetTooSmall { minimum_bytes } => {
                write!(f, "budget too small, need {minimum_bytes} bytes")
            }
            Self::Limit { what, limit } => write!(f, "{what} exceeds {limit}"),
            Self::Duplicate { kind, name } => write!(f, "duplicate {kind} {name}"),
            Self::UnresolvedRef { kind, name } => write!(f, "unresolved {kind} {name}"),
            Self::RequiredPluginMissing { id } => write!(f, "required plugin {id} is missing"),
            Self::UserDisabled { id } => write!(f, "plugin {id} is disabled by the user"),
            Self::UnknownField { plugin, field } => write!(f, "unknown field {field} on {plugin}"),
            Self::ReservedAlias { alias } => write!(f, "reserved alias {alias}"),
        }
    }
}

impl std::error::Error for TemplateError {}

/// V1 structural failure. Display text matches the dctx-core template errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateValidationError {
    /// A required section is absent.
    MissingSection(String),
    /// A required section has no body.
    EmptySection(String),
    /// A `{{name}}` in a partial or layout is not defined.
    UnresolvedVariable(String),
    /// The source is not valid v1 syntax.
    InvalidSyntax(String),
    /// Two variables reference each other.
    Cycle(String),
}

impl fmt::Display for TemplateValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSection(s) => write!(f, "Missing required section: {s}"),
            Self::EmptySection(s) => write!(f, "Section is empty: {s}"),
            Self::UnresolvedVariable(s) => write!(f, "Unresolved variable: {{{{{s}}}}}"),
            Self::InvalidSyntax(s) => write!(f, "Invalid syntax: {s}"),
            Self::Cycle(s) => write!(f, "Cyclic reference: {s}"),
        }
    }
}

impl std::error::Error for TemplateValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_display_matches_v1_braces() {
        let text = TemplateValidationError::UnresolvedVariable("name".to_string()).to_string();
        assert_eq!(text, "Unresolved variable: {{name}}");
    }
}
