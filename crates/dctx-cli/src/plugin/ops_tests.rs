// SPDX-License-Identifier: Apache-2.0
//! Operation regression tests for PLUGINS.md §5.1.
//! Depends on signed release fixtures and fake distribution ports.
//! Exercises the unchanged operations dispatcher without starting workers.

use super::*;
use crate::plugin::parse::parse_argv;
use crate::plugin::IndexCache;
use dctx::PluginHost;
use dctx_plugin_distribution::domain::signature::{INDEX_DOMAIN, MANIFEST_DOMAIN};
use dctx_plugin_distribution::fakes::{FakeNetwork, FixedClock, MemoryStore};
use dctx_plugin_distribution::{
    fingerprint, InstallPolicy, KeyId, OfflineBundle, PluginId, Sha256Hex, Store, Target,
    TrustStore,
};
use ed25519_dalek::{Signer, SigningKey};

const NOW: u64 = 1_000_000;
const BASE: &str = "https://releases.example.org/dctx-git";
const INDEX: &str = "https://releases.example.org/index.json";

struct World {
    net: FakeNetwork,
    store: MemoryStore,
    clock: FixedClock,
    trust: TrustStore,
    policy: InstallPolicy,
    key: SigningKey,
    index_key: [u8; 32],
}

impl World {
    fn new() -> Self {
        let key = SigningKey::from_bytes(&[7; 32]);
        let index_key = key.verifying_key().to_bytes();
        let mut trust = TrustStore::new();
        let signer = trust
            .pin_official(KeyId::parse("official-1").unwrap(), index_key)
            .unwrap();
        trust.pin_publisher(PluginId::parse("dctx/git").unwrap(), signer);
        trust.record_revocations([], [], NOW);
        Self {
            net: FakeNetwork::new(),
            store: MemoryStore::new(),
            clock: FixedClock::at(NOW),
            trust,
            policy: InstallPolicy::new(Target::AppleArm64),
            key,
            index_key,
        }
    }

    fn sign(&self, domain: &[u8], document: &[u8]) -> Vec<u8> {
        let mut message = domain.to_vec();
        message.extend_from_slice(document);
        self.key.sign(&message).to_bytes().to_vec()
    }

    fn release(&self, version: &str, sequence: u64) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let executable = b"#!/bin/sh\necho fixture\n".to_vec();
        let digest = Sha256Hex::of(&executable).unwrap();
        let manifest = format!(
                concat!(
                    "{{\"manifest_version\":1,\"id\":\"dctx/git\",\"name\":\"dctx-plugin-git\",\"version\":\"{version}\",",
                    "\"release_sequence\":{sequence},\"publisher_key\":\"official-1\",\"license\":\"Apache-2.0\",",
                    "\"protocol\":[{{\"major\":1,\"min_minor\":0,\"max_minor\":0}}],\"host_api\":{{\"major\":1,\"min_minor\":0}},",
                    "\"hooks\":[],\"commands\":[],\"capabilities\":{caps},\"config\":{{}},\"limits\":{{}},",
                    "\"reason_codes\":[],\"diagnostic_codes\":[],\"artifacts\":[{{\"target\":\"aarch64-apple-darwin\",",
                    "\"file\":\"dctx-plugin-git-{version}\",\"bytes\":{len},\"sha256\":\"{digest}\",",
                    "\"format\":\"native-executable\",\"min_os\":\"14.0\",\"runtime_resources\":[]}}]}}"
                ),
                version = version,
                sequence = sequence,
                caps = bundled::CAPABILITIES_JSON,
                len = executable.len(),
                digest = digest,
            )
            .into_bytes();
        let signature = self.sign(MANIFEST_DOMAIN, &manifest);
        (manifest, signature, executable)
    }

    fn publish(&self, version: &str, sequence: u64) {
        let (manifest, signature, executable) = self.release(version, sequence);
        let dir = format!("{BASE}/{version}");
        self.net.serve(&format!("{dir}/plugin.json"), &manifest);
        self.net
            .serve(&format!("{dir}/plugin.json.sig"), &signature);
        self.net
            .serve(&format!("{dir}/dctx-plugin-git-{version}"), &executable);
    }

    fn publish_index(&self, summary: &str, version: &str, sequence: u64) {
        let (manifest, _, _) = self.release(version, sequence);
        let entry = format!(
                "{{\"id\":\"dctx/git\",\"repository\":\"fixture/dctx-plugin-git\",\"summary\":{summary},\"publisher_key_fingerprint\":\"{fp}\",\"releases\":[{{\"version\":\"{version}\",\"release_sequence\":{sequence},\"manifest_url\":\"{BASE}/{version}/plugin.json\",\"manifest_sha256\":\"{sha}\"}}]}}",
                summary = render::json_string(summary),
                fp = fingerprint(&self.index_key),
                sha = Sha256Hex::of(&manifest).unwrap(),
            );
        let bytes =
            format!("{{\"schema\":1,\"sequence\":{sequence},\"plugins\":[{entry}]}}").into_bytes();
        let signature = self.sign(INDEX_DOMAIN, &bytes);
        self.net.serve(INDEX, &bytes);
        self.net
            .serve("https://releases.example.org/index.json.sig", &signature);
    }

    fn bundle(&self, version: &str, sequence: u64) -> Vec<u8> {
        let (manifest, signature, executable) = self.release(version, sequence);
        let file = format!("dctx-plugin-git-{version}");
        OfflineBundle::encode(&[
            ("plugin.json", manifest.as_slice()),
            ("plugin.json.sig", signature.as_slice()),
            (file.as_str(), executable.as_slice()),
        ])
    }
}

