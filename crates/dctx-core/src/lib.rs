//! dctx-core: the pure dctx domain (ids, selection decisions, budgets, errors, pack facts)
//! plus, for the DX-B0 bridge period, the legacy IO modules at their old paths.
//! Implements ARCHITECTURE-PROPOSAL §2 ("dctx-core: pure domain") and §7 Stage 1 row DX-B0.
//! Pure modules import no `std::fs`, environment, process or global output (see `tests/purity.rs`);
//! the `legacy` modules keep today's IO behaviour for dctx-cli and the desktop until removal.

#![forbid(unsafe_code)]
// Deny unwrap() in production code - use expect() with clear messages or proper error handling
// Tests are allowed to use unwrap() for convenience
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

// --- Pure domain ---
pub mod budget;
pub mod datatypes;
pub mod diff;
pub mod domain;
pub mod error;
pub mod facts;
pub mod file_classifier;
pub mod filter;
pub mod ids;
pub mod selection;
pub mod settings;
pub mod version;

// --- Legacy compatibility (IO; bridge period only) ---
mod legacy;
pub use legacy::{
    config, content_reader, git_safety, ignore, llm_formatter, omit_content, reader, registry,
    state, template, walker, writer,
};

pub use config::AppConfig;
pub use content_reader::ContentReader;
pub use datatypes::{DirContextTreeNode, IgnoreRule, NodeType, PatternType};
pub use diff::{ChangeType, DiffReport};
pub use domain::{FileHandlingDecision, ItemContext};
pub use error::{DctxError, ErrorCode};
pub use filter::FilterConfig;
// decision 078: no-op. dctx does not write or stage .gitignore, and does not call Git.
pub use git_safety::ensure_local_gitignore_is_safe;
pub use ids::{ContentDigest, RelativePath, SnapshotDigest, Timestamp, ViewKey};
pub use ignore::{IgnoreEngine, IgnoreError};
pub use registry::ProjectRegistry;
pub use selection::SelectionPolicy;
pub use template::{CompiledTemplate, Template, TemplateValidationError};
pub use version::Version;
