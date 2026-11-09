// SPDX-License-Identifier: Apache-2.0
//! Public desktop plugin behavior for PLUGINS.md §4/§5/§7.3 and decision 6.
//! Uses only injected distribution fakes, catalog metadata and public desktop operations.
//! No filesystem, network or worker execution; verifies consent, sticky disable and views.

use dctx_desktop_api::{
    bundled_plugin_version, DesktopError, PluginFailure, PluginOps, PluginSource, PluginState,
};
use dctx_plugin_distribution::{
    domain::registry_codec,
    fakes::{FakeNetwork, Fault, FixedClock, MemoryStore},
    CapabilitySet, Confinement, ConsentApproval, ConsentScope, Distribution, DistributionError,
    Enablement, HttpsUrl, IndexEntry, IndexRelease, InstallPolicy, Origin, PluginId, Registry,
    Sha256Hex, Store, Target, TrustStore, VerifiedIndex, Version,
};

struct Fixture {
    network: FakeNetwork,
    store: MemoryStore,
    clock: FixedClock,
    trust: TrustStore,
    policy: InstallPolicy,
}

impl Fixture {
    fn new() -> Self {
        Self {
            network: FakeNetwork::new(),
            store: MemoryStore::new(),
            clock: FixedClock::at(100),
            trust: TrustStore::new(),
            policy: InstallPolicy::new(Target::AppleArm64),
        }
    }

    fn ops(&self) -> PluginOps<'_> {
        PluginOps::new(
            &self.network,
            &self.store,
            &self.clock,
            &self.trust,
            &self.policy,
        )
    }

    fn dist(&self) -> Distribution<'_> {
        Distribution::new(
            &self.network,
            &self.store,
            &self.clock,
            &self.trust,
            &self.policy,
        )
    }

    fn save(&self, registry: &Registry) {
        let token = self.store.try_lock().unwrap();
        self.store
            .save_registry(token, &registry_codec::encode(registry))
            .unwrap();
        self.store.unlock(token);
    }

    fn third_party(&self) -> PluginId {
        let id = PluginId::parse("example/tool").unwrap();
        let mut registry = Registry::default();
        registry
            .record_install(&id, Origin::Installed, bundled_plugin_version().unwrap())
            .unwrap();
        self.save(&registry);
        id
    }
}

fn approval() -> ConsentApproval {
    let version = bundled_plugin_version().unwrap();
    ConsentApproval {
        digest: version.digest,
        capabilities: version.capabilities,
        scope: ConsentScope::User,
    }
}

#[test]
fn official_installation_acceptance_lists_default_bundled_enabled_with_grants() {
    let f = Fixture::new();
    f.ops().consent("dctx/git", &approval()).unwrap();
    let rows = f.ops().list().unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.id, "dctx/git");
    assert_eq!(row.version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(row.source, PluginSource::Bundled);
    assert_eq!(row.state, PluginState::Enabled);
    assert!(row.enabled);
    assert!(!row.needs_consent);
    for grant in dctx::bundled_git_plugin().capabilities {
        assert!(row.capabilities.contains(&grant));
    }
    let encoded = serde_json::to_string(row).unwrap();
    assert_eq!(
        serde_json::from_str::<dctx_desktop_api::PluginRow>(&encoded).unwrap(),
        *row
    );
}

#[test]
fn migrated_or_copied_binary_needs_consent_and_enable_never_grants_it() {
    let f = Fixture::new();
    let row = f.ops().list().unwrap().remove(0);
    assert!(row.enabled && row.needs_consent);
    assert_eq!(row.state, PluginState::NeedsConsent);
    assert!(row.last_failure.is_none());
    assert_eq!(
        f.ops().enable("dctx/git").unwrap().state,
        PluginState::NeedsConsent
    );
    assert_eq!(f.store.load_registry().unwrap(), None);
    let id = PluginId::parse("dctx/git").unwrap();
    f.dist()
        .register_bundled(&id, bundled_plugin_version().unwrap())
        .unwrap();
    let before = f.store.load_registry().unwrap();
    assert_eq!(
        f.ops().enable("dctx/git").unwrap().state,
        PluginState::NeedsConsent
    );
    assert_eq!(before, f.store.load_registry().unwrap());
    assert!(f
        .dist()
        .registry()
        .unwrap()
        .get(&id)
        .unwrap()
        .consent
        .is_none());
}

