// SPDX-License-Identifier: Apache-2.0
//! Facade PP1/PP4 controls: independent current authority and bound broker requests.
//! Exercises admission-to-commit revocation through the existing in-memory host ports.
//! No process or filesystem effects; every refusal has the unchanged authorized control.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use std::sync::{Arc, Mutex};

use dctx::memory::{CancelFlag, FixedClock};
use dctx::{
    Boundary, DctxError, HookEffect, HostCeiling, HostGrants, OperationLimits, PackRequest, PluginHost,
    RuleSources, Session, StoragePolicy, Timestamp, VecSink, bundled_git_plugin,
};
use dctx_plugin_host::{Binding, Decision, GrantLayers, Selection};
use dctx_plugin_protocol::records::capability::Capability;
use dctx_plugin_protocol::records::scalars::{Digest, PluginId, ProjectPath, Token};

const DIGEST: &str = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const TEMPLATE: &str = r#"
[DTS]
version = 2
[SELECT:s]
roots = ["src", ".gitignore"]
[INDEX:s]
parser = "structural"
[BUDGET:b]
records = 4
[VIEW:map]
selection = "s"
index = "s"
budget = "b"
[PLUGIN:dctx/git]
version = "1.1.0"
digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
hooks = ["before-pack-output-written"]
required = true
allow = ["file.read:.gitignore", "file.write:.gitignore", "program:git.add-ignore", "resource:git-admin:stage-ignore"]
"#;

#[derive(Clone, Copy, Debug)]
enum Change { None, Manifest, Consent, Organization, Host, Selection, Adapter, Root, Session, Namespace, Artifact, Epoch, Plugin, Invocation }

fn token(text: &str) -> Token { Token::new(text).expect("token") }
fn path() -> ProjectPath { ProjectPath::new(".gitignore").expect("path") }

fn grants() -> GrantLayers {
    let caps = vec![
        Capability::FileRead { path: path(), max_bytes: 1024 },
        Capability::FileWrite { path: path(), create: false, max_growth_bytes: 1024, max_bytes: 2048 },
        Capability::GitAddIgnore { max_output_bytes: 1024 },
    ];
    GrantLayers { manifest: caps.clone(), consent: caps.clone(), organization: caps.clone(),
        host: caps.clone(), adapter: caps, denies: vec![] }
}

fn change(which: Change, binding: &mut Binding, layers: &mut GrantLayers, selection: &mut Selection) {
    match which {
        Change::None => {}
        Change::Manifest => layers.manifest.clear(),
        Change::Consent => layers.consent.clear(),
        Change::Organization => layers.organization.clear(),
        Change::Host => layers.host.clear(),
        Change::Selection => *selection = Selection::default(),
        Change::Adapter => layers.adapter.clear(),
        Change::Root => binding.root = token("other:root"),
        Change::Session => binding.session = token("other:session"),
        Change::Namespace => binding.namespace = token("other:namespace"),
        Change::Artifact => binding.artifact = Digest::new(&format!("sha256:{}", "b".repeat(64))).expect("digest"),
        Change::Epoch => binding.generation += 1,
        Change::Plugin => binding.plugin = PluginId::new("other/plugin").expect("plugin"),
        Change::Invocation => binding.invocation = token("other:invocation"),
    }
}

fn host(effect: HookEffect, which: Change, at: usize) -> (PluginHost, Arc<Mutex<usize>>) {
    let calls = Arc::new(Mutex::new(0));
    let observed = Arc::clone(&calls);
    let host = PluginHost::fake().seed(".gitignore", b"#\n").expect("seed")
        .on("dctx/git", Boundary::BeforePack, vec![effect])
        .with_authority_source(move |plugin, invocation, epoch| {
            let mut calls = observed.lock().expect("calls");
            *calls += 1;
            let mut binding = Binding {
                plugin: PluginId::new(plugin).expect("plugin"), artifact: Digest::new(DIGEST).expect("digest"),
                root: token("trusted:root"), session: token("trusted:session"), namespace: token("trusted:namespace"),
                generation: 40 + epoch, invocation: invocation.clone(),
            };
            let mut layers = grants();
            let mut selected = Selection::new([(path(), Decision::Include)]);
            if *calls >= at { change(which, &mut binding, &mut layers, &mut selected); }
            Ok((binding, layers, selected))
        });
    (host, calls)
}

fn open(host: PluginHost) -> Session {
    let mut ceiling = HostCeiling::base();
    ceiling.plugins.push(bundled_git_plugin());
    let rules = RuleSources { local_ignore: Some("#\n".to_string()), ..RuleSources::default() };
    let mut session = Session::builder(common::repository())
        .clock(FixedClock(Timestamp(0))).cancellation(Arc::new(CancelFlag::new()))
        .selection(rules.policy()).rule_sources(rules).ceiling(ceiling).grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 }).limits(OperationLimits::default())
        .place("fixture").view("map").plugins(host).build().expect("session");
    let plan = session.compile_template(TEMPLATE).expect("plan");
    session.install_plan(plan);
    session
}

