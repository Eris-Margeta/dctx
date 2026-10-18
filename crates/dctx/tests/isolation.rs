//! Session construction, capabilities, policy narrowing and template refusals.
//! The facade reports the build, not the host grants. A missing clock is an error.
//! Templates narrow an explicit ceiling. Empty grants do not become a hidden allow.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;

use dctx::memory::{CancelFlag, FixedClock, MemorySource};
#[cfg(feature = "map")]
use dctx::{DctxError, SnapshotRequest, VecSink};
use dctx::{
    ErrorCode, HostCeiling, HostGrants, RuleTexts, SelectionPolicy, Session, StoragePolicy, Timestamp,
};

use common::{open_session, PING};
#[cfg(feature = "map")]
use common::{session_parts, README};

#[cfg(all(feature = "map", feature = "templates"))]
const STRUCTURAL: &str = "\
[DTS]
version = 2
[SELECT:s]
roots = [\"src\"]
[INDEX:s]
parser = \"structural\"
[BUDGET:b]
records = 4
[VIEW:map]
selection = \"s\"
index = \"s\"
budget = \"b\"
";

#[test]
fn missing_clock_is_invalid_and_place_is_the_one_supplied() {
    let error = Session::builder(MemorySource::new()).build().expect_err("clock");
    assert_eq!(error.code(), ErrorCode::InvalidRequest);
    assert!(!error.is_retryable());

    let error = Session::builder(MemorySource::new())
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::CallerOwned { max_bytes: 64 })
        .place("fixture")
        .view("map")
        .build()
        .expect_err("ports");
    assert_eq!(error.code(), ErrorCode::InvalidRequest);

    let ready = open_session(ping_source());
    assert_eq!(ready.place(), "fixture");
    assert_eq!(ready.epoch(), 1);
    let caps = ready.capabilities();
    assert_eq!(caps.facade_version, "0.1.0");
    assert_eq!(caps.profile, "strict");
    assert_eq!(caps.wire_version, 2);
    assert_eq!(caps.dts_version, 2);
    assert!(!caps.git);
    assert!(!caps.native_code);
    assert_eq!(caps.subprocess, "none");
    assert_eq!(caps.unsafe_boundary, "none");
    assert_eq!(caps.cancellation, "checkpoint");
    assert_eq!(caps.features.contains(&"map"), cfg!(feature = "map"));
    assert_eq!(caps.features.contains(&"fs"), cfg!(feature = "fs"));
    assert!(!caps.features.contains(&"syntax-rust"));
    assert!(caps.parsers.iter().any(|parser| *parser == "dctx-structural/1"));
}

#[test]
#[cfg(feature = "map")]
fn two_sessions_with_the_same_bytes_share_a_digest() {
    let mut left = open_session(ping_source());
    let mut right = open_session(ping_source());
    let a = left.snapshot(&SnapshotRequest::immutable()).expect("left");
    let b = right.snapshot(&SnapshotRequest::immutable()).expect("right");
    assert_eq!(a.digest(), b.digest());
    assert_eq!(a.generation(), 1);
    assert_ne!(left.place(), "home");
}

#[test]
#[cfg(feature = "map")]
fn metadata_lists_names_and_a_later_read_is_stale() {
    let mut session = open_session(ping_source());
    let handle = session.snapshot(&SnapshotRequest::metadata()).expect("meta");
    assert_eq!(session.content_reads(), 0);
    let mut sink = VecSink::new();
    let error = session.read(&handle, "m:src/lib.rs", &mut sink).expect_err("stale");
    assert!(matches!(error, DctxError::Stale { .. }));
    assert!(sink.is_empty());
    assert_eq!(session.content_reads(), 0);
    assert_eq!(session.cached_content_reads(), 0);

    let mut live = open_session(ping_source());
    let handle = live.snapshot(&SnapshotRequest::immutable()).expect("live");
    assert_eq!(live.content_reads(), 1);
    let mut sink = VecSink::new();
    let read = live.read(&handle, "s:src/lib.rs#ping", &mut sink).expect("read");
    assert_eq!(read.records[0].source_utf8.as_deref(), Some("pub fn ping() {}"));
    assert_eq!(live.content_reads(), 2);
}

