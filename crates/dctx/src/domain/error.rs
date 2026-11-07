// SPDX-License-Identifier: Apache-2.0
//! Typed facade errors and the stable code set.
//! Implements contracts/API-PORTS-v1.md ("Typed error set") plus stale and identity refusals.
//! Depends on dctx-core and dctx-template only to translate their errors. Callers match [`DctxError`].
//! Messages carry a bounded detail and never a denied path or file bytes.

use std::fmt;

use dctx_core::error::DctxError as CoreError;
use dctx_core::error::Resource as CoreResource;
use dctx_template::TemplateError;

/// Longest adapter or template detail kept on an error, in bytes.
const MAX_DETAIL_BYTES: usize = 160;

/// A bounded quantity a request or a source can exceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    /// Tree entries visited by one snapshot.
    Entries,
    /// Directory nesting.
    Depth,
    /// Bytes of one file.
    FileBytes,
    /// Bytes read from the whole source.
    SourceBytes,
    /// Bytes of a path or key.
    PathBytes,
    /// Answer records.
    Records,
    /// Estimated tokens.
    Tokens,
    /// Work counter.
    Work,
    /// Segment or parse memory.
    Memory,
    /// Milliseconds past the injected clock.
    Deadline,
}

impl Resource {
    /// Stable name used in error text.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Entries => "entries",
            Self::Depth => "depth",
            Self::FileBytes => "file_bytes",
            Self::SourceBytes => "source_bytes",
            Self::PathBytes => "path_bytes",
            Self::Records => "records",
            Self::Tokens => "tokens",
            Self::Work => "work",
            Self::Memory => "memory",
            Self::Deadline => "deadline",
        }
    }
}

/// Stable machine-readable code of a [`DctxError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// See [`DctxError::InvalidRequest`].
    InvalidRequest,
    /// See [`DctxError::PolicyDenied`].
    PolicyDenied,
    /// See [`DctxError::PolicyUnavailable`].
    PolicyUnavailable,
    /// See [`DctxError::UnsupportedCapability`].
    UnsupportedCapability,
    /// See [`DctxError::UnsupportedVersion`].
    UnsupportedVersion,
    /// See [`DctxError::UnsupportedIdentity`].
    UnsupportedIdentity,
    /// See [`DctxError::UnsupportedDurability`].
    UnsupportedDurability,
    /// See [`DctxError::BudgetTooSmall`].
    BudgetTooSmall,
    /// See [`DctxError::ResourceLimit`].
    ResourceLimit,
    /// See [`DctxError::CacheLimit`].
    CacheLimit,
    /// See [`DctxError::Busy`].
    Busy,
    /// See [`DctxError::Cancelled`].
    Cancelled,
    /// See [`DctxError::Stale`].
    Stale,
    /// See [`DctxError::StaleContinuation`].
    StaleContinuation,
    /// See [`DctxError::CorruptIndex`].
    CorruptIndex,
    /// See [`DctxError::SourceChanged`].
    SourceChanged,
    /// See [`DctxError::SourceUnavailable`].
    SourceUnavailable,
    /// See [`DctxError::Io`].
    Io,
    /// See [`DctxError::Hook`].
    Hook,
}

impl ErrorCode {
    /// The code name used by conformance tests (`PolicyDenied`, `BudgetTooSmall`, …).
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "InvalidRequest",
            Self::PolicyDenied => "PolicyDenied",
            Self::PolicyUnavailable => "PolicyUnavailable",
            Self::UnsupportedCapability => "UnsupportedCapability",
            Self::UnsupportedVersion => "UnsupportedVersion",
            Self::UnsupportedIdentity => "UnsupportedIdentity",
            Self::UnsupportedDurability => "UnsupportedDurability",
            Self::BudgetTooSmall => "BudgetTooSmall",
            Self::ResourceLimit => "ResourceLimit",
            Self::CacheLimit => "CacheLimit",
            Self::Busy => "Busy",
            Self::Cancelled => "Cancelled",
            Self::Stale => "Stale",
            Self::StaleContinuation => "StaleContinuation",
            Self::CorruptIndex => "CorruptIndex",
            Self::SourceChanged => "SourceChanged",
            Self::SourceUnavailable => "SourceUnavailable",
            Self::Io => "Io",
            Self::Hook => "Hook",
        }
    }
}