fn run(effect: HookEffect, which: Change, at: usize, reason: Option<&str>) {
    let writes = matches!(effect, HookEffect::ReplaceGitignore(_));
    let stages = matches!(effect, HookEffect::StageGitignore);
    let (host, calls) = host(effect, which, at);
    let mut session = open(host);
    let epoch = session.epoch();
    let mut sink = VecSink::new();
    let result = session.pack(&PackRequest::new(), &mut sink);
    if let Some(expected) = reason {
        match result.expect_err("must refuse") {
            DctxError::Hook { reason, output_committed, .. } => {
                assert_eq!(reason, expected, "{which:?}");
                assert!(!output_committed);
            }
            other => panic!("unexpected refusal: {other:?}"),
        }
        assert!(sink.is_empty());
        assert_eq!(session.epoch(), epoch, "no replacement or ignore reload");
        assert_eq!(session.git_children(), 0);
        assert_eq!(session.cache_status().committed_bytes, 0);
        assert_eq!(*calls.lock().expect("calls"), at);
    } else {
        result.expect("authorized control");
        assert!(!sink.is_empty());
        assert_eq!(session.epoch(), epoch + u64::from(writes));
        assert_eq!(session.git_children(), u64::from(stages));
        assert_eq!(*calls.lock().expect("calls"), if writes { 7 } else if stages { 5 } else { 3 });
    }
}

#[test]
fn every_authority_input_is_current_at_admission_and_commit() {
    for which in [Change::Manifest, Change::Consent, Change::Organization, Change::Host, Change::Selection, Change::Adapter] {
        for (effect, commit) in [(HookEffect::ReplaceGitignore(b"secret.rs\n".to_vec()), 7), (HookEffect::StageGitignore, 5)] {
            for at in [2, commit - 1, commit] {
                let reason = if matches!(which, Change::Selection) { "source-not-included" }
                    else if at == commit { "grant-revoked" } else { "not-granted" };
                run(effect.clone(), which, at, Some(reason));
                run(effect.clone(), Change::None, at, None);
            }
        }
    }
}

#[test]
fn handles_and_pending_requests_refuse_every_foreign_binding_field() {
    for which in [Change::Root, Change::Session, Change::Namespace, Change::Artifact, Change::Epoch, Change::Plugin, Change::Invocation] {
        for at in [2, 3, 6, 7] {
            let reason = if matches!(which, Change::Artifact) { "artifact-mismatch" } else { "binding-mismatch" };
            run(HookEffect::ReplaceGitignore(b"secret.rs\n".to_vec()), which, at, Some(reason));
            run(HookEffect::ReplaceGitignore(b"secret.rs\n".to_vec()), Change::None, at, None);
        }
    }
}

#[test]
fn injected_contexts_succeed_and_wrong_artifacts_fail_before_launch() {
    for which in [Change::Root, Change::Session, Change::Namespace, Change::Epoch] {
        run(HookEffect::ReplaceGitignore(b"secret.rs\n".to_vec()), which, 1, None);
    }
    for which in [Change::Artifact, Change::Plugin, Change::Invocation] {
        let (host, _) = host(HookEffect::StageGitignore, which, 1);
        let mut session = open(host);
        let mut sink = VecSink::new();
        assert!(matches!(session.pack(&PackRequest::new(), &mut sink), Err(DctxError::Hook { .. })));
        assert_eq!(session.plugin_admissions(), 0);
        assert_eq!(session.plugin_spawns(), 0);
        assert_eq!(session.git_children(), 0);
        assert!(sink.is_empty());
        run(HookEffect::StageGitignore, Change::None, 1, None);
    }
}

#[test]
fn template_configuration_only_narrows_the_trusted_manifest() {
    for allow_write in [false, true] {
        let (host, _) = host(HookEffect::ReplaceGitignore(b"secret.rs\n".to_vec()), Change::None, 1);
        let mut session = open(host);
        let source = if allow_write { TEMPLATE.to_string() } else { TEMPLATE.replace("\"file.write:.gitignore\", ", "") };
        let plan = session.compile_template(&source).expect("narrowed plan");
        session.install_plan(plan);
        let epoch = session.epoch();
        let mut sink = VecSink::new();
        let result = session.pack(&PackRequest::new(), &mut sink);
        if allow_write {
            result.expect("unchanged manifest control");
            assert_eq!(session.epoch(), epoch + 1);
        } else {
            assert!(matches!(result, Err(DctxError::Hook { reason: "not-granted", .. })));
            assert!(sink.is_empty());
            assert_eq!(session.epoch(), epoch);
        }
        assert_eq!(session.git_children(), 0);
    }
}

#[test]
fn templates_and_equal_capability_updates_cannot_supply_consent() {
    for which in [Change::Consent, Change::None] {
        let (host, _) = host(HookEffect::StageGitignore, which, 1);
        let mut session = open(host);
        for config in ["", "\nconfig.stage = true\n"] {
            let source = format!("{TEMPLATE}{config}");
            let plan = session.compile_template(&source).expect("updated template");
            session.install_plan(plan);
            let mut sink = VecSink::new();
            let result = session.pack(&PackRequest::new(), &mut sink);
            if matches!(which, Change::Consent) {
                assert!(matches!(result, Err(DctxError::Hook { reason: "not-granted", .. })));
                assert!(sink.is_empty());
                assert_eq!(session.git_children(), 0);
            } else {
                result.expect("explicit consent control");
                assert!(!sink.is_empty());
                assert!(session.git_children() > 0);
            }
        }
    }
}
