//! Plugin host integration: real and fake ports, commit flags and ignore revalidation.
//! The library default starts no host. Map queries do not schedule a hook.
//! Engines reach the host only through session methods. A platform launch stays closed.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;

use dctx::memory::{CancelFlag, FixedClock, MemorySource};
use dctx::{
    bundled_git_plugin, Boundary, ErrorCode, GcRequest, HookEffect, HostCeiling, HostGrants, OperationLimits,
    PackRequest, PluginHost, RuleSources, RuleTexts, SelectionPolicy, Session, SnapshotRequest, StoragePolicy,
    Timestamp, VecSink,
};

use common::{open_session, repository};

const ALL: &[&str] = &[
    "before-pack-output-written",
    "after-pack-output-written",
    "before-index",
    "after-index",
    "cache-gc",
];
const SRC: &str = "\"src\"";
const SRC_IGNORE: &str = "\"src\", \".gitignore\"";

fn ceiling(hooks: &[&str]) -> HostCeiling {
    let mut ceiling = HostCeiling::base();
    let mut plugin = bundled_git_plugin();
    plugin.hooks = hooks.iter().map(|hook| (*hook).to_string()).collect();
    ceiling.plugins.push(plugin);
    ceiling
}

fn source(hooks: &[&str], required: bool, roots: &str) -> String {
    let listed = hooks.iter().map(|hook| format!("\"{hook}\"")).collect::<Vec<_>>().join(", ");
    let flag = if required { "true" } else { "false" };
    format!(
        "\
[DTS]
version = 2
[SELECT:s]
roots = [{roots}]
[INDEX:s]
parser = \"structural\"
[BUDGET:b]
records = 4
[VIEW:map]
selection = \"s\"
index = \"s\"
budget = \"b\"
[PLUGIN:dctx/git]
version = \"1.0.0\"
digest = \"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\"
hooks = [{listed}]
required = {flag}
allow = [\"file.read:.gitignore\", \"file.write:.gitignore\", \"program:git.add-ignore\", \"resource:git-admin:stage-ignore\"]
"
    )
}

fn open(
    tree: MemorySource,
    host: PluginHost,
    rules: Option<RuleSources>,
    hooks: &[&str],
    required: bool,
    roots: &str,
) -> Session {
    let selection = rules.as_ref().map(RuleSources::policy).unwrap_or_else(|| SelectionPolicy::from_texts(&RuleTexts::default()));
    let mut builder = Session::builder(tree)
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(selection)
        .ceiling(ceiling(hooks))
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 })
        .limits(OperationLimits::default())
        .place("fixture")
        .view("map")
        .plugins(host);
    if let Some(rules) = rules {
        builder = builder.rule_sources(rules);
    }
    let mut session = builder.build().expect("session");
    let plan = session.compile_template(&source(hooks, required, roots)).expect("plan");
    session.install_plan(plan);
    session
}

fn duo() -> MemorySource {
    let mut tree = MemorySource::new();
    tree.insert_file("src/lib.rs", b"pub fn ping() {}\n", 0).expect("lib");
    tree.insert_file("src/secret.rs", b"pub fn secret() {}\n", 0).expect("secret");
    tree.insert_file(".gitignore", b"#\n", 0).expect("ignore");
    tree
}

fn commented() -> RuleSources {
    RuleSources { local_ignore: Some("#\n".to_string()), ..RuleSources::default() }
}

#[test]
fn engines_do_not_import_host_internals() {
    let files = [
        include_str!("../src/index/mod.rs"),
        include_str!("../src/index/extract.rs"),
        include_str!("../src/index/query.rs"),
        include_str!("../src/index/segment.rs"),
        include_str!("../src/index/syn_extract.rs"),
        include_str!("../src/index/walk.rs"),
        include_str!("../src/session/mod.rs"),
        include_str!("../src/session/pack_op.rs"),
        include_str!("../src/session/query_op.rs"),
        include_str!("../src/session/cache_op.rs"),
    ];
    for source in files {
        assert!(!source.contains("dctx_plugin_host"));
        assert!(!source.contains("dctx_plugin_platform"));
        assert!(!source.contains("dctx_plugin_protocol"));
    }
}

#[test]
fn library_isolation_spawns_nothing() {
    let mut session = open_session(repository());
    let mut sink = VecSink::new();
    session.pack(&PackRequest::new(), &mut sink).expect("pack");
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    session.map(&handle, "m:src/lib.rs", &mut sink).expect("map");
    session.find(&handle, "ping", &mut sink).expect("find");
    session.explain(&handle, "s:src/lib.rs#ping", &mut sink).expect("explain");
    session.read(&handle, "s:src/lib.rs#ping", &mut sink).expect("read");
    session.cache_gc(GcRequest { apply: false }).expect("dry");
    drop(handle);
    session.cache_gc(GcRequest { apply: true }).expect("apply");
    assert_eq!(session.plugin_spawns(), 0);
    assert_eq!(session.git_children(), 0);
    assert_eq!(session.plugin_admissions(), 0);
}