/// A typed refusal or failure from the facade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DctxError {
    /// The request or a value in it is malformed.
    InvalidRequest {
        /// Static description of the violated rule.
        reason: &'static str,
        /// Bounded extra text. Empty when the reason is enough.
        detail: Box<str>,
    },
    /// The selection policy does not authorize the operation.
    PolicyDenied,
    /// The policy needed for an authorization could not be resolved.
    PolicyUnavailable,
    /// The operation needs a capability this build or session lacks.
    UnsupportedCapability {
        /// Stable capability name (`git`, `syntax-rust`, `template`, …).
        capability: &'static str,
        /// Bounded extra text, such as the template token that was refused.
        detail: Box<str>,
    },
    /// A request, plan or segment version this build cannot read.
    UnsupportedVersion {
        /// Version found.
        found: u32,
    },
    /// An identity algorithm this build does not hash or accept.
    UnsupportedIdentity {
        /// Algorithm name.
        algorithm: &'static str,
    },
    /// A durability promise this build will not make.
    UnsupportedDurability {
        /// Durability mode name.
        mode: &'static str,
    },
    /// Even the smallest answer page does not fit the budget.
    BudgetTooSmall {
        /// Smallest encoded size that would fit one page.
        minimum_bytes: u64,
    },
    /// A bounded quantity was exceeded.
    ResourceLimit {
        /// Which quantity.
        resource: Resource,
        /// The limit in force.
        limit: u64,
    },
    /// The cache cannot admit the requested bytes.
    CacheLimit {
        /// Bytes requested.
        requested: u64,
        /// Bytes still available.
        available: u64,
    },
    /// Another writer holds the resource.
    Busy,
    /// The caller cancelled the operation.
    Cancelled,
    /// A pinned fact is no longer valid for this request.
    Stale {
        /// Static description.
        reason: &'static str,
    },
    /// A continuation cursor does not belong to this generation or ranking.
    StaleContinuation,
    /// A stored object failed validation.
    CorruptIndex {
        /// Static description of the failed check.
        reason: &'static str,
    },
    /// The source bytes no longer match the indexed digest.
    SourceChanged,
    /// The source root is missing or cannot be opened.
    SourceUnavailable,
    /// An adapter IO failure, with a bounded detail.
    Io {
        /// Truncated adapter message.
        detail: Box<str>,
    },
    /// A plugin hook failed. `output_committed` is true only after the engine published.
    Hook {
        /// Hook boundary name.
        hook: &'static str,
        /// Host reason. A static token, never a path or file bytes.
        reason: &'static str,
        /// True when the pack, index or cache result is already published.
        output_committed: bool,
    },
}

