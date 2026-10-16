//! Activation, consent, sticky disable, leases, rollback, freshness, verify and crash/concurrency.
//! Install.tla PP9Integrity and the activation-crash/install-integrity scenario pairs;
//! requirements I-ATOMIC, I-CONSENT, I-TRUST, I-MANAGEMENT, I-BUNDLED; decision 080 confinement gate.

mod common;

use common::*;
use dctx_plugin_distribution::fakes::{Fault, MemoryStore};
use dctx_plugin_distribution::{
    CapabilitySet, Confinement, ConsentApproval, ConsentScope, Distribution, DistributionError as E, Enablement,
    InstalledVersion, Origin, Sha256Hex, Store, Version, fingerprint,
};

fn approve(report: &dctx_plugin_distribution::InstallReport) -> ConsentApproval {
    ConsentApproval { digest: report.key.digest, capabilities: report.capabilities.clone(), scope: ConsentScope::User }
}

fn enabled(world: &World, release: &Release) {
    let report = world.publish_and_install(release);
    world.dist().activate(&id(GIT), &release.digest()).unwrap();
    world.dist().enable(&id(GIT), Some(&approve(&report)), Confinement::Established).unwrap();
}

#[test]
fn enable_needs_consent_and_confinement_for_third_party() {
    let world = World::new();
    let release = Release::git("1.0.0", 1);
    let report = world.publish_and_install(&release);
    let git = id(GIT);
    assert_eq!(world.dist().enable(&git, None, Confinement::Established), Err(E::NotActive));
    world.dist().activate(&git, &release.digest()).unwrap();
    assert_eq!(world.dist().enable(&git, None, Confinement::Established), Err(E::ConsentRequired));
    let wrong = ConsentApproval { digest: Sha256Hex::from_bytes([3; 32]), ..approve(&report) };
    assert_eq!(world.dist().enable(&git, Some(&wrong), Confinement::Established), Err(E::ConsentRequired));
    let narrow = ConsentApproval { capabilities: CapabilitySet::default(), ..approve(&report) };
    assert_eq!(world.dist().enable(&git, Some(&narrow), Confinement::Established), Err(E::ConsentRequired));
    assert_eq!(world.dist().enable(&git, Some(&approve(&report)), Confinement::Unavailable), Err(E::ConfinementRequired));
    world.dist().enable(&git, Some(&approve(&report)), Confinement::Established).unwrap();
    let admission = world.dist().admit(&git).unwrap();
    assert_eq!(admission.digest, release.digest());
    world.dist().release(&admission).unwrap();
}

#[test]
fn equal_update_keeps_consent_and_expansion_needs_new_consent() {
    let world = World::new();
    let git = id(GIT);
    enabled(&world, &Release::git("1.0.0", 1));
    let v2 = Release::git("2.0.0", 2);
    world.publish_and_install(&v2);
    world.dist().activate(&git, &v2.digest()).unwrap();
    assert!(world.dist().admit(&git).is_ok(), "equal capability update keeps derived consent");

    let mut v3 = Release::git("3.0.0", 3);
    v3.capabilities = WIDE.into();
    let report = world.publish_and_install(&v3);
    assert_eq!(world.dist().activate(&git, &v3.digest()), Err(E::ConsentRequired));
    assert_eq!(world.active(), Some(v2.digest()), "old activation kept until consent");
    let record = world.dist().registry().unwrap().get(&git).cloned().unwrap();
    assert!(!report.capabilities.is_within(&record.consent.unwrap().capabilities));
}

#[test]
fn disable_is_sticky_and_persisted_across_updates_and_bundled_releases() {
    let world = World::new();
    let git = id(GIT);
    enabled(&world, &Release::git("1.0.0", 1));
    let lease = world.dist().admit(&git).unwrap();
    assert_eq!(world.dist().disable(&git), Ok(1), "caller must cancel one leased worker");
    assert_eq!(world.dist().admit(&git), Err(E::Disabled));
    world.dist().release(&lease).unwrap();

    let v2 = Release::git("2.0.0", 2);
    world.publish_and_install(&v2);
    world.dist().activate(&git, &v2.digest()).unwrap();
    let restarted = Distribution::new(&world.net, &world.store, &world.clock, &world.trust, &world.policy);
    assert_eq!(restarted.registry().unwrap().get(&git).unwrap().enablement, Enablement::Disabled);
    assert_eq!(restarted.admit(&git), Err(E::Disabled));

    restarted.remove(&git).unwrap();
    let bundled = InstalledVersion {
        version: Version::parse("9.0.0").unwrap(),
        release_sequence: 9,
        digest: Sha256Hex::from_bytes([9; 32]),
        manifest_sha256: Sha256Hex::from_bytes([8; 32]),
        signer: fingerprint(&public(&world.official)),
        capabilities: CapabilitySet::default(),
        quarantined: false,
    };
    restarted.register_bundled(&git, bundled).unwrap();
    let status = &restarted.list().unwrap()[0];
    assert_eq!(status.enablement, Enablement::Disabled, "removal mask survives a bundled release");
    assert_eq!(status.version, None);
}

