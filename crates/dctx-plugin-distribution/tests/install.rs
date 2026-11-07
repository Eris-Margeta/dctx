// SPDX-License-Identifier: Apache-2.0
//! Remote install through the fake network: SHA, size, signature, key, identity, target, revocation,
//! downgrade/replay, download limits, origins and interruption, each beside its positive control.
//! PLUGINS.md §5.2/§5.3, requirements I-INTEGRITY, I-SIGNATURE, I-TRUST, I-IDENTITY, I-ORIGIN.

mod common;

use common::*;
use dctx_ed25519::VerificationError;
use dctx_plugin_distribution::domain::signature::INDEX_DOMAIN;
use dctx_plugin_distribution::fakes::{FakeResponse, PUBLIC_PEER};
use dctx_plugin_distribution::{
    DistributionError as E, Enablement, HttpsUrl, KeyId, NetworkError, OriginRefusal, Sha256Hex,
    TrustSource, fingerprint,
};

fn dir(release: &Release) -> String {
    format!("{BASE}/{}", release.version)
}

fn artifact_url(release: &Release) -> String {
    format!("{}/{}", dir(release), release.artifacts[0].1)
}

fn nothing_installed(world: &World) {
    let registry = world.dist().registry().unwrap();
    assert!(
        registry
            .get(&id(GIT))
            .is_none_or(|record| record.versions.is_empty())
    );
    assert!(
        world.dist().recover().unwrap().discarded_staging.is_empty(),
        "refusal left staging behind"
    );
}

#[test]
fn verified_install_is_inactive_and_runs_nothing() {
    let world = World::new();
    let release = Release::git("1.0.0", 1);
    let report = world.publish_and_install(&release);
    assert_eq!(report.key.digest, release.digest());
    assert_eq!(report.signer, fingerprint(&public(&world.official)));
    assert_eq!(report.origins, vec!["releases.example.org".to_owned()]);
    let registry = world.dist().registry().unwrap();
    let record = registry.get(&id(GIT)).unwrap();
    assert_eq!(record.enablement, Enablement::NeverEnabled);
    assert_eq!(record.active, None, "install never activates");
    let files = world.store.installed_files(&report.key).unwrap();
    assert!(
        files.contains(&"plugin.json".to_owned()) && files.contains(&"plugin.json.sig".to_owned())
    );
    assert_eq!(world.dist().admit(&id(GIT)), Err(E::ConsentRequired));
}

#[test]
fn artifact_sha_and_size_failures_never_publish() {
    let cases: [(&[u8], E); 3] = [
        (
            b"#!/bin/sh\necho installed-code-ran-1.0.0 > /tmp/dctx-pl-i-markeX\n",
            E::IntegrityFailure,
        ),
        (b"short", E::SizeMismatch { expected: 0 }),
        (
            b"#!/bin/sh\necho installed-code-ran-1.0.0 > /tmp/dctx-pl-i-marker\nEXTRA",
            E::SizeMismatch { expected: 0 },
        ),
    ];
    for (served, expected) in cases {
        let world = World::new();
        let release = Release::git("1.0.0", 1);
        world.publish(&release, &world.official);
        world.net.serve(&artifact_url(&release), served);
        let error = world.install(&release).unwrap_err();
        match expected {
            E::SizeMismatch { .. } => assert_eq!(
                error,
                E::SizeMismatch {
                    expected: release.executable().len() as u64
                }
            ),
            other => assert_eq!(error, other),
        }
        nothing_installed(&world);
    }
}

#[test]
fn signature_failures_are_refused() {
    let release = Release::git("1.0.0", 1);
    let sig_url = format!("{}/plugin.json.sig", dir(&release));
    let other = test_key(9);
    let mut whitespace = release.manifest();
    whitespace.push(b'\n');
    let mut truncated = release.signature(&test_key(1));
    truncated.pop();
    let cases: Vec<(Box<dyn Fn(&World) + '_>, E)> = vec![
        (
            Box::new(|w: &World| w.net.serve(&sig_url, &release.signature(&other))),
            E::Signature(VerificationError::EquationMismatch),
        ),
        (
            Box::new(|w: &World| w.net.serve(&release.url(), &whitespace)),
            E::Signature(VerificationError::EquationMismatch),
        ),
        (
            Box::new(|w: &World| {
                w.net.serve(
                    &sig_url,
                    &sign(INDEX_DOMAIN, &release.manifest(), &w.official),
                )
            }),
            E::Signature(VerificationError::EquationMismatch),
        ),
        (
            Box::new(|w: &World| w.net.serve(&sig_url, &truncated)),
            E::Malformed {
                document: dctx_plugin_distribution::Document::Signature,
                reason: "signature-length",
            },
        ),
        (
            Box::new(|w: &World| w.net.serve(&sig_url, &[0u8; 65])),
            E::LimitExceeded { limit: 64 },
        ),
        (
            Box::new(|w: &World| w.net.unroute(&sig_url)),
            E::Network(NetworkError::NotFound),
        ),
    ];
    for (tamper, expected) in cases {
        let world = World::new();
        world.publish(&release, &world.official);
        tamper(&world);
        assert_eq!(world.install(&release).map(|_| ()), Err(expected));
        assert!(
            !world.net.requests().contains(&artifact_url(&release)),
            "artifact fetched before trust"
        );
        nothing_installed(&world);
    }
}

