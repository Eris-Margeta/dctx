// SPDX-License-Identifier: Apache-2.0
//! Offline `.dctx-plugin` install: same trust gates with zero network use; malformed, hostile,
//! undeclared, wrong-target and oversized containers are refused before anything is published.
//! distribution.md §offline bundle, requirements I-BUNDLE and I-OFFLINE.

mod common;

use common::*;
use dctx_ed25519::VerificationError;
use dctx_plugin_distribution::domain::bundle::MAGIC;
use dctx_plugin_distribution::{DistributionError as E, Document, OfflineBundle};

fn refused(world: &World, bytes: &[u8]) -> E {
    let error = world.dist().install_bundle(&id(GIT), bytes).unwrap_err();
    let registry = world.dist().registry().unwrap();
    assert!(
        registry.get(&id(GIT)).is_none(),
        "refused bundle reached the registry"
    );
    assert_eq!(
        world.dist().recover().unwrap().discarded_staging,
        [] as [dctx_plugin_distribution::InstallKey; 0]
    );
    error
}

#[test]
fn signed_bundle_installs_offline_with_no_network() {
    let world = World::new();
    let mut release = Release::git("1.0.0", 1);
    release.resources = vec![("git-help.txt".into(), b"usage".to_vec())];
    let bundle = release.bundle_with(
        &world.official,
        &[("LICENSE", b"Apache-2.0"), ("NOTICE", b"")],
    );
    let report = world.dist().install_bundle(&id(GIT), &bundle).unwrap();
    assert_eq!(report.key.digest, release.digest());
    assert!(
        world.net.requests().is_empty(),
        "offline install touched the network"
    );
    let files = world.store.installed_files(&report.key).unwrap();
    assert!(files.contains(&"git-help.txt".to_owned()));
    assert!(
        !files.contains(&"LICENSE".to_owned()),
        "ancillary data is not installed as payload"
    );
}

#[test]
fn bundle_framing_and_table_failures_are_refused() {
    let world = World::new();
    let release = Release::git("1.0.0", 1);
    let good = release.bundle(&world.official);
    let mut trailing = good.clone();
    trailing.push(0);
    let mut magic = good.clone();
    magic[7] = b'2';
    assert_eq!(
        refused(&world, &good[..good.len() - 1]),
        E::Malformed {
            document: Document::Bundle,
            reason: "truncated-payload"
        }
    );
    assert_eq!(
        refused(&world, &trailing),
        E::Malformed {
            document: Document::Bundle,
            reason: "trailing-bytes"
        }
    );
    assert_eq!(
        refused(&world, &magic),
        E::Malformed {
            document: Document::Bundle,
            reason: "magic"
        }
    );
    assert_eq!(
        refused(&world, &good[..8]),
        E::Malformed {
            document: Document::Bundle,
            reason: "truncated-length"
        }
    );

    let table = r#"{"schema":1,"entries":[{"name":"../../bin/dctx","bytes":1,"sha256":"ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"}]}"#;
    let mut traversal = MAGIC.to_vec();
    traversal.extend_from_slice(
        &u32::try_from(table.len())
            .expect("fixture table length")
            .to_le_bytes(),
    );
    traversal.extend_from_slice(table.as_bytes());
    traversal.push(b'a');
    assert_eq!(
        refused(&world, &traversal),
        E::Malformed {
            document: Document::Bundle,
            reason: "entry-name"
        }
    );
}

#[test]
fn bundle_contents_must_match_the_signed_target() {
    let world = World::new();
    let release = Release::git("1.0.0", 1);
    let script: &[u8] = b"#!/bin/sh\nrm -rf ~\n";
    assert_eq!(
        refused(
            &world,
            &release.bundle_with(&world.official, &[("post-install.sh", script)])
        ),
        E::Malformed {
            document: Document::Bundle,
            reason: "undeclared-payload"
        }
    );

    let mut both = Release::git("1.0.0", 1);
    both.artifacts.push((
        "x86_64-unknown-linux-gnu".into(),
        "dctx-plugin-git-linux".into(),
        b"elf".to_vec(),
    ));
    let foreign: &[u8] = b"elf";
    assert_eq!(
        refused(
            &world,
            &both.bundle_with(&world.official, &[("dctx-plugin-git-linux", foreign)])
        ),
        E::TargetMismatch
    );

    let mut linux_first = both.clone();
    linux_first.artifacts.swap(0, 1);
    assert_eq!(
        refused(&world, &linux_first.bundle(&world.official)),
        E::TargetMismatch
    );

    let mut resources = Release::git("1.0.0", 1);
    resources.resources = vec![("git-help.txt".into(), b"usage".to_vec())];
    let manifest = resources.manifest();
    let signature = resources.signature(&world.official);
    let missing = OfflineBundle::encode(&[
        ("plugin.json", manifest.as_slice()),
        ("plugin.json.sig", signature.as_slice()),
        (resources.artifacts[0].1.as_str(), resources.executable()),
    ]);
    assert_eq!(
        refused(&world, &missing),
        E::Malformed {
            document: Document::Bundle,
            reason: "missing-payload"
        }
    );
}

#[test]
fn bundle_trust_failures_match_remote_installs() {
    let world = World::new();
    let release = Release::git("1.0.0", 1);
    assert_eq!(
        refused(&world, &release.bundle(&test_key(7))),
        E::Signature(VerificationError::EquationMismatch)
    );
    let manifest = release.manifest();
    let unsigned = OfflineBundle::encode(&[
        ("plugin.json", manifest.as_slice()),
        (release.artifacts[0].1.as_str(), release.executable()),
    ]);
    assert_eq!(
        refused(&world, &unsigned),
        E::Malformed {
            document: Document::Bundle,
            reason: "missing-manifest"
        }
    );
    let signature = release.signature(&world.official);
    let swapped: &[u8] = b"other bytes";
    let swapped = OfflineBundle::encode(&[
        ("plugin.json", manifest.as_slice()),
        ("plugin.json.sig", signature.as_slice()),
        (release.artifacts[0].1.as_str(), swapped),
    ]);
    assert_eq!(
        refused(&world, &swapped),
        E::SizeMismatch {
            expected: release.executable().len() as u64
        }
    );
}

#[test]
fn bundle_storage_limit_is_checked_first() {
    let mut world = World::new();
    world.policy.max_bundle_bytes = 64;
    let release = Release::git("1.0.0", 1);
    assert_eq!(
        refused(&world, &release.bundle(&world.official)),
        E::LimitExceeded { limit: 64 }
    );
}