fn run(world: &World, index: &mut IndexCache, args: &[&str]) -> Outcome {
    let host = PluginHost::fake();
    let mut env = Env {
        network: &world.net,
        store: &world.store,
        clock: &world.clock,
        trust: &world.trust,
        policy: &world.policy,
        index,
        index_url: INDEX,
        index_key: Some(world.index_key),
        host: &host,
    };
    let owned: Vec<String> = std::iter::once("dctx")
        .chain(args.iter().copied())
        .map(str::to_owned)
        .collect();
    let request = parse_argv(&owned).unwrap();
    let outcome = execute(&request, &mut env);
    assert_eq!(host.spawns(), 0);
    assert_eq!(host.git_children(), 0);
    outcome
}

#[cfg(feature = "git-worker")]
#[test]
fn list_and_help_have_no_effects_and_show_the_bundled_plugin() {
    let world = World::new();
    let before = world.store.load_registry().unwrap();
    let mut index = IndexCache::default();
    let listed = run(&world, &mut index, &["plugin", "list"]);
    assert_eq!(listed.code, 0);
    assert!(listed.body.contains("dctx/git"));
    assert!(listed.body.contains("enabled"));
    assert!(listed.body.contains("needs-consent"));
    assert!(listed.body.contains("same-binary"));
    let json = run(&world, &mut index, &["plugin", "list", "--json"]);
    assert!(json.body.contains("\"facade_git\":false"));
    assert!(json.body.contains("\"origin\":\"bundled\""));
    assert!(json.body.contains("needs-consent"));
    let info = run(
        &world,
        &mut index,
        &["plugin", "info", "dctx/git", "--json"],
    );
    assert_eq!(info.code, 0);
    for label in [
        "file.read:.gitignore",
        "file.write:.gitignore",
        "program:git.add-ignore",
        "resource:git-admin:stage-ignore",
    ] {
        assert!(info.body.contains(label), "{label} in {}", info.body);
    }
    let help = run(&world, &mut index, &["plugin", "help"]);
    assert_eq!(help.code, 0);
    assert!(help.body.contains("plugin search"));
    assert_eq!(world.net.requests(), [] as [std::string::String; 0]);
    assert_eq!(world.store.load_registry().unwrap(), before);
    let search = run(&world, &mut index, &["plugin", "search", "git", "--json"]);
    assert_eq!(search.code, 0);
    assert!(search.body.contains("\"trust\":\"absent\""));
    assert_eq!(world.net.requests(), [] as [std::string::String; 0]);
}

#[cfg(not(feature = "git-worker"))]
#[test]
fn strict_list_reports_unavailable_and_help_still_works_without_effects() {
    let world = World::new();
    let before = world.store.load_registry().unwrap();
    let mut index = IndexCache::default();
    for args in [vec!["plugin", "list"], vec!["plugin", "list", "--json"]] {
        let listed = run(&world, &mut index, &args);
        assert_eq!(listed.code, 0, "{}", listed.body);
        for label in ["dctx/git", "bundled", "unavailable", "excluded"] {
            assert!(listed.body.contains(label), "{}", listed.body);
        }
        assert!(!listed.body.contains("same-binary"));
        assert!(!listed.body.contains("needs-consent"));
    }
    let info = run(
        &world,
        &mut index,
        &["plugin", "info", "dctx/git", "--json"],
    );
    assert_eq!(info.code, 0, "{}", info.body);
    assert!(info.body.contains("unavailable"));
    for args in [vec!["plugin", "help"], vec!["plugin", "help", "--json"]] {
        let help = run(&world, &mut index, &args);
        assert_eq!(help.code, 0, "{}", help.body);
        assert!(help.body.contains("plugin search"));
    }
    assert!(world.net.requests().is_empty());
    assert_eq!(world.store.load_registry().unwrap(), before);
}

