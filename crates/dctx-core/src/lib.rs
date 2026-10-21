//! dctx-core: the pure dctx domain (ids, selection decisions, budgets, errors, pack facts).
//! Implements ARCHITECTURE-PROPOSAL §2 and D3-LEGACY-REMOVAL §5 D3-08.
//! Depends on dctx-hash for content identity; used by ports, engines, adapters and frontends.
//! No filesystem, environment, process or global output APIs (see `tests/purity.rs`).

#![forbid(unsafe_code)]
#![deny(missing_docs)]
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

pub use datatypes::{DirContextTreeNode, IgnoreRule, NodeType, PatternType};
pub use diff::{ChangeType, DiffReport};
pub use domain::{FileHandlingDecision, ItemContext};
pub use error::{DctxError, ErrorCode};
pub use filter::FilterConfig;
pub use ids::{ContentDigest, RelativePath, SnapshotDigest, Timestamp, ViewKey};
pub use selection::SelectionPolicy;
pub use version::Version;