#[test]
fn read_only_map_does_not_spawn() {
    let mut session = open(repository(), PluginHost::fake(), None, ALL, false, SRC);
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let spawns = session.plugin_spawns();
    let admissions = session.plugin_admissions();
    let git = session.git_children();
    assert!(spawns >= 2);
    let mut sink = VecSink::new();
    session.map(&handle, "m:src/lib.rs", &mut sink).expect("map");
    session.find(&handle, "ping", &mut sink).expect("find");
    session.neighbors(&handle, "m:src/lib.rs", &mut sink).expect("neighbors");
    session.explain(&handle, "s:src/lib.rs#ping", &mut sink).expect("explain");
    session.read(&handle, "s:src/lib.rs#ping", &mut sink).expect("read");
    assert_eq!(session.plugin_spawns(), spawns);
    assert_eq!(session.plugin_admissions(), admissions);
    assert_eq!(session.git_children(), git);
    assert_eq!(git, 0);
}

#[test]
fn corpus_fake_runs_five_hooks_without_git_children() {
    let mut session = open(repository(), PluginHost::fake(), None, ALL, false, SRC);
    let mut sink = VecSink::new();
    session.pack(&PackRequest::new(), &mut sink).expect("pack");
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    drop(handle);
    session.cache_gc(GcRequest { apply: true }).expect("gc");
    assert_eq!(session.plugin_admissions(), 5);
    assert_eq!(session.plugin_spawns(), 5);
    assert_eq!(session.git_children(), 0);
    assert!(session.hook_warnings().is_empty());
    session.cache_gc(GcRequest { apply: false }).expect("dry");
    assert_eq!(session.plugin_admissions(), 5);
    assert_eq!(session.plugin_spawns(), 5);
}

#[test]
fn corpus_platform_warns_and_spawns_nothing() {
    let mut session = open(repository(), PluginHost::platform(), None, ALL, false, SRC);
    let mut sink = VecSink::new();
    let mut seen = Vec::new();
    session.pack(&PackRequest::new(), &mut sink).expect("pack");
    seen.extend(session.hook_warnings().iter().cloned());
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    seen.extend(session.hook_warnings().iter().cloned());
    drop(handle);
    session.cache_gc(GcRequest { apply: true }).expect("gc");
    seen.extend(session.hook_warnings().iter().cloned());
    assert_eq!(session.plugin_spawns(), 0);
    assert_eq!(session.git_children(), 0);
    assert_eq!(session.plugin_admissions(), 5);
    let flags: Vec<bool> = seen.iter().map(|warning| warning.output_committed).collect();
    assert_eq!(flags, vec![false, true, false, true, true]);
    assert_eq!(seen.len(), 5);
}

#[test]
fn before_commit_required_is_not_committed() {
    let host = PluginHost::fake().on("dctx/git", Boundary::BeforePack, vec![HookEffect::Fail]);
    let mut session = open(repository(), host, None, &["before-pack-output-written"], true, SRC);
    let mut sink = VecSink::new();
    let error = session.pack(&PackRequest::new(), &mut sink).expect_err("blocked");
    assert_eq!(error.code(), ErrorCode::Hook);
    assert!(!error.output_committed());
    assert!(sink.is_empty());
    assert_eq!(session.cache_status().committed_bytes, 0);
}

#[test]
fn after_commit_required_stays_committed() {
    let failing = PluginHost::fake().on("dctx/git", Boundary::AfterPack, vec![HookEffect::Fail]);
    let mut session = open(repository(), failing, None, &["after-pack-output-written"], true, SRC);
    let mut sink = VecSink::new();
    let error = session.pack(&PackRequest::new(), &mut sink).expect_err("reported");
    assert_eq!(error.code(), ErrorCode::Hook);
    assert!(error.output_committed());
    assert!(!sink.is_empty());
    assert!(session.cache_status().committed_bytes > 0);

    let clean = PluginHost::fake();
    let mut session = open(repository(), clean, None, &["after-pack-output-written"], true, SRC);
    let mut sink = VecSink::new();
    session.pack(&PackRequest::new(), &mut sink).expect("published");
    assert!(!sink.is_empty());
}

#[test]
fn before_index_required_does_not_read() {
    let host = PluginHost::fake().on("dctx/git", Boundary::BeforeIndex, vec![HookEffect::Fail]);
    let mut session = open(repository(), host, None, &["before-index"], true, SRC);
    let error = session.snapshot(&SnapshotRequest::immutable()).expect_err("veto");
    assert_eq!(error.code(), ErrorCode::Hook);
    assert!(!error.output_committed());
    assert_eq!(session.content_reads(), 0);
    assert_eq!(session.generation_count(), 0);

    let host = PluginHost::fake().on("dctx/git", Boundary::AfterIndex, vec![HookEffect::Fail]);
    let mut session = open(repository(), host, None, &["after-index"], true, SRC);
    let error = session.snapshot(&SnapshotRequest::immutable()).expect_err("published");
    assert!(error.output_committed());
    assert!(session.content_reads() > 0);
    assert_eq!(session.generation_count(), 1);
}