#[test]
fn search_refresh_install_enable_and_refusals() {
    let world = World::new();
    world.publish("1.0.0", 1);
    world.publish_index("Protect exports", "1.0.0", 1);
    let mut index = IndexCache::default();
    let missing = run(
        &world,
        &mut index,
        &["plugin", "search", "--refresh", "--json"],
    );
    assert_eq!(missing.code, 0, "{}", missing.body);
    assert!(missing.body.contains("Protect exports"));
    assert_ne!(world.net.requests(), [] as [std::string::String; 0]);
    let requests = world.net.requests().len();
    let cached = run(&world, &mut index, &["plugin", "search", "git", "--json"]);
    assert_eq!(cached.code, 0);
    assert_eq!(
        world.net.requests().len(),
        requests,
        "search without refresh does not fetch"
    );

    let refused = run(
        &world,
        &mut index,
        &["plugin", "install", "missing/repo", "--json"],
    );
    assert_eq!(refused.code, 1);
    assert!(refused.body.contains("NotFound"));
    let bad_url = run(
        &world,
        &mut index,
        &[
            "plugin",
            "install",
            "--url",
            "http://releases.example.org/plugin.json",
            "--id",
            "dctx/git",
            "--json",
        ],
    );
    assert_eq!(bad_url.code, 1);
    assert!(bad_url.body.contains("Origin"));

    let installed = run(
        &world,
        &mut index,
        &[
            "plugin",
            "install",
            "fixture/dctx-plugin-git@1.0.0",
            "--json",
        ],
    );
    assert_eq!(installed.code, 0, "{}", installed.body);
    assert!(installed.body.contains("\"activated\":false"));
    let listed = run(&world, &mut index, &["plugin", "list", "--json"]);
    assert!(
        listed.body.contains("inactive")
            || listed.body.contains("override")
            || listed.body.contains("installed")
    );

    let blocked = run(
        &world,
        &mut index,
        &["plugin", "enable", "dctx/git", "--approve", "--json"],
    );
    assert_eq!(blocked.code, 1, "{}", blocked.body);
    assert!(
        blocked.body.contains("ConfinementRequired")
            || blocked.body.contains("NotActive")
            || blocked.body.contains("ConsentRequired"),
        "{}",
        blocked.body
    );
}

#[cfg(feature = "git-worker")]
#[test]
fn bundled_enable_routes_git_and_canonical_run_then_disable_sticks() {
    let world = World::new();
    let mut index = IndexCache::default();
    let refused = run(
        &world,
        &mut index,
        &["plugin", "enable", "dctx/git", "--json"],
    );
    assert_eq!(refused.code, 1);
    assert!(refused.body.contains("ConsentRequired"));
    assert!(world.store.load_registry().unwrap().is_none());
    let enabled = run(
        &world,
        &mut index,
        &[
            "plugin",
            "enable",
            "dctx/git",
            "--approve",
            "--project",
            "/work",
            "--json",
        ],
    );
    assert_eq!(enabled.code, 0, "{}", enabled.body);
    let git = run(&world, &mut index, &["git", "status", "--json"]);
    let run_cmd = run(
        &world,
        &mut index,
        &["plugin", "run", "dctx/git", "--", "status"],
    );
    assert_eq!(git.code, 0, "{}", git.body);
    assert_eq!(run_cmd.code, 0, "{}", run_cmd.body);
    assert!(git.body.contains("\"verb\":\"status\""));
    assert!(run_cmd.body.contains("\"verb\":\"status\""));
    assert!(git.body.contains("\"spawned\":false"));
    assert!(git.body.contains("same-binary"));
    let unknown = run(&world, &mut index, &["git", "add", "--json"]);
    assert_eq!(unknown.code, 2);
    assert!(unknown.body.contains("unknown-verb"));
    let updated = run(
        &world,
        &mut index,
        &["plugin", "update", "dctx/git", "--check", "--json"],
    );
    assert_eq!(updated.code, 0, "{}", updated.body);
    assert!(updated.body.contains("containing-release"));
    assert!(updated.body.contains("\"downloaded\":false"));
    assert_eq!(world.net.requests(), [] as [std::string::String; 0]);
    let verified = run(
        &world,
        &mut index,
        &["plugin", "verify", "dctx/git", "--json"],
    );
    assert_eq!(verified.code, 0, "{}", verified.body);
    assert!(verified.body.contains("\"executed\":false"));
    let disabled = run(
        &world,
        &mut index,
        &["plugin", "disable", "dctx/git", "--json"],
    );
    assert_eq!(disabled.code, 0, "{}", disabled.body);
    let again = run(&world, &mut index, &["git", "protect", "--json"]);
    assert_eq!(again.code, 1);
    assert!(again.body.contains("Disabled"));
    let removed = run(
        &world,
        &mut index,
        &["plugin", "remove", "dctx/git", "--json"],
    );
    assert_eq!(removed.code, 0, "{}", removed.body);
    let listed = run(&world, &mut index, &["plugin", "list"]);
    assert!(listed.body.contains("disabled"));
    assert!(!listed.body.contains("needs-consent"));
}