#[test]
#[cfg(feature = "map")]
fn ceiling_exclude_is_not_expanded_by_an_include_selection() {
    let mut denied = session_parts(
        tree(),
        Arc::new(CancelFlag::new()),
        HostGrants::Ceiling,
        StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 },
        SelectionPolicy::from_texts(&RuleTexts::default()),
    );
    let handle = denied.snapshot(&SnapshotRequest::immutable()).expect("empty grant");
    assert_eq!(denied.content_reads(), 0);
    let mut sink = VecSink::new();
    let gone = denied.explain(&handle, "s:src/lib.rs#ping", &mut sink).expect("gone");
    assert_eq!(gone.next.as_deref(), Some("map"));
    assert!(gone.records.is_empty());

    let mut allowed = open_session(tree());
    let handle = allowed.snapshot(&SnapshotRequest::immutable()).expect("permit");
    assert!(allowed.content_reads() >= 1);
    let mut sink = VecSink::new();
    let hit = allowed.explain(&handle, "s:src/lib.rs#ping", &mut sink).expect("ping");
    assert_eq!(hit.records[0].signature.as_deref(), Some("pub fn ping()"));
}

#[test]
#[cfg(all(feature = "map", feature = "templates"))]
fn plan_roots_exclude_readme_and_templates_fail_closed() {
    let mut session = open_session(tree());
    let open = session.snapshot(&SnapshotRequest::immutable()).expect("open");
    assert_eq!(session.content_reads(), 2);
    let mut sink = VecSink::new();
    let heading = session.explain(&open, "d:README.md#Fixture", &mut sink).expect("heading");
    assert_eq!(heading.records[0].summary, "Fixture");
    assert_eq!(heading.records[0].summary_source, "heading");
    assert_eq!(heading.records[0].kind, "heading");

    let plan = session.compile_template(STRUCTURAL).expect("structural");
    let bytes = dctx_template::to_bytes(&plan).expect("encode");
    let loaded = session.load_template(&bytes).expect("load");
    assert_eq!(loaded, plan);
    session.install_plan(plan);
    assert_eq!(session.epoch(), 2);
    let narrowed = session.snapshot(&SnapshotRequest::immutable()).expect("roots");
    assert_eq!(narrowed.epoch(), 2);
    assert_eq!(session.content_reads(), 3);
    let mut sink = VecSink::new();
    let gone = session.explain(&narrowed, "d:README.md#Fixture", &mut sink).expect("outside");
    assert!(gone.records.is_empty());
    let mut sink = VecSink::new();
    let ping = session.explain(&narrowed, "s:src/lib.rs#ping", &mut sink).expect("inside");
    assert_eq!(ping.records[0].path, "src/lib.rs");

    let version = session.compile_template("[DTS]\nversion = 9\n").expect_err("version");
    assert_eq!(version, DctxError::UnsupportedVersion { found: 9 });

    let tree = STRUCTURAL.replace("parser = \"structural\"", "parser = \"tree-sitter\"");
    let refused = session.compile_template(&tree).expect_err("tree-sitter");
    match refused {
        DctxError::UnsupportedCapability { capability, detail } => {
            assert_eq!(capability, "template");
            assert!(detail.contains("tree-sitter"));
        }
        other => panic!("template capability, got {other}"),
    }

    let syn = STRUCTURAL.replace("parser = \"structural\"", "parser = \"syn\"");
    let compiled = session.compile_template(&syn).expect("syn plan");
    assert_eq!(compiled.source_version, 2);
    session.install_plan(compiled);
    if cfg!(feature = "syntax-rust") {
        let handle = session.snapshot(&SnapshotRequest::immutable()).expect("syn");
        assert!(handle.generation() > 0);
    } else {
        let error = session.snapshot(&SnapshotRequest::immutable()).expect_err("no syn");
        assert!(matches!(error, DctxError::UnsupportedCapability { capability: "syntax-rust", .. }));
        assert_eq!(session.generation_count(), 2);
    }
}

#[test]
fn object_key_uses_the_first_party_digest() {
    let empty = dctx::object_key(b"").expect("empty");
    assert_eq!(
        empty,
        "dctx:object:v1:sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855:0"
    );
    let abc = dctx::object_key(b"abc").expect("abc");
    assert_eq!(
        abc,
        "dctx:object:v1:sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad:3"
    );
}

fn ping_source() -> MemorySource {
    let mut source = MemorySource::new();
    source.insert_file("src/lib.rs", PING, 0).expect("lib");
    source
}

#[cfg(feature = "map")]
fn tree() -> MemorySource {
    let mut source = ping_source();
    source.insert_file("README.md", README, 0).expect("readme");
    source
}