#[test]
fn key_and_identity_failures_are_refused() {
    let world = World::new();
    let mut unknown = Release::git("1.0.0", 1);
    unknown.key_id = "unknown-key".into();
    world.publish(&unknown, &world.official);
    assert_eq!(world.install(&unknown).map(|_| ()), Err(E::UnknownKey));

    let mut world = World::new();
    let rogue = test_key(5);
    world
        .trust
        .approve_key(
            KeyId::parse("rogue").unwrap(),
            public(&rogue),
            &fingerprint(&public(&rogue)),
            TrustSource::Approved,
        )
        .unwrap();
    let mut transferred = Release::git("1.0.0", 1);
    transferred.key_id = "rogue".into();
    world.publish(&transferred, &rogue);
    assert_eq!(
        world.install(&transferred).map(|_| ()),
        Err(E::SignerChanged),
        "renamed repo cannot take over identity"
    );

    let mut other = Release::git("1.0.0", 1);
    other.id = "dctx/other".into();
    world.publish(&other, &world.official);
    assert_eq!(world.install(&other).map(|_| ()), Err(E::IdentityMismatch));
    let url = HttpsUrl::parse(&other.url()).unwrap();
    assert_eq!(
        world
            .dist()
            .install_url(&id("dctx/other"), &url)
            .map(|_| ()),
        Err(E::UnknownPublisher)
    );
    world
        .trust
        .pin_publisher(id("dctx/other"), fingerprint(&public(&world.official)));
    assert!(
        world.dist().install_url(&id("dctx/other"), &url).is_ok(),
        "explicit approval is the positive control"
    );

    let mut world = World::new();
    world
        .trust
        .record_revocations([fingerprint(&public(&world.official))], [], NOW);
    let release = Release::git("1.0.0", 1);
    world.publish(&release, &world.official);
    assert_eq!(world.install(&release).map(|_| ()), Err(E::KeyRevoked));
}

#[test]
fn target_failures_are_refused() {
    let world = World::new();
    let mut linux = Release::git("1.0.0", 1);
    linux.artifacts[0].0 = "x86_64-unknown-linux-gnu".into();
    world.publish(&linux, &world.official);
    assert_eq!(world.install(&linux).map(|_| ()), Err(E::UnsupportedTarget));
    assert!(!world.net.requests().contains(&artifact_url(&linux)));
}

#[test]
fn revoked_downgraded_and_replayed_releases_are_refused() {
    let mut world = World::new();
    let v1 = Release::git("1.0.0", 1);
    world.trust.record_revocations([], [v1.digest()], NOW);
    world.publish(&v1, &world.official);
    assert_eq!(world.install(&v1).map(|_| ()), Err(E::ArtifactRevoked));
    assert!(!world.net.requests().contains(&artifact_url(&v1)));

    let world = World::new();
    let v2 = Release::git("2.0.0", 2);
    world.publish_and_install(&v2);
    world.dist().activate(&id(GIT), &v2.digest()).unwrap();
    world.publish(&v1, &world.official);
    assert_eq!(
        world.install(&v1).map(|_| ()),
        Err(E::Downgrade {
            candidate: 1,
            floor: 2
        })
    );
    let mut replay = Release::git("2.0.1", 2);
    replay.artifacts[0].2 = b"different bytes".to_vec();
    world.publish(&replay, &world.official);
    assert_eq!(
        world.install(&replay).map(|_| ()),
        Err(E::Replay { sequence: 2 })
    );
    assert_eq!(world.active(), Some(v2.digest()));
    let v3 = Release::git("3.0.0", 3);
    world.publish_and_install(&v3);
}