#[test]
fn disable_survives_bundled_and_independent_updates_and_can_be_explicitly_reenabled() {
    let f = Fixture::new();
    f.ops().consent("dctx/git", &approval()).unwrap();
    assert_eq!(f.ops().disable("dctx/git").unwrap(), 0);
    let id = PluginId::parse("dctx/git").unwrap();
    let mut update = bundled_plugin_version().unwrap();
    update.version = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    update.release_sequence = 2;
    update.digest = Sha256Hex::from_bytes([2; 32]);
    f.dist().register_bundled(&id, update.clone()).unwrap();
    assert_eq!(f.ops().list().unwrap()[0].state, PluginState::Disabled);
    assert_eq!(
        f.dist().registry().unwrap().get(&id).unwrap().enablement,
        Enablement::Disabled
    );
    assert_eq!(
        f.ops().enable("dctx/git").unwrap().state,
        PluginState::Enabled
    );

    let id = f.third_party();
    let ops = f.ops().with_confinement(Confinement::Established);
    ops.consent(id.as_str(), &approval()).unwrap();
    ops.disable(id.as_str()).unwrap();
    let mut registry = f.dist().registry().unwrap();
    registry
        .record_install(&id, Origin::Installed, update.clone())
        .unwrap();
    f.save(&registry);
    f.dist().activate(&id, &update.digest).unwrap();
    let row = ops
        .list()
        .unwrap()
        .into_iter()
        .find(|row| row.id == id.as_str())
        .unwrap();
    assert_eq!(row.state, PluginState::Disabled);
    assert_eq!(row.version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(ops.enable(id.as_str()).unwrap().state, PluginState::Enabled);
}

#[test]
fn third_party_install_is_inactive_until_explicit_consent_and_confinement() {
    let f = Fixture::new();
    let id = f.third_party();
    let row = f
        .ops()
        .list()
        .unwrap()
        .into_iter()
        .find(|row| row.id == id.as_str())
        .unwrap();
    assert_eq!(row.source, PluginSource::Installed);
    assert_eq!(row.state, PluginState::Disabled);
    assert!(!row.enabled && row.needs_consent);
    assert_ne!(row.capabilities, [] as [std::string::String; 0]);
    let before = f.store.load_registry().unwrap();
    assert_eq!(
        f.ops().enable(id.as_str()).unwrap().state,
        PluginState::NeedsConsent
    );
    assert_eq!(
        f.ops().consent(id.as_str(), &approval()),
        Err(DesktopError::Plugin {
            reason: DistributionError::ConfinementRequired,
        })
    );
    assert_eq!(before, f.store.load_registry().unwrap());
    let row = f
        .ops()
        .with_confinement(Confinement::Established)
        .consent(id.as_str(), &approval())
        .unwrap();
    assert_eq!(row.state, PluginState::Enabled);
    assert!(!row.needs_consent);
}

#[test]
fn wrong_digest_or_insufficient_grants_never_approve_a_plugin() {
    let f = Fixture::new();
    let id = f.third_party();
    let ops = f.ops().with_confinement(Confinement::Established);
    let before = f.store.load_registry().unwrap();
    let mut bad = approval();
    bad.digest = Sha256Hex::from_bytes([9; 32]);
    assert_eq!(
        ops.consent(id.as_str(), &bad),
        Err(DesktopError::PluginNeedsConsent)
    );
    bad = approval();
    bad.capabilities = CapabilitySet::default();
    assert_eq!(
        ops.consent(id.as_str(), &bad),
        Err(DesktopError::PluginNeedsConsent)
    );
    assert_eq!(before, f.store.load_registry().unwrap());
    assert_eq!(
        ops.consent(id.as_str(), &approval()).unwrap().state,
        PluginState::Enabled
    );
}

#[test]
fn failure_rows_preserve_the_latest_reason_and_quarantine() {
    let f = Fixture::new();
    let id = PluginId::parse("dctx/git").unwrap();
    f.ops().consent(id.as_str(), &approval()).unwrap();
    let failures = [PluginFailure {
        id: id.clone(),
        reason: DistributionError::Interrupted,
    }];
    let row = f.ops().with_failures(&failures).list().unwrap().remove(0);
    assert_eq!(row.state, PluginState::Failed);
    assert_eq!(
        row.last_failure,
        Some(DistributionError::Interrupted.to_string())
    );
    let mut registry = f.dist().registry().unwrap();
    registry.quarantine(&id, &approval().digest).unwrap();
    f.save(&registry);
    let row = f.ops().list().unwrap().remove(0);
    assert_eq!(row.state, PluginState::Failed);
    assert_eq!(
        row.last_failure,
        Some(DistributionError::Quarantined.to_string())
    );
    assert!(f.ops().consent(id.as_str(), &approval()).is_err());
}

#[test]
fn list_help_and_update_checks_never_lock_write_or_fetch() {
    let f = Fixture::new();
    f.ops().consent("dctx/git", &approval()).unwrap();
    let before = f.store.load_registry().unwrap();
    let token = f.store.hold_lock();
    assert_eq!(f.ops().list().unwrap()[0].state, PluginState::Enabled);
    assert!(f.ops().help().contains("consent"));
    let update = f.ops().check_update("dctx/git", None, None).unwrap();
    assert!(update.containing_release);
    assert_eq!(update.version, None);
    assert_eq!(before, f.store.load_registry().unwrap());
    assert_eq!(f.network.requests(), [] as [std::string::String; 0]);
    f.store.unlock(token);
    f.store.inject(Fault::SaveRegistry);
    f.ops().list().unwrap();
    assert_eq!(
        f.ops().disable("dctx/git"),
        Err(DesktopError::Plugin {
            reason: DistributionError::Interrupted
        })
    );
    assert_eq!(before, f.store.load_registry().unwrap());
}

#[test]
fn independent_update_check_uses_only_the_supplied_verified_index() {
    let f = Fixture::new();
    let id = f.third_party();
    let index = VerifiedIndex {
        sequence: 1,
        entries: vec![IndexEntry {
            id: id.clone(),
            repository: "example/tool".into(),
            summary: "A tool".into(),
            publisher_key_fingerprint: approval().digest,
            releases: vec![IndexRelease {
                version: Version::parse("2.0.0").unwrap(),
                release_sequence: 2,
                manifest_url: HttpsUrl::parse("https://example.com/plugin.json").unwrap(),
                manifest_sha256: approval().digest,
            }],
        }],
    };
    let before = f.store.load_registry().unwrap();
    let result = f
        .ops()
        .check_update(id.as_str(), Some(&index), None)
        .unwrap();
    assert!(!result.containing_release);
    assert_eq!(result.version.as_deref(), Some("2.0.0"));
    assert!(f.ops().check_update(id.as_str(), None, None).is_err());
    assert!(f
        .ops()
        .check_update(
            id.as_str(),
            Some(&index),
            Some(&Version::parse("9.0.0").unwrap())
        )
        .is_err());
    assert_eq!(before, f.store.load_registry().unwrap());
    assert_eq!(f.network.requests(), [] as [std::string::String; 0]);
}

#[test]
fn unknown_plugin_mutations_refuse_without_storage_creation() {
    let f = Fixture::new();
    let missing = Err(DesktopError::Plugin {
        reason: DistributionError::NotInstalled,
    });
    assert_eq!(f.ops().enable("example/missing"), missing);
    assert_eq!(
        f.ops().disable("example/missing"),
        Err(DesktopError::Plugin {
            reason: DistributionError::NotInstalled
        })
    );
    assert!(f.ops().enable("../bad").is_err());
    assert_eq!(f.store.load_registry().unwrap(), None);
    assert_eq!(f.ops().disable("dctx/git").unwrap(), 0);
    assert_eq!(f.ops().list().unwrap()[0].state, PluginState::Disabled);
}

#[test]
fn review_payload_serializes_exact_identity_bounds_and_roundtrips_to_approval() {
    let f = Fixture::new();
    let row = f
        .ops()
        .list()
        .unwrap()
        .remove(0)
        .with_consent(&Registry::default())
        .unwrap();
    let encoded = serde_json::to_value(&row).unwrap();
    let payload = &encoded["consent"];
    assert_eq!(payload["id"], row.id);
    assert_eq!(payload["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(payload["digest"], approval().digest.to_string());
    assert_eq!(payload["scope"], serde_json::json!({"kind": "user"}));
    let lines = payload["capability_lines"].as_array().unwrap();
    assert_eq!(lines.len(), approval().capabilities.len());
    assert!(lines.iter().all(|line| {
        row.capabilities
            .contains(&line.as_str().unwrap().to_owned())
    }));

    // Import the transported entries through the public codec to prove every bound
    // and section survives transport, rather than reconstructing grants from labels.
    let mut registry = Registry::default();
    let id = PluginId::parse("dctx/git").unwrap();
    registry.register_bundled(&id, bundled_plugin_version().unwrap());
    let mut data: serde_json::Value =
        serde_json::from_slice(&registry_codec::encode(&registry)).unwrap();
    let entries: Vec<_> = payload["capabilities"]
        .as_object()
        .unwrap()
        .iter()
        .flat_map(|(section, entries)| {
            entries
                .as_array()
                .unwrap()
                .iter()
                .map(move |entry| serde_json::json!({"section": section, "entry": entry}))
        })
        .collect();
    data["plugins"][0]["versions"][0]["capabilities"] = entries.into();
    let restored = registry_codec::decode(data.to_string().as_bytes()).unwrap();
    let capabilities = restored
        .get(&id)
        .unwrap()
        .active_version()
        .unwrap()
        .capabilities
        .clone();
    assert_eq!(capabilities, approval().capabilities);
    let approval = ConsentApproval {
        digest: Sha256Hex::parse(
            payload["digest"].as_str().unwrap(),
            dctx_plugin_distribution::Document::Registry,
        )
        .unwrap(),
        capabilities,
        scope: ConsentScope::User,
    };
    assert_eq!(f.store.load_registry().unwrap(), None);
    assert_eq!(f.network.requests(), [] as [std::string::String; 0]);
    assert_eq!(
        f.ops().consent("dctx/git", &approval).unwrap().state,
        PluginState::Enabled
    );
}

#[test]
fn review_payload_selects_inactive_candidate_and_refuses_stale_versions() {
    let f = Fixture::new();
    let id = f.third_party();
    let mut registry = f.dist().registry().unwrap();
    let mut candidate = bundled_plugin_version().unwrap();
    candidate.version = Version::parse("2.0.0").unwrap();
    candidate.release_sequence = 2;
    candidate.digest = Sha256Hex::from_bytes([3; 32]);
    registry
        .record_install(&id, Origin::Installed, candidate.clone())
        .unwrap();
    f.save(&registry);
    let row = f
        .ops()
        .list()
        .unwrap()
        .into_iter()
        .find(|row| row.id == id.as_str())
        .unwrap();
    let reviewed = row.clone().with_consent(&registry).unwrap();
    let payload = reviewed.consent.unwrap();
    assert_eq!(payload["id"], id.as_str());
    assert_eq!(payload["version"], "2.0.0");
    assert_eq!(payload["digest"], candidate.digest.to_string());
    let mut stale = row.clone();
    stale.version = Some("1.0.0".into());
    assert_eq!(
        stale.with_consent(&registry),
        Err(DesktopError::PluginNeedsConsent)
    );
    assert!(row.with_consent(&Registry::default()).is_err());
    f.dist().activate(&id, &approval().digest).unwrap();
    let row = f
        .ops()
        .list()
        .unwrap()
        .into_iter()
        .find(|row| row.id == id.as_str())
        .unwrap();
    let registry = f.dist().registry().unwrap();
    let payload = row.with_consent(&registry).unwrap().consent.unwrap();
    assert_eq!(payload["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(payload["digest"], approval().digest.to_string());
}
