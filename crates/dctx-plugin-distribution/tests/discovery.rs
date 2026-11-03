// SPDX-License-Identifier: Apache-2.0
//! Discovery client: signed index verification, PL-X hostile fixtures, escaped search and install
//! from a verified index whose manifest digest and publisher pin are still checked independently.
//! PLUGINS.md §5.3 discovery, requirements I-INDEX and I-IDENTITY.

mod common;

use common::*;
use dctx_plugin_distribution::domain::signature::MANIFEST_DOMAIN;
use dctx_plugin_distribution::{DistributionError as E, Document, VerifiedIndex, escape_display};
use dctx_ed25519::VerificationError;

const PLX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins/index/invalid/");

fn plx(name: &str) -> Vec<u8> {
    std::fs::read(format!("{PLX}{name}.json")).unwrap()
}

#[test]
fn index_signature_uses_the_pinned_key_and_its_own_domain() {
    let index_key = test_key(3);
    let release = Release::git("1.0.0", 1);
    let (bytes, signature) = signed_index(&index_entry(&release, &test_key(1)), 4, &index_key);
    let index = VerifiedIndex::verify(&bytes, &signature, &public(&index_key)).unwrap();
    assert_eq!(index.sequence, 4);
    assert_eq!(
        VerifiedIndex::verify(&bytes, &signature, &public(&test_key(4))),
        Err(E::Signature(VerificationError::EquationMismatch))
    );
    let confused = sign(MANIFEST_DOMAIN, &bytes, &index_key);
    assert_eq!(VerifiedIndex::verify(&bytes, &confused, &public(&index_key)), Err(E::Signature(VerificationError::EquationMismatch)));
    assert_eq!(index.check_newer_than(Some(5)), Err(E::Downgrade { candidate: 4, floor: 5 }));
}

#[test]
fn plx_hostile_index_fixtures_are_refused_or_escaped() {
    let key = test_key(3);
    let verify = |bytes: &[u8]| VerifiedIndex::verify(bytes, &sign(dctx_plugin_distribution::domain::signature::INDEX_DOMAIN, bytes, &key), &public(&key));
    for (name, reason) in [
        ("duplicate-id", "duplicate-id"),
        ("same-repository", "duplicate-repository"),
        ("unsigned-metadata", "unknown-field"),
        ("alias-claim", "unknown-field"),
    ] {
        assert_eq!(verify(&plx(name)), Err(E::Malformed { document: Document::Index, reason }), "{name}");
    }
    assert!(matches!(verify(&plx("http-url")), Err(E::Origin(_))));
    assert!(matches!(verify(&plx("userinfo-url")), Err(E::Origin(_))));

    let world = World::new();
    let changed = verify(&plx("changed-publisher")).unwrap();
    assert_eq!(changed.entry(&id(GIT), &world.trust).map(|_| ()), Err(E::SignerChanged));

    let hostile = verify(&plx("hostile-display")).unwrap();
    let hits = hostile.search("hidden");
    assert_eq!(hits.len(), 1);
    assert!(!escape_display(&hits[0].summary).contains('\u{202e}'));
}

#[test]
fn install_from_index_checks_manifest_digest_and_fetches_nothing_during_search() {
    let world = World::new();
    let key = test_key(3);
    let release = Release::git("1.0.0", 1);
    let (bytes, signature) = signed_index(&index_entry(&release, &world.official), 1, &key);
    let index = VerifiedIndex::verify(&bytes, &signature, &public(&key)).unwrap();
    assert_eq!(index.search("git").len(), 1);
    assert!(world.net.requests().is_empty(), "search never fetches");

    world.publish(&release, &world.official);
    let mut altered = release.manifest();
    altered.push(b' ');
    world.net.serve(&release.url(), &altered);
    assert_eq!(world.dist().install_from_index(&index, &id(GIT), None).map(|_| ()), Err(E::ManifestDigestMismatch));
    world.net.serve(&release.url(), &release.manifest());
    let report = world.dist().install_from_index(&index, &id(GIT), None).unwrap();
    assert_eq!(report.key.digest, release.digest());
    assert_eq!(world.dist().install_from_index(&index, &id("dctx/none"), None).map(|_| ()), Err(E::NotFound));
}