impl DctxError {
    /// An invalid request with no extra detail.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn invalid(reason: &'static str) -> Self {
        Self::InvalidRequest {
            reason,
            detail: Box::from(""),
        }
    }

    /// An invalid request whose extra text is cut to at most 160 bytes
    /// at a UTF-8 character boundary.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn invalid_detail(reason: &'static str, detail: &str) -> Self {
        Self::InvalidRequest {
            reason,
            detail: bounded_detail(detail),
        }
    }

    /// A missing capability with no extra detail.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn unsupported(capability: &'static str) -> Self {
        Self::UnsupportedCapability {
            capability,
            detail: Box::from(""),
        }
    }

    /// Builds an [`DctxError::Io`] whose detail is cut at a char boundary.
    pub fn io(detail: impl fmt::Display) -> Self {
        Self::Io {
            detail: bounded_detail(&detail.to_string()),
        }
    }

    /// Translates an engine error into the facade set.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn from_core(error: CoreError) -> Self {
        match error {
            CoreError::InvalidRequest { reason } => Self::invalid(reason),
            CoreError::PolicyDenied => Self::PolicyDenied,
            CoreError::PolicyUnavailable => Self::PolicyUnavailable,
            CoreError::UnsupportedCapability { capability } => Self::unsupported(capability),
            CoreError::UnsupportedVersion { found } => Self::UnsupportedVersion { found },
            CoreError::BudgetTooSmall { minimum_bytes } => Self::BudgetTooSmall { minimum_bytes },
            CoreError::ResourceLimit { resource, limit } => Self::ResourceLimit {
                resource: map_resource(resource),
                limit,
            },
            CoreError::CacheLimit {
                requested,
                available,
            } => Self::CacheLimit {
                requested,
                available,
            },
            CoreError::Busy => Self::Busy,
            CoreError::Cancelled => Self::Cancelled,
            CoreError::CorruptIndex { reason } => Self::CorruptIndex { reason },
            CoreError::SourceChanged => Self::SourceChanged,
            CoreError::SourceUnavailable => Self::SourceUnavailable,
            CoreError::Io { detail } => Self::io(detail),
        }
    }

    /// Translates a template refusal. Plugin and parser gaps stay capability errors.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn from_template(error: TemplateError) -> Self {
        let text = error.to_string();
        match error {
            TemplateError::UnknownCapability { name }
            | TemplateError::RequiredPluginMissing { id: name } => Self::UnsupportedCapability {
                capability: "template",
                detail: bounded_detail(&name),
            },
            TemplateError::UnsupportedVersion { found } => Self::UnsupportedVersion { found },
            TemplateError::BudgetTooSmall { minimum_bytes } => {
                Self::BudgetTooSmall { minimum_bytes }
            }
            TemplateError::BudgetAboveCeiling { .. } => {
                Self::invalid_detail("template budget exceeds the host ceiling", &text)
            }
            TemplateError::GrantExpansion { .. } | TemplateError::UserDisabled { .. } => {
                Self::PolicyDenied
            }
            TemplateError::Corrupt { reason } => Self::CorruptIndex { reason },
            TemplateError::DigestMismatch => Self::CorruptIndex { reason: "digest" },
            TemplateError::UnknownOpcode { .. } => Self::CorruptIndex { reason: "opcode" },
            TemplateError::UnknownSection { .. } => Self::CorruptIndex { reason: "section" },
            TemplateError::Limit { limit, .. } => Self::ResourceLimit {
                resource: Resource::Records,
                limit,
            },
            TemplateError::MissingSection { .. }
            | TemplateError::EmptySection { .. }
            | TemplateError::UnresolvedVariable { .. }
            | TemplateError::InvalidSyntax { .. }
            | TemplateError::Cycle { .. }
            | TemplateError::Duplicate { .. }
            | TemplateError::UnresolvedRef { .. }
            | TemplateError::UnknownField { .. }
            | TemplateError::ReservedAlias { .. } => {
                Self::invalid_detail("template rejected", &text)
            }
        }
    }

    /// The stable code of this error.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidRequest { .. } => ErrorCode::InvalidRequest,
            Self::PolicyDenied => ErrorCode::PolicyDenied,
            Self::PolicyUnavailable => ErrorCode::PolicyUnavailable,
            Self::UnsupportedCapability { .. } => ErrorCode::UnsupportedCapability,
            Self::UnsupportedVersion { .. } => ErrorCode::UnsupportedVersion,
            Self::UnsupportedIdentity { .. } => ErrorCode::UnsupportedIdentity,
            Self::UnsupportedDurability { .. } => ErrorCode::UnsupportedDurability,
            Self::BudgetTooSmall { .. } => ErrorCode::BudgetTooSmall,
            Self::ResourceLimit { .. } => ErrorCode::ResourceLimit,
            Self::CacheLimit { .. } => ErrorCode::CacheLimit,
            Self::Busy => ErrorCode::Busy,
            Self::Cancelled => ErrorCode::Cancelled,
            Self::Stale { .. } => ErrorCode::Stale,
            Self::StaleContinuation => ErrorCode::StaleContinuation,
            Self::CorruptIndex { .. } => ErrorCode::CorruptIndex,
            Self::SourceChanged => ErrorCode::SourceChanged,
            Self::SourceUnavailable => ErrorCode::SourceUnavailable,
            Self::Io { .. } => ErrorCode::Io,
            Self::Hook { .. } => ErrorCode::Hook,
        }
    }

    /// Whether the engine result this error refers to is already published.
    /// Only a post-commit hook failure returns true. Every other error returns false.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn output_committed(&self) -> bool {
        match self {
            Self::Hook {
                output_committed, ..
            } => *output_committed,
            _ => false,
        }
    }

    /// True when the same request may succeed later without a different grant.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self.code(),
            ErrorCode::Busy
                | ErrorCode::Io
                | ErrorCode::SourceChanged
                | ErrorCode::CacheLimit
                | ErrorCode::Stale
        )
    }
}

fn map_resource(resource: CoreResource) -> Resource {
    match resource {
        CoreResource::Entries => Resource::Entries,
        CoreResource::Depth => Resource::Depth,
        CoreResource::FileBytes => Resource::FileBytes,
        CoreResource::SourceBytes => Resource::SourceBytes,
        CoreResource::PathBytes => Resource::PathBytes,
    }
}