#[cfg(not(feature = "git-worker"))]
#[test]
fn strict_alias_and_canonical_run_refuse_without_effects() {
    let world = World::new();
    let mut index = IndexCache::default();
    for installed in [false, true] {
        if installed {
            world.publish("1.0.0", 1);
            let outcome = run(
                &world,
                &mut index,
                &[
                    "plugin",
                    "install",
                    "--url",
                    "https://releases.example.org/dctx-git/1.0.0/plugin.json",
                    "--id",
                    "dctx/git",
                    "--json",
                ],
            );
            assert_eq!(outcome.code, 0, "{}", outcome.body);
        }
        let before = world.store.load_registry().unwrap();
        let requests = world.net.requests().len();
        for verb in ["status", "protect"] {
            for json in [false, true] {
                let mut alias = vec!["git", verb];
                let mut canonical = vec!["plugin", "run", "dctx/git"];
                if json {
                    alias.push("--json");
                    canonical.push("--json");
                }
                canonical.extend(["--", verb]);
                for args in [alias, canonical] {
                    let refused = run(&world, &mut index, &args);
                    assert_eq!(refused.code, 1, "{}", refused.body);
                    assert!(refused.body.contains("StrictBuild"), "{}", refused.body);
                    if json {
                        assert!(refused.body.contains("\"error\":\"StrictBuild\""));
                        assert!(refused.body.contains("\"spawned\":false"));
                    }
                }
            }
        }
        assert_eq!(world.net.requests().len(), requests);
        assert_eq!(world.store.load_registry().unwrap(), before);
    }
}

#[test]
fn offline_install_verifies_and_a_stale_trust_refresh_refuses() {
    let world = World::new();
    let bytes = world.bundle("1.0.0", 1);
    let path = std::env::temp_dir().join(format!(
        "dctx-cli-bundle-{}.dctx-plugin",
        std::process::id()
    ));
    fs::write(&path, &bytes).unwrap();
    let mut index = IndexCache::default();
    let args_path = path.to_string_lossy().to_string();
    let installed = run(
        &world,
        &mut index,
        &[
            "plugin",
            "install",
            "--file",
            args_path.as_str(),
            "--id",
            "dctx/git",
            "--json",
        ],
    );
    assert_eq!(installed.code, 0, "{}", installed.body);
    let _ = fs::remove_file(&path);
    let verified = run(
        &world,
        &mut index,
        &["plugin", "verify", "dctx/git", "--json"],
    );
    assert_eq!(verified.code, 0, "{}", verified.body);
    let mut policy = world.policy.clone();
    policy.freshness_ceiling_secs = Some(10);
    let host = PluginHost::fake();
    world.clock.set(NOW + 11);
    let mut env = Env {
        network: &world.net,
        store: &world.store,
        clock: &world.clock,
        trust: &world.trust,
        policy: &policy,
        index: &mut index,
        index_url: INDEX,
        index_key: Some(world.index_key),
        host: &host,
    };
    let request = parse_argv(&[
        "dctx".into(),
        "plugin".into(),
        "verify".into(),
        "--refresh-trust".into(),
        "--json".into(),
    ])
    .unwrap();
    let stale = execute(&request, &mut env);
    assert_eq!(stale.code, 1, "{}", stale.body);
    assert!(stale.body.contains("StaleRevocation"));
    let positive = run(
        &world,
        &mut index,
        &["plugin", "verify", "dctx/git", "--json"],
    );
    assert_eq!(positive.code, 0, "{}", positive.body);
}

#[test]
fn hostile_search_text_is_escaped_and_unsigned_refresh_refuses() {
    let world = World::new();
    let summary = format!("ok{}", '\u{202e}');
    world.publish("1.0.0", 1);
    world.publish_index(&summary, "1.0.0", 1);
    let mut index = IndexCache::default();
    let found = run(
        &world,
        &mut index,
        &["plugin", "search", "--refresh", "--json"],
    );
    assert_eq!(found.code, 0, "{}", found.body);
    assert!(!found.body.contains('\u{202e}'));
    assert!(found.body.contains("\\\\u{202e}") || found.body.contains("\\u{202e}"));
    world.net.unroute(INDEX);
    world.net.serve(INDEX, b"not-an-index");
    let mut index = IndexCache::default();
    let refused = run(
        &world,
        &mut index,
        &["plugin", "search", "--refresh", "--json"],
    );
    assert_eq!(refused.code, 1);
    assert!(refused.body.contains("Signature") || refused.body.contains("Malformed"));
}