#[cfg(not(feature = "plugin-runtime"))]
#[test]
fn disabled_host_refuses_ports_and_contains_no_runtime_symbols() {
    use dctx::{Boundary, DctxError, HookEffect, PluginHost};
    for host in [PluginHost::fake(), PluginHost::platform()] {
        let host = host.on("dctx/git", Boundary::BeforePack, vec![HookEffect::StageGitignore]);
        assert_eq!((host.spawns(), host.admissions(), host.git_children()), (0, 0, 0));
        assert!(matches!(host.seed(".gitignore", b"#\n"),
            Err(DctxError::UnsupportedCapability { capability: "plugin-runtime", .. })));
    }
    #[cfg(feature = "fs")]
    assert!(matches!(PluginHost::platform_with_audit(std::path::Path::new("missing"), 1024),
        Err(DctxError::UnsupportedCapability { capability: "plugin-runtime", .. })));
    let binary = std::fs::read(std::env::current_exe().expect("executable")).expect("binary");
    for suffix in ["host", "platform", "protocol"] {
        let symbol = format!("dctx_plugin_{suffix}");
        assert!(!binary.windows(symbol.len()).any(|bytes| bytes == symbol.as_bytes()), "{symbol} linked");
    }
}

#[cfg(all(not(feature = "plugin-runtime"), feature = "pack", feature = "map", feature = "templates", feature = "cache"))]
#[test]
fn disabled_hooks_warn_or_refuse_without_spawning_and_engines_still_work() {
    use dctx::{Boundary, GcRequest, HookEffect, PackRequest, PluginHost, bundled_git_plugin};
    for host in [None, Some(PluginHost::fake()), Some(PluginHost::platform())] {
        for (hook, required) in [("", false), ("before-pack-output-written", true),
            ("after-pack-output-written", true), ("before-index", true), ("after-index", true), ("cache-gc", true), ("all", false)] {
            let mut ceiling = HostCeiling::base();
            let mut plugin = bundled_git_plugin();
            plugin.hooks = ["before-pack-output-written", "after-pack-output-written", "before-index", "after-index", "cache-gc"]
                .map(str::to_string).to_vec();
            ceiling.plugins.push(plugin);
            let mut builder = Session::builder(ping_source()).clock(FixedClock(Timestamp(0)))
                .cancellation(Arc::new(CancelFlag::new())).selection(SelectionPolicy::from_texts(&RuleTexts::default()))
                .ceiling(ceiling).grants(HostGrants::PermitAll).storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 })
                .place("fixture").view("map");
            if let Some(host) = host.clone() {
                builder = builder.plugins(host.on("dctx/git", Boundary::BeforePack, vec![HookEffect::StageGitignore]));
            }
            let mut session = builder.build().expect("session");
            let mut source = STRUCTURAL.to_string();
            if !hook.is_empty() {
                let hooks = if hook == "all" { "before-pack-output-written\", \"after-pack-output-written\", \"before-index\", \"after-index\", \"cache-gc" } else { hook };
                source.push_str(&format!("[PLUGIN:dctx/git]\nversion = \"1.0.0\"\ndigest = \"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\"\nhooks = [\"{hooks}\"]\nrequired = {required}\n"));
            }
            let plan = session.compile_template(&source).expect("plan");
            session.install_plan(plan);
            let mut sink = VecSink::new();
            let result = if hook.contains("index") {
                session.snapshot(&SnapshotRequest::immutable()).map(|_| ())
            } else if hook == "cache-gc" {
                session.cache_gc(GcRequest { apply: true }).map(|_| ())
            } else {
                session.pack(&PackRequest::new(), &mut sink).map(|_| ())
            };
            if required {
                let committed = hook.starts_with("after") || hook == "cache-gc";
                assert_eq!(result, Err(DctxError::Hook { hook: hook_name(hook), reason: "plugin-runtime-disabled", output_committed: committed }));
                if hook.starts_with("before") { assert!(sink.is_empty()); assert_eq!(session.content_reads(), 0); }
            } else {
                result.expect("pack without runtime");
                assert!(!sink.is_empty());
                let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
                let answer = session.map(&handle, "m:src/lib.rs", &mut sink).expect("map");
                assert!(!answer.records.is_empty());
                if hook == "all" { assert_eq!(session.hook_warnings().len(), 2); }
                assert!(session.hook_warnings().iter().all(|w| w.reason == "plugin-runtime-disabled"));
            }
            assert_eq!((session.plugin_spawns(), session.plugin_admissions(), session.git_children()), (0, 0, 0));
        }
    }
    fn hook_name(hook: &str) -> &'static str {
        [Boundary::BeforePack, Boundary::AfterPack, Boundary::BeforeIndex, Boundary::AfterIndex, Boundary::CacheGc]
            .into_iter().find(|boundary| boundary.as_str() == hook).expect("boundary").as_str()
    }
}