#[test]
fn cache_gc_after_failure_is_committed() {
    let host = PluginHost::fake().on("dctx/git", Boundary::CacheGc, vec![HookEffect::Fail]);
    let mut session = open(repository(), host, None, &["cache-gc"], true, SRC);
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    drop(handle);
    let admissions = session.plugin_admissions();
    session.cache_gc(GcRequest { apply: false }).expect("dry");
    assert_eq!(session.plugin_admissions(), admissions);
    assert_eq!(session.plugin_spawns(), admissions);
    let error = session.cache_gc(GcRequest { apply: true }).expect_err("hook");
    assert_eq!(error.code(), ErrorCode::Hook);
    assert!(error.output_committed());
    assert_eq!(session.generation_count(), 0);
}

#[test]
fn gitignore_revalidate_changes_pack_selection() {
    let quiet = PluginHost::fake().seed(".gitignore", b"#\n").expect("seed");
    let noisy = PluginHost::fake()
        .seed(".gitignore", b"#\n")
        .expect("seed")
        .on("dctx/git", Boundary::BeforePack, vec![HookEffect::ReplaceGitignore(b"secret.rs\n".to_vec())]);
    let hooks = &["before-pack-output-written"];
    let mut untouched = open(duo(), quiet, Some(commented()), hooks, false, SRC_IGNORE);
    let mut revised = open(duo(), noisy, Some(commented()), hooks, false, SRC_IGNORE);
    let mut sink = VecSink::new();
    let kept = untouched.pack(&PackRequest::new(), &mut sink).expect("kept");
    let hidden = revised.pack(&PackRequest::new(), &mut sink).expect("hidden");
    assert!(hidden.files < kept.files);
    assert_eq!(revised.epoch(), untouched.epoch() + 1);
}

#[test]
fn gitignore_change_without_rule_sources_refuses() {
    let host = PluginHost::fake()
        .seed(".gitignore", b"#\n")
        .expect("seed")
        .on("dctx/git", Boundary::BeforePack, vec![HookEffect::ReplaceGitignore(b"secret.rs\n".to_vec())]);
    let mut session = open(duo(), host, None, &["before-pack-output-written"], false, SRC_IGNORE);
    let mut sink = VecSink::new();
    let error = session.pack(&PackRequest::new(), &mut sink).expect_err("unsafe");
    assert_eq!(error.code(), ErrorCode::PolicyUnavailable);
    assert!(!error.output_committed());
    assert!(sink.is_empty());
    assert_eq!(session.cache_status().committed_bytes, 0);
}

#[test]
fn stage_git_fake_counts_a_child_and_platform_does_not() {
    let fake = PluginHost::fake()
        .seed(".gitignore", b"#\n")
        .expect("seed")
        .on("dctx/git", Boundary::BeforePack, vec![HookEffect::StageGitignore]);
    let mut session = open(duo(), fake, None, &["before-pack-output-written"], false, SRC_IGNORE);
    let mut sink = VecSink::new();
    session.pack(&PackRequest::new(), &mut sink).expect("staged");
    assert_eq!(session.git_children(), 1);
    assert_eq!(session.plugin_spawns(), 1);

    let platform = PluginHost::platform().on("dctx/git", Boundary::BeforePack, vec![HookEffect::StageGitignore]);
    let mut session = open(duo(), platform, None, &["before-pack-output-written"], false, SRC_IGNORE);
    let mut sink = VecSink::new();
    session.pack(&PackRequest::new(), &mut sink).expect("warned");
    assert_eq!(session.git_children(), 0);
    assert_eq!(session.plugin_spawns(), 0);
    assert_eq!(session.hook_warnings().len(), 1);
    assert!(!session.hook_warnings()[0].output_committed);
}

#[test]
fn real_required_before_blocks_without_children() {
    let host = PluginHost::platform().on("dctx/git", Boundary::BeforePack, vec![HookEffect::Fail]);
    let mut session = open(repository(), host, None, &["before-pack-output-written"], true, SRC);
    let mut sink = VecSink::new();
    let error = session.pack(&PackRequest::new(), &mut sink).expect_err("closed");
    assert_eq!(error.code(), ErrorCode::Hook);
    assert!(!error.output_committed());
    assert!(sink.is_empty());
    assert_eq!(session.plugin_spawns(), 0);
    assert_eq!(session.git_children(), 0);
    assert_eq!(session.cache_status().committed_bytes, 0);
}
