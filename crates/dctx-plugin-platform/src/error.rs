// SPDX-License-Identifier: Apache-2.0
//! Redacted platform failures and explicit missing guarantees.
//! Implements S-ADMISSION/S-FILE: failures never cause unrestricted execution.
//! Depends on protocol error codes; paths, environment and OS messages stay private.

use dctx_plugin_protocol::ErrorCode;

/// A guarantee unavailable with the current approved implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsupported {
    /// No tested provider for this target.
    Target,
    /// Safe, atomic, single-use transition into an arbitrary executable is absent.
    OneShotExec,
    /// The bundled image/runtime identity and complete lifecycle are not qualified.
    BundledLaunch,
    /// The separately qualified Git recipe and namespace exclusion are absent.
    ProgramRecipe,
    /// No compare-and-replace primitive or enforced writer exclusion.
    ConditionalReplace,
    /// Kernel lacks required openat2 resolution semantics.
    PathResolution,
    /// The kernel cannot fully enforce the required Landlock rights.
    Landlock,
    /// The syscall filter cannot be installed or this architecture is unqualified.
    Seccomp,
    /// The private entry has other threads, descriptors, environment or non-pipe stdio.
    Bootstrap,
    /// The parent cannot be bound or the required process limits cannot be installed.
    Lifecycle,
    /// No approved safe Seatbelt API/one-shot exec/lifecycle composition.
    Seatbelt,
}

/// An adapter failure with no raw path, environment, contents or syscall diagnostics.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// A required guarantee is unavailable; there is no fallback.
    #[error(
        "unsupported platform guarantee: {0:?}; next: use a qualified provider for this operation"
    )]
    Unsupported(Unsupported),
    /// The requested path is not a canonical literal relative resource.
    #[error(
        "invalid relative resource; next: supply a bounded canonical relative name without traversal or reserved components"
    )]
    InvalidPath,
    /// The checked object or bytes changed.
    #[error("resource identity conflict; next: reauthorize the current object identity and digest")]
    Conflict,
    /// The admitted byte bound was exceeded.
    #[error(
        "resource limit exceeded; next: reduce the requested byte range or entry parameters to their documented bounds"
    )]
    ResourceLimit,
    /// The object is missing or cannot be safely opened.
    #[error(
        "resource unavailable; next: check the authorized root and resource name before retrying"
    )]
    Unavailable,
    /// A local IO operation failed; callers must not expose its raw diagnostics.
    #[error(
        "platform IO failure; next: check local resource availability and retry from a newly authorized handle"
    )]
    Io,
}

impl PlatformError {
    /// Frozen protocol category; capability details remain host-side audit metadata.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Unsupported(_) => ErrorCode::UnsupportedPlatform,
            Self::InvalidPath => ErrorCode::InvalidMessage,
            Self::Conflict => ErrorCode::Conflict,
            Self::ResourceLimit => ErrorCode::ResourceLimit,
            Self::Unavailable | Self::Io => ErrorCode::Denied,
        }
    }
}