#[test]
fn download_limits_are_enforced_before_and_while_reading() {
    let mut world = World::new();
    world.policy.max_artifact_bytes = 8;
    let release = Release::git("1.0.0", 1);
    world.publish(&release, &world.official);
    assert_eq!(
        world.install(&release).map(|_| ()),
        Err(E::LimitExceeded { limit: 8 })
    );
    assert!(!world.net.requests().contains(&artifact_url(&release)));

    let world = World::new();
    world.publish(&release, &world.official);
    world.net.serve(&release.url(), &vec![b' '; 262_145]);
    assert_eq!(
        world.install(&release).map(|_| ()),
        Err(E::LimitExceeded { limit: 262_144 })
    );
}

#[test]
fn origins_redirects_and_peers_follow_policy() {
    let release = Release::git("1.0.0", 1);
    let cdn = "https://cdn.example.net/blob";
    let redirect = |world: &World, to: &str| {
        world
            .net
            .route(&artifact_url(&release), FakeResponse::Redirect(to.into()))
    };

    let world = World::new();
    world.publish(&release, &world.official);
    redirect(&world, cdn);
    world.net.serve(cdn, release.executable());
    assert_eq!(
        world.install(&release).map(|_| ()),
        Err(E::Origin(OriginRefusal::NotApproved))
    );
    let mut approved = World::new();
    approved.policy.origin.approved_redirects = vec!["cdn.example.net".into()];
    approved.publish(&release, &approved.official);
    redirect(&approved, cdn);
    approved.net.serve(cdn, release.executable());
    assert!(
        approved.install(&release).is_ok(),
        "approved redirect is the positive control"
    );

    for (to, refusal) in [
        (
            "https://169.254.169.254/latest",
            OriginRefusal::PrivateDestination,
        ),
        ("http://releases.example.org/x", OriginRefusal::Scheme),
        (
            "https://user@releases.example.org/x",
            OriginRefusal::Userinfo,
        ),
    ] {
        let world = World::new();
        world.publish(&release, &world.official);
        redirect(&world, to);
        assert_eq!(
            world.install(&release).map(|_| ()),
            Err(E::Origin(refusal)),
            "{to}"
        );
    }

    let world = World::new();
    world.publish(&release, &world.official);
    redirect(&world, &artifact_url(&release));
    assert_eq!(
        world.install(&release).map(|_| ()),
        Err(E::Origin(OriginRefusal::TooManyRedirects))
    );

    let world = World::new();
    world.publish(&release, &world.official);
    let rebinding = FakeResponse::Content {
        bytes: release.executable().to_vec(),
        peer: "10.0.0.5".parse().unwrap(),
        fail_after: None,
    };
    world.net.route(&artifact_url(&release), rebinding);
    assert_eq!(
        world.install(&release).map(|_| ()),
        Err(E::Origin(OriginRefusal::PrivateDestination))
    );
    assert_ne!(PUBLIC_PEER, "10.0.0.5".parse::<std::net::IpAddr>().unwrap());
}

#[test]
fn interrupted_download_leaves_nothing_and_retry_succeeds() {
    let world = World::new();
    let release = Release::git("1.0.0", 1);
    world.publish(&release, &world.official);
    let partial = FakeResponse::Content {
        bytes: release.executable().to_vec(),
        peer: PUBLIC_PEER,
        fail_after: Some(5),
    };
    world.net.route(&artifact_url(&release), partial);
    assert_eq!(
        world.install(&release).map(|_| ()),
        Err(E::Network(NetworkError::Interrupted))
    );
    nothing_installed(&world);
    world.net.route(
        &format!("{}/plugin.json", dir(&release)),
        FakeResponse::Fail(NetworkError::RateLimited),
    );
    assert_eq!(
        world.install(&release).map(|_| ()),
        Err(E::Network(NetworkError::RateLimited))
    );
    world.publish(&release, &world.official);
    assert!(world.install(&release).is_ok());
}

#[test]
fn sums_mirror_must_agree_with_the_signed_manifest() {
    let world = World::new();
    let release = Release::git("1.0.0", 1);
    world.publish(&release, &world.official);
    let sums_url = format!("{}/SHA256SUMS", dir(&release));
    world.net.serve(
        &sums_url,
        format!(
            "{}  {}\n",
            Sha256Hex::of(b"evil").unwrap(),
            release.artifacts[0].1
        )
        .as_bytes(),
    );
    assert_eq!(world.install(&release).map(|_| ()), Err(E::SumsMismatch));
    world.net.serve(
        &sums_url,
        format!("{}  {}\n", release.digest(), release.artifacts[0].1).as_bytes(),
    );
    assert!(world.install(&release).is_ok());
}