#[test]
fn bundled_default_is_enabled_but_needs_consent_when_copied() {
    let world = World::new();
    let git = id(GIT);
    let bundled = InstalledVersion {
        version: Version::parse("1.0.0").unwrap(),
        release_sequence: 1,
        digest: Sha256Hex::from_bytes([1; 32]),
        manifest_sha256: Sha256Hex::from_bytes([2; 32]),
        signer: fingerprint(&public(&world.official)),
        capabilities: CapabilitySet::default(),
        quarantined: false,
    };
    world.dist().register_bundled(&git, bundled.clone()).unwrap();
    let status = &world.dist().list().unwrap()[0];
    assert_eq!((status.origin, status.enablement, status.needs_consent), (Origin::Bundled, Enablement::Enabled, true));
    assert_eq!(world.dist().admit(&git), Err(E::ConsentRequired));
    let approval = ConsentApproval { digest: bundled.digest, capabilities: CapabilitySet::default(), scope: ConsentScope::User };
    world.dist().enable(&git, Some(&approval), Confinement::Unavailable).unwrap();
    assert!(world.dist().admit(&git).is_ok(), "bundled git may run brokered without OS confinement (decision 080)");
}

#[test]
fn leases_block_removal_and_rollback_is_explicit() {
    let mut world = World::new();
    let git = id(GIT);
    let v1 = Release::git("1.0.0", 1);
    enabled(&world, &v1);
    let lease = world.dist().admit(&git).unwrap();
    let v2 = Release::git("2.0.0", 2);
    world.publish_and_install(&v2);
    world.dist().activate(&git, &v2.digest()).unwrap();
    assert_eq!(world.dist().remove_version(&git, &v1.digest()), Err(E::Leased));
    world.dist().release(&lease).unwrap();

    assert_eq!(world.dist().activate(&git, &v1.digest()), Err(E::Downgrade { candidate: 1, floor: 2 }));
    assert!(world.dist().rollback(&git, &v1.digest(), " ").is_err(), "rollback needs a reason");
    world.dist().rollback(&git, &v1.digest(), "v2 regression").unwrap();
    assert_eq!(world.active(), Some(v1.digest()));
    world.dist().activate(&git, &v2.digest()).unwrap();
    world.trust.record_revocations([], [v1.digest()], NOW);
    assert_eq!(world.dist().rollback(&git, &v1.digest(), "again"), Err(E::ArtifactRevoked));
    world.dist().remove_version(&git, &v1.digest()).unwrap();
    assert_eq!(world.dist().remove_version(&git, &v2.digest()), Err(E::Leased), "active version is never unlinked");
}

#[test]
fn offline_freshness_reports_age_and_ceiling_refuses() {
    let mut world = World::new();
    let git = id(GIT);
    enabled(&world, &Release::git("1.0.0", 1));
    world.clock.set(NOW + 7200);
    assert_eq!(world.dist().list().unwrap()[0].revocation_age_secs, Some(7200));
    assert!(world.dist().admit(&git).is_ok(), "no ceiling: last known state is reported, not refused");
    world.policy.freshness_ceiling_secs = Some(3600);
    assert_eq!(world.dist().admit(&git).map(|_| ()), Err(E::StaleRevocation { age_secs: Some(7200) }));
    assert_eq!(world.dist().activate(&git, &Sha256Hex::from_bytes([0; 32])), Err(E::StaleRevocation { age_secs: Some(7200) }));
    world.trust.record_revocations([], [], NOW + 7000);
    assert!(world.dist().admit(&git).is_ok());
}

