// SPDX-License-Identifier: Apache-2.0
//! Pure selection, confinement, and freshness values.
//! Implements ADR-014, ADR-015b, and DX-B3 source identity (decision 078).
//! Depends on `dctx-hash` for digest types only. Adapters perform all filesystem IO.

mod confine;
mod defaults;
mod freshness;
mod glob;
mod policy;
mod rule;
mod selection;
mod stamp;

pub use confine::{is_inside, join_lexical, normalize};
pub(crate) use defaults::{builtin_ignore, builtin_omit};
pub use freshness::{
    AuthorizedFile, FileBody, FreshnessMode, RecordedIdentity, SourceFile, StaleReason, Validation,
};
pub use policy::SelectionPolicy;
pub(crate) use rule::Rule;
pub use rule::parse_rule;
pub use selection::{FileHandlingDecision, ItemContext};
pub use stamp::FileStamp;
