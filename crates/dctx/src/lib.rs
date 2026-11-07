// SPDX-License-Identifier: Apache-2.0
//! Stable dctx library facade for pack, map, templates, cache and an optional plugin host.
//! Implements ARCHITECTURE-PROPOSAL §2 and §4 (DX-D1, DX-D3): explicit ports, no home-directory discovery.
//! Depends on dctx-core, dctx-ports, dctx-adapter-cache, dctx-lex-js and dctx-lex-md.
//! Every surface calls this crate. It does not discover a home directory, a global config or a git binary.
//! A caller may inject a plugin host and a cache root. The library default starts none and spawns nothing.
//! Disable `plugin-runtime` to exclude the host/platform/protocol dependencies and use typed refusal stubs.

#![cfg_attr(not(windows), forbid(unsafe_code))]
#![cfg_attr(windows, deny(unsafe_code))]
#![deny(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]

mod adapters;
mod domain;
mod index;
pub mod languages;
mod plugins;
mod ports;
mod session;

/// Facade crate version: the package version, so it moves with the workspace release.
pub const FACADE_VERSION: &str = env!("CARGO_PKG_VERSION");
/// JSON protocol emitted by query and pack.
pub const WIRE_VERSION: u32 = 2;

pub use domain::{
    Answer, AnswerRecord, AutoCleanConfig, BoundedSink, CacheLedger, CacheReport, Capabilities,
    DctxError, Direction, ENTRIES_ENV, ErrorCode, Freshness, FreshnessMode, GcRequest, HostGrants,
    IdentityOutcome, MapRequest, OperationLimits, PackReport, PackRequest, Precision, PurgeRequest,
    PurgeScope, RECORDS_ENV, REQUEST_VERSION, Resource, RuleSources, SOURCE_ENV, STRUCTURAL_PARSER,
    SegmentInfo, SnapshotRequest, VecSink, Verb, encode_answer, encode_answer_with,
    entries_ceiling, object_key, parse_source_ceiling, source_ceiling, validate_segment,
};
pub use languages::{Confidence, LANGUAGES, Language, Tier};
pub use plugins::{Boundary, HookEffect, HookWarning, PluginHost};
pub use ports::{Cache, Cancellation, Clock, FactStore, Source};
pub use session::{Session, SessionBuilder, SnapshotHandle, StoragePolicy};

#[cfg(feature = "fs")]
pub use adapters::FsSource;

pub use dctx_core::selection::RuleTexts;
pub use dctx_core::{FileHandlingDecision, SelectionPolicy, Timestamp};
pub use dctx_ports::memory;
pub use dctx_template::{ContextPlan, GrantRule, HostCeiling, KnownPlugin, bundled_git_plugin};

#[cfg(test)]
mod isolation_scan {
    #[test]
    fn library_sources_do_not_print_or_spawn() {
        let files: &[(&str, &str)] = &[
            ("lib.rs", include_str!("lib.rs")),
            ("domain/mod.rs", include_str!("domain/mod.rs")),
            ("domain/answer.rs", include_str!("domain/answer.rs")),
            (
                "domain/capabilities.rs",
                include_str!("domain/capabilities.rs"),
            ),
            ("domain/error.rs", include_str!("domain/error.rs")),
            ("domain/identity.rs", include_str!("domain/identity.rs")),
            ("domain/limits.rs", include_str!("domain/limits.rs")),
            ("domain/policy.rs", include_str!("domain/policy.rs")),
            ("domain/report.rs", include_str!("domain/report.rs")),
            ("domain/request.rs", include_str!("domain/request.rs")),
            ("domain/sink.rs", include_str!("domain/sink.rs")),
            ("domain/rules.rs", include_str!("domain/rules.rs")),
            ("domain/wire.rs", include_str!("domain/wire.rs")),
            ("index/mod.rs", include_str!("index/mod.rs")),
            ("index/extract.rs", include_str!("index/extract.rs")),
            ("index/query.rs", include_str!("index/query.rs")),
            ("index/segment.rs", include_str!("index/segment.rs")),
            ("index/syn_extract.rs", include_str!("index/syn_extract.rs")),
            ("index/walk.rs", include_str!("index/walk.rs")),
            ("languages.rs", include_str!("languages.rs")),
            ("session/mod.rs", include_str!("session/mod.rs")),
            ("session/build.rs", include_str!("session/build.rs")),
            ("session/cache_op.rs", include_str!("session/cache_op.rs")),
            ("session/ledger.rs", include_str!("session/ledger.rs")),
            (
                "session/ledger/cache_bind.rs",
                include_str!("session/ledger/cache_bind.rs"),
            ),
            ("session/pack_op.rs", include_str!("session/pack_op.rs")),
            ("session/query_op.rs", include_str!("session/query_op.rs")),
            ("session/hooks.rs", include_str!("session/hooks.rs")),
            (
                "session/template_op.rs",
                include_str!("session/template_op.rs"),
            ),
            ("plugins/mod.rs", include_str!("plugins/mod.rs")),
            ("plugins/disabled.rs", include_str!("plugins/disabled.rs")),
            ("plugins/ports.rs", include_str!("plugins/ports.rs")),
            ("plugins/plan.rs", include_str!("plugins/plan.rs")),
            ("plugins/effects.rs", include_str!("plugins/effects.rs")),
            ("plugins/boundary.rs", include_str!("plugins/boundary.rs")),
            ("ports/mod.rs", include_str!("ports/mod.rs")),
            ("adapters/mod.rs", include_str!("adapters/mod.rs")),
            ("adapters/fs.rs", include_str!("adapters/fs.rs")),
        ];
        let needles = [
            ["print", "ln!"].concat(),
            ["eprint", "ln!"].concat(),
            ["dbg", "!"].concat(),
            ["std::", "process"].concat(),
            ["Command", "::new"].concat(),
            ["env", "::var"].concat(),
            ["home", "_dir"].concat(),
            ["Open", "Options"].concat(),
            ["create", "(true)"].concat(),
            ["File", "::create"].concat(),
            ["std::fs", "::write"].concat(),
            ["std::fs", "::remove"].concat(),
            ["std::fs", "::rename"].concat(),
        ];
        for (name, source) in files {
            for needle in &needles {
                assert!(!source.contains(needle), "{name} contains a forbidden call");
            }
        }
    }
}