#[test]
fn verify_quarantines_tampered_files_without_running_them() {
    let world = World::new();
    let git = id(GIT);
    let release = Release::git("1.0.0", 1);
    enabled(&world, &release);
    assert!(world.dist().verify(&git).unwrap().iter().all(|outcome| outcome.result.is_ok()));
    let key = dctx_plugin_distribution::InstallKey { id: git.clone(), version: Version::parse("1.0.0").unwrap(), digest: release.digest() };
    let mut evil = release.executable().to_vec();
    evil[0] ^= 1;
    world.store.tamper(&key, &release.artifacts[0].1, &evil);
    let outcomes = world.dist().verify(&git).unwrap();
    assert_eq!(outcomes[0].result, Err(E::IntegrityFailure));
    assert_eq!(world.dist().admit(&git), Err(E::Quarantined));
    assert!(world.dist().list().unwrap()[0].blocked);
}

#[test]
fn interrupted_activation_keeps_the_previous_complete_version() {
    for fault in [Fault::Stage, Fault::Publish, Fault::SaveRegistry] {
        let world = World::new();
        let git = id(GIT);
        let v1 = Release::git("1.0.0", 1);
        enabled(&world, &v1);
        let v2 = Release::git("2.0.0", 2);
        world.publish(&v2, &world.official);
        world.store.inject(fault);
        assert_eq!(world.install(&v2).map(|_| ()), Err(E::Interrupted), "{fault:?}");
        assert_eq!(world.active(), Some(v1.digest()), "{fault:?}");
        assert!(world.dist().admit(&git).is_ok());
        let report = world.dist().recover().unwrap();
        assert!(report.discarded_staging.is_empty(), "{fault:?}: staging already discarded");
        if fault == Fault::SaveRegistry {
            assert_eq!(report.removed_orphans.len(), 1, "published but unrecorded directory is an orphan");
        }
        assert!(world.dist().registry().unwrap().get(&git).unwrap().version(&v2.digest()).is_none());
        world.install(&v2).unwrap();
        world.dist().activate(&git, &v2.digest()).unwrap();
        assert_eq!(world.active(), Some(v2.digest()));
    }
}

#[test]
fn concurrent_management_sees_the_lock_and_only_complete_activations() {
    let world = World::new();
    let git = id(GIT);
    let other = world.store.hold_lock();
    let release = Release::git("1.0.0", 1);
    world.publish(&release, &world.official);
    assert_eq!(world.install(&release).map(|_| ()), Err(E::Busy));
    assert_eq!(world.dist().disable(&git), Err(E::Busy));
    world.store.unlock(other);

    let releases: Vec<Release> = (1..=6).map(|n| Release::git(&format!("{n}.0.0"), n)).collect();
    for release in &releases {
        world.publish(release, &world.official);
    }
    let app = MemoryStore::clone(&world.store);
    std::thread::scope(|scope| {
        for (index, release) in releases.iter().enumerate() {
            let store = if index % 2 == 0 { &world.store } else { &app };
            let world = &world;
            scope.spawn(move || {
                let dist = Distribution::new(&world.net, store, &world.clock, &world.trust, &world.policy);
                let url = dctx_plugin_distribution::HttpsUrl::parse(&release.url()).unwrap();
                for _ in 0..10_000 {
                    match dist.install_url(&git_id(), &url) {
                        Err(E::Busy) => std::thread::yield_now(),
                        Err(E::Downgrade { .. }) | Ok(_) => break,
                        Err(other) => panic!("{other:?}"),
                    }
                }
                for _ in 0..10_000 {
                    match dist.activate(&git_id(), &release.digest()) {
                        Err(E::Busy) => std::thread::yield_now(),
                        _ => break,
                    }
                }
            });
        }
    });
    let registry = world.dist().registry().unwrap();
    let record = registry.get(&git).unwrap();
    let active = record.active.expect("some activation completed");
    let version = record.version(&active).unwrap();
    assert_eq!(version.release_sequence, record.high_water);
    for version in &record.versions {
        let key = dctx_plugin_distribution::InstallKey { id: git.clone(), version: version.version.clone(), digest: version.digest };
        assert!(world.store.installed_files(&key).is_some(), "recorded version is a complete directory");
    }
    assert!(world.store.staged().unwrap().is_empty());
}

fn git_id() -> dctx_plugin_distribution::PluginId {
    id(GIT)
}