pub(crate) fn bounded_detail(text: &str) -> Box<str> {
    if text.len() <= MAX_DETAIL_BYTES {
        return Box::from(text);
    }
    let mut end = MAX_DETAIL_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Box::from(&text[..end])
}

impl fmt::Display for DctxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { reason, detail } => write_detail(formatter, reason, detail),
            Self::PolicyDenied => formatter.write_str("denied by selection policy"),
            Self::PolicyUnavailable => formatter.write_str("selection policy unavailable"),
            Self::UnsupportedCapability { capability, detail } => {
                write_detail(formatter, capability, detail)
            }
            Self::UnsupportedVersion { found } => write!(formatter, "unsupported version {found}"),
            Self::UnsupportedIdentity { algorithm } => {
                write!(formatter, "unsupported identity {algorithm}")
            }
            Self::UnsupportedDurability { mode } => {
                write!(formatter, "unsupported durability {mode}")
            }
            Self::BudgetTooSmall { minimum_bytes } => {
                write!(
                    formatter,
                    "budget too small: needs at least {minimum_bytes} bytes"
                )
            }
            Self::ResourceLimit { resource, limit } => {
                write!(
                    formatter,
                    "resource limit exceeded: {} > {limit}",
                    resource.as_str()
                )
            }
            Self::CacheLimit {
                requested,
                available,
            } => {
                write!(
                    formatter,
                    "cache limit: requested {requested} bytes, {available} available"
                )
            }
            Self::Busy => formatter.write_str("busy"),
            Self::Cancelled => formatter.write_str("cancelled"),
            Self::Stale { reason } => write!(formatter, "stale: {reason}"),
            Self::StaleContinuation => formatter.write_str("stale continuation"),
            Self::CorruptIndex { reason } => write!(formatter, "corrupt index: {reason}"),
            Self::SourceChanged => formatter.write_str("source changed during read"),
            Self::SourceUnavailable => formatter.write_str("source unavailable"),
            Self::Io { detail } => write!(formatter, "io: {detail}"),
            Self::Hook {
                hook,
                reason,
                output_committed,
            } => {
                write!(
                    formatter,
                    "hook {hook} failed (output_committed={output_committed}): {reason}"
                )
            }
        }
    }
}

fn write_detail(formatter: &mut fmt::Formatter<'_>, reason: &str, detail: &str) -> fmt::Result {
    if detail.is_empty() {
        formatter.write_str(reason)
    } else {
        write!(formatter, "{reason}: {detail}")
    }
}

impl std::error::Error for DctxError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_detail_is_bounded_and_codes_match_names() {
        let long = "é".repeat(200);
        let DctxError::Io { detail } = DctxError::io(&long) else {
            panic!("expected Io");
        };
        assert!(detail.len() <= MAX_DETAIL_BYTES);
        assert_eq!(DctxError::Cancelled.code().as_str(), "Cancelled");
        assert!(!DctxError::Cancelled.is_retryable());
        assert!(DctxError::Busy.is_retryable());
        assert!(DctxError::SourceChanged.is_retryable());
        let limit = DctxError::ResourceLimit {
            resource: Resource::Entries,
            limit: 3,
        };
        assert_eq!(limit.to_string(), "resource limit exceeded: entries > 3");
    }

    #[test]
    fn core_cancel_and_template_capability_translate() {
        let error = DctxError::from_core(CoreError::Cancelled);
        assert_eq!(error, DctxError::Cancelled);
        let template = DctxError::from_template(TemplateError::UnknownCapability {
            name: "parser:tree-sitter".to_string(),
        });
        assert_eq!(template.code(), ErrorCode::UnsupportedCapability);
        assert!(template.to_string().contains("tree-sitter"));
        let version = DctxError::from_template(TemplateError::UnsupportedVersion { found: 9 });
        assert_eq!(version, DctxError::UnsupportedVersion { found: 9 });
        assert!(!DctxError::Cancelled.output_committed());
        let before = DctxError::Hook {
            hook: "before-pack-output-written",
            reason: "hook-failed",
            output_committed: false,
        };
        let after = DctxError::Hook {
            hook: "after-pack-output-written",
            reason: "hook-failed",
            output_committed: true,
        };
        assert!(!before.output_committed());
        assert!(after.output_committed());
        assert!(before.to_string().contains("output_committed=false"));
        assert!(after.to_string().contains("output_committed=true"));
        assert_eq!(after.code().as_str(), "Hook");
    }
}
