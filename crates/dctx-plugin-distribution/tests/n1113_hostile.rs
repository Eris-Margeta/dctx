// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile distribution inputs. Every byte decoder sees empty/one-byte input,
//! invalid UTF-8 and every truncation of a small valid document; typed APIs get boundary values.
//! Signatures use fixed test seeds, time is injected, and network ports are in-memory only.

mod common;

use common::*;
use dctx_plugin_distribution::domain::bundle::{MAGIC, MAX_ENTRIES, MAX_TABLE_BYTES};
use dctx_plugin_distribution::domain::digest::PayloadCheck;
use dctx_plugin_distribution::domain::index::INDEX_LIMITS;
use dctx_plugin_distribution::domain::origin::is_private_address;
use dctx_plugin_distribution::domain::registry_codec;
use dctx_plugin_distribution::domain::signature::{INDEX_DOMAIN, MANIFEST_DOMAIN, verify_detached};
use dctx_plugin_distribution::domain::sums::{MAX_SUMS_BYTES, check_sums};
use dctx_plugin_distribution::fakes::{Fault, MemoryStore};
use dctx_plugin_distribution::*;
use dctx_plugin_protocol::{DecodeLimits, Json, decode_object, to_canonical};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

fn json(bytes: &[u8]) -> Json {
    decode_object(
        bytes,
        DecodeLimits {
            max_bytes: 16 << 20,
            max_members: 1 << 20,
            ..DecodeLimits::WIRE
        },
    )
    .unwrap()
}
fn member_mut<'a>(value: &'a mut Json, key: &str) -> &'a mut Json {
    let Json::Object(members) = value else {
        panic!("object")
    };
    &mut members.iter_mut().find(|(name, _)| name == key).unwrap().1
}
fn array_mut(value: &mut Json) -> &mut Vec<Json> {
    let Json::Array(items) = value else {
        panic!("array")
    };
    items
}
fn version(seed: u8) -> InstalledVersion {
    InstalledVersion {
        version: Version::parse(&format!("{seed}.0.0")).unwrap(),
        release_sequence: u64::from(seed),
        digest: Sha256Hex::from_bytes([seed; 32]),
        manifest_sha256: Sha256Hex::from_bytes([seed; 32]),
        signer: Sha256Hex::from_bytes([42; 32]),
        capabilities: CapabilitySet::default(),
        quarantined: false,
    }
}
fn registry() -> Registry {
    let mut registry = Registry::default();
    registry
        .record_install(&id(GIT), Origin::Installed, version(1))
        .unwrap();
    registry
        .activate(&id(GIT), &version(1).digest, &TrustStore::new())
        .unwrap();
    registry
        .enable(
            &id(GIT),
            Some(&ConsentApproval {
                digest: version(1).digest,
                capabilities: CapabilitySet::default(),
                scope: ConsentScope::User,
            }),
            Confinement::Established,
            &TrustStore::new(),
        )
        .unwrap();
    registry
}
fn record_mut(value: &mut Json) -> &mut Json {
    &mut array_mut(member_mut(value, "plugins"))[0]
}

#[test]
fn document_parsers_reject_empty_every_one_byte_and_invalid_utf8() {
    let key = test_key(1);
    for bytes in std::iter::once(Vec::new())
        .chain((0..=255).map(|b| vec![b]))
        .chain([b"{\"x\":\"\xff\"}".to_vec(), b"\xef\xbb\xbf{}".to_vec()])
    {
        assert!(Manifest::parse(&bytes).is_err());
        assert!(OfflineBundle::parse(&bytes, u64::MAX).is_err());
        assert!(registry_codec::decode(&bytes).is_err());
        let signature = sign(INDEX_DOMAIN, &bytes, &key);
        assert!(VerifiedIndex::verify(&bytes, &signature, &public(&key)).is_err());
        assert!(
            TrustStore::new()
                .authenticate(&bytes, &[], &id(GIT))
                .is_err()
        );
    }
}

#[test]
fn manifest_and_registry_every_byte_truncation_is_safe() {
    let manifest = Release::git("1.0.0", 1).manifest();
    let canonical = to_canonical(&json(&manifest));
    for end in 0..canonical.len() {
        assert!(
            Manifest::parse(&canonical[..end]).is_err(),
            "manifest offset {end}"
        );
    }
    assert_eq!(Manifest::parse(&manifest).unwrap().bytes(), manifest);
    let registry = registry();
    let bytes = registry_codec::encode(&registry);
    for end in 0..bytes.len() {
        assert!(
            registry_codec::decode(&bytes[..end]).is_err(),
            "registry offset {end}"
        );
    }
    assert_eq!(registry_codec::decode(&bytes).unwrap(), registry);
    assert_eq!(registry_codec::encode(&registry), bytes);
    assert_eq!(
        registry_codec::encode(&registry_codec::decode(&bytes).unwrap()),
        bytes
    );
}

#[test]
fn signed_index_and_signature_every_truncation_is_safe() {
    let key = test_key(3);
    let (bytes, signature) = signed_index("", 1, &key);
    assert!(VerifiedIndex::verify(&bytes, &signature, &public(&key)).is_ok());
    for end in 0..bytes.len() {
        assert!(VerifiedIndex::verify(&bytes[..end], &signature, &public(&key)).is_err());
        let signed = sign(INDEX_DOMAIN, &bytes[..end], &key);
        assert!(VerifiedIndex::verify(&bytes[..end], &signed, &public(&key)).is_err());
    }
    for end in 0..signature.len() {
        assert!(verify_detached(INDEX_DOMAIN, &public(&key), &bytes, &signature[..end]).is_err());
    }
    assert!(verify_detached(INDEX_DOMAIN, &public(&key), &bytes, &[0; 65]).is_err());
    assert!(verify_detached(MANIFEST_DOMAIN, &public(&key), &bytes, &signature).is_err());
    for bytes in [b"".as_slice(), b"x", b"\xff\0"] {
        let signature = sign(MANIFEST_DOMAIN, bytes, &key);
        assert!(verify_detached(MANIFEST_DOMAIN, &public(&key), bytes, &signature).is_ok());
    }
}

#[test]
fn document_byte_caps_are_exact() {
    let release = Release::git("1.0.0", 1);
    let mut manifest = release.manifest();
    manifest.resize(DecodeLimits::MANIFEST.max_bytes, b' ');
    assert!(Manifest::parse(&manifest).is_ok());
    manifest.push(b' ');
    assert!(Manifest::parse(&manifest).is_err());
    let mut bytes = registry_codec::encode(&Registry::default());
    bytes.resize(16 << 20, b' ');
    assert!(registry_codec::decode(&bytes).is_ok());
    bytes.push(b' ');
    assert!(registry_codec::decode(&bytes).is_err());
    let key = test_key(3);
    let (mut bytes, _) = signed_index("", 1, &key);
    bytes.resize(INDEX_LIMITS.max_bytes, b' ');
    let signature = sign(INDEX_DOMAIN, &bytes, &key);
    assert!(VerifiedIndex::verify(&bytes, &signature, &public(&key)).is_ok());
    bytes.push(b' ');
    assert!(VerifiedIndex::verify(&bytes, &signature, &public(&key)).is_err());
}

#[test]
fn bundle_every_truncation_tampering_and_storage_bound_is_safe() {
    for payload in [b"".as_slice(), b"x", b"\xff\0\xc0\x80"] {
        let encoded = OfflineBundle::encode(&[("payload", payload)]);
        assert_eq!(encoded, OfflineBundle::encode(&[("payload", payload)]));
        let parsed = OfflineBundle::parse(&encoded, encoded.len() as u64).unwrap();
        assert_eq!(parsed.get("payload").unwrap().bytes, payload);
        assert_eq!(parsed.entries().len(), 1);
        assert!(parsed.get("absent").is_none());
        assert!(OfflineBundle::parse(&encoded, encoded.len() as u64 - 1).is_err());
        for end in 0..encoded.len() {
            assert!(
                OfflineBundle::parse(&encoded[..end], u64::MAX).is_err(),
                "bundle offset {end}"
            );
        }
        let mut appended = encoded.clone();
        appended.push(0);
        assert!(OfflineBundle::parse(&appended, u64::MAX).is_err());
        if !payload.is_empty() {
            let mut changed = encoded;
            *changed.last_mut().unwrap() ^= 1;
            assert!(OfflineBundle::parse(&changed, u64::MAX).is_err());
        }
    }
}

#[test]
fn bundle_entry_and_table_caps_duplicates_reordering_and_name_escaping() {
    assert!(OfflineBundle::parse(&OfflineBundle::encode(&[]), u64::MAX).is_err());
    let names: Vec<_> = (0..=MAX_ENTRIES).map(|n| format!("file-{n}")).collect();
    let rows: Vec<_> = names.iter().map(|n| (n.as_str(), b"".as_slice())).collect();
    assert_eq!(
        OfflineBundle::parse(&OfflineBundle::encode(&rows[..MAX_ENTRIES]), u64::MAX)
            .unwrap()
            .entries()
            .len(),
        MAX_ENTRIES
    );
    assert!(OfflineBundle::parse(&OfflineBundle::encode(&rows), u64::MAX).is_err());
    for names in [["a", "a"], ["A", "a"], ["a", "A"]] {
        assert!(
            OfflineBundle::parse(
                &OfflineBundle::encode(&[(names[0], b"x"), (names[1], b"y")]),
                u64::MAX
            )
            .is_err()
        );
    }
    for rows in [
        [("b", b"y".as_slice()), ("a", b"x")],
        [("a", b"x"), ("b", b"y")],
    ] {
        let bundle = OfflineBundle::parse(&OfflineBundle::encode(&rows), u64::MAX).unwrap();
        assert_eq!(bundle.get("a").unwrap().bytes, b"x");
        assert_eq!(bundle.get("b").unwrap().bytes, b"y");
    }
    let small = OfflineBundle::encode(&[("a", b"x")]);
    let length = u32::from_le_bytes(small[8..12].try_into().unwrap()) as usize;
    let mut exact = MAGIC.to_vec();
    exact.extend_from_slice(&MAX_TABLE_BYTES.to_le_bytes());
    exact.extend_from_slice(&small[12..12 + length]);
    exact.resize(12 + MAX_TABLE_BYTES as usize, b' ');
    exact.push(b'x');
    assert!(OfflineBundle::parse(&exact, u64::MAX).is_ok());
    exact[8..12].copy_from_slice(&(MAX_TABLE_BYTES + 1).to_le_bytes());
    assert!(OfflineBundle::parse(&exact, u64::MAX).is_err());
    for name in ["\"", "a\\b", "a\nb", "é", "a\0b"] {
        let bytes = OfflineBundle::encode(&[(name, b"x")]);
        let len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        if !name.contains('\0') {
            let table = json(&bytes[12..12 + len]);
            assert_eq!(
                table.get("entries").unwrap().as_array().unwrap()[0]
                    .get("name")
                    .unwrap()
                    .as_str(),
                Some(name)
            );
        }
        assert!(OfflineBundle::parse(&bytes, u64::MAX).is_err());
    }
}

macro_rules! name_bound {
    ($test:ident, $ty:ty, $valid:expr) => {
        #[test]
        fn $test() {
            let valid: String = $valid;
            assert_eq!(valid.len(), 128);
            assert_eq!(<$ty>::parse(&valid).unwrap().as_str(), valid);
            assert!(<$ty>::parse(&(valid.clone() + "a")).is_err());
            for value in ["", "\0", "é", "a/b/c"] {
                assert!(<$ty>::parse(value).is_err(), "{value:?}");
            }
            for end in 0..=valid.len() {
                let _ = <$ty>::parse(&valid[..end]);
            }
            assert_eq!(<$ty>::parse(&valid), <$ty>::parse(&valid));
        }
    };
}
name_bound!(plugin_id_limits, PluginId, format!("p/{}", "a".repeat(126)));
name_bound!(file_name_limits, SafeFileName, "a".repeat(128));
name_bound!(key_id_limits, KeyId, "a".repeat(128));
name_bound!(
    version_limits,
    Version,
    format!("1.0.0+{}", "a".repeat(122))
);

#[test]
fn semver_prerelease_rules_agree_with_protocol_and_build_zeros_remain_valid() {
    for text in [
        "1.0.0-01",
        "1.0.0-rc.00",
        "1.0.0-0.01+build",
        "1.0.0-",
        "1.0.0+a..b",
    ] {
        assert!(Version::parse(text).is_err(), "{text}");
        assert!(dctx_plugin_protocol::records::scalars::SemVer::new(text).is_err());
    }
    for text in ["1.0.0-0", "1.0.0-rc.1+build.00", "1.0.0+01"] {
        assert!(Version::parse(text).is_ok());
    }
    for text in ["con", "NUL.txt", "COM9", "lpt1.ext", ".", "..", "a.", "a/b"] {
        assert!(SafeFileName::parse(text).is_err());
    }
    assert_eq!(SafeFileName::parse("AbC").unwrap().folded(), "abc");
    assert_eq!(id("p/a").components(), ("p", "a"));
    for target in [
        Target::AppleArm64,
        Target::AppleX64,
        Target::LinuxX64,
        Target::LinuxArm64,
        Target::WindowsX64,
    ] {
        assert_eq!(Target::parse(target.as_str()).unwrap(), target);
    }
    assert!(Target::parse("").is_err());
    assert!(Target::parse("x").is_err());
}

#[test]
fn digest_and_payload_stream_boundaries_include_every_partition() {
    let sample = b"a\xff\0\xc0\x80z";
    for end in 0..=sample.len() {
        let bytes = &sample[..end];
        let digest = Sha256Hex::of(bytes).unwrap();
        assert_eq!(
            Sha256Hex::parse(&digest.to_string(), Document::Bundle)
                .unwrap()
                .as_bytes(),
            digest.as_bytes()
        );
        PayloadCheck::whole(bytes, end as u64, digest).unwrap();
        for split in 0..=end {
            let mut check = PayloadCheck::new(end as u64, digest);
            check.update(&bytes[..split]).unwrap();
            check.update(&[]).unwrap();
            check.update(&bytes[split..]).unwrap();
            check.finish().unwrap();
        }
        assert!(PayloadCheck::whole(bytes, end as u64 + 1, digest).is_err());
        if end > 0 {
            assert!(PayloadCheck::whole(bytes, end as u64 - 1, digest).is_err());
        }
        let mut check = PayloadCheck::new(end as u64, digest);
        check.update(bytes).unwrap();
        assert!(check.update(b"x").is_err());
        // An update refusal consumes no bytes; the accepted prefix remains valid.
        check.finish().unwrap();
    }
    for text in [
        "".into(),
        "a".into(),
        "a".repeat(63),
        "a".repeat(65),
        "é".repeat(32),
        "A".repeat(64),
        "g".repeat(64),
    ] {
        assert!(Sha256Hex::parse(&text, Document::Bundle).is_err());
    }
    let hex = Sha256Hex::of(sample).unwrap().to_string();
    for end in 0..hex.len() {
        assert!(Sha256Hex::parse(&hex[..end], Document::Bundle).is_err());
    }
    assert_ne!(fingerprint(&[1; 32]), fingerprint(&[2; 32]));
}

#[test]
fn sums_every_truncation_duplicate_reordering_invalid_utf8_and_byte_cap() {
    let mut release = Release::git("1.0.0", 1);
    release
        .resources
        .push(("data".into(), b"resource".to_vec()));
    let manifest = Manifest::parse(&release.manifest()).unwrap();
    let lines: Vec<_> = manifest.artifacts[0]
        .payloads()
        .map(|p| format!("{}  {}", p.sha256, p.file.as_str()))
        .collect();
    let sums = lines.join("\n");
    for end in 0..sums.len() {
        assert!(check_sums(&manifest, &sums.as_bytes()[..end]).is_err());
    }
    assert!(check_sums(&manifest, sums.as_bytes()).is_ok());
    assert!(
        check_sums(
            &manifest,
            lines
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
                .as_bytes()
        )
        .is_ok()
    );
    assert!(check_sums(&manifest, format!("{sums}\n{sums}").as_bytes()).is_err());
    for bad in [b"".as_slice(), b"x", b"\xff"] {
        assert!(check_sums(&manifest, bad).is_err());
    }
    let mut exact = sums.into_bytes();
    exact.resize(MAX_SUMS_BYTES as usize, b'\n');
    assert!(check_sums(&manifest, &exact).is_ok());
    exact.push(b'\n');
    assert!(check_sums(&manifest, &exact).is_err());
}

#[test]
fn capabilities_validate_caller_built_trees_and_canonical_duplicates() {
    let value = json(NARROW.as_bytes());
    let caps = CapabilitySet::from_manifest(&value).unwrap();
    assert_eq!(caps.len(), 1);
    assert!(!caps.is_empty());
    assert!(caps.is_within(&caps));
    assert!(caps.expansion_over(&caps).is_empty());
    let mut reordered = value.clone();
    let Json::Object(members) = &mut reordered else {
        unreachable!()
    };
    members.reverse();
    assert_eq!(CapabilitySet::from_manifest(&reordered).unwrap(), caps);
    let mut dup = value.clone();
    let required = array_mut(member_mut(&mut dup, "required"));
    required.push(required[0].clone());
    assert!(CapabilitySet::from_manifest(&dup).is_err());
    let mut dup_key = value.clone();
    let Json::Object(members) = &mut dup_key else {
        unreachable!()
    };
    members.push(members[0].clone());
    assert!(CapabilitySet::from_manifest(&dup_key).is_err());
    let mut bad_entry = value.clone();
    let entry = &mut array_mut(member_mut(&mut bad_entry, "required"))[0];
    let Json::Object(members) = entry else {
        unreachable!()
    };
    members.push(members[0].clone());
    assert!(CapabilitySet::from_manifest(&bad_entry).is_err());
    for n in [0, 1, 256, 257] {
        let mut value = json(br#"{"required":[],"optional":[],"outputs":[],"network":[]}"#);
        *member_mut(&mut value, "required") = Json::Array(
            (0..n)
                .map(|i| Json::Object(vec![("n".into(), Json::Int(i))]))
                .collect(),
        );
        assert_eq!(CapabilitySet::from_manifest(&value).is_ok(), n <= 256);
    }
    for bad in [
        Json::Null,
        Json::Str("x".into()),
        Json::Array(vec![]),
        Json::Object(vec![]),
    ] {
        assert!(CapabilitySet::from_manifest(&bad).is_err());
    }
}

#[test]
fn consent_signer_scope_and_expansion_checks_are_repeatable() {
    let caps = CapabilitySet::from_manifest(&json(NARROW.as_bytes())).unwrap();
    let wide = CapabilitySet::from_manifest(&json(WIDE.as_bytes())).unwrap();
    let consent = ConsentRecord {
        digest: version(1).digest,
        signer: version(1).signer,
        capabilities: caps.clone(),
        scope: ConsentScope::Project("project".into()),
    };
    assert!(consent.covers(&consent.signer, &caps));
    assert!(!consent.covers(&consent.signer, &wide));
    assert!(!consent.covers(&version(2).digest, &caps));
    assert_eq!(
        consent.derive(version(2).digest, &consent.signer, &caps),
        consent.derive(version(2).digest, &consent.signer, &caps)
    );
    assert!(
        consent
            .derive(version(2).digest, &consent.signer, &wide)
            .is_none()
    );
}

#[test]
fn manifest_duplicate_reordered_artifacts_and_cross_target_filename_conflicts() {
    let release = Release::git("1.0.0", 1);
    let bytes = release.manifest();
    let mut value = json(&bytes);
    let Json::Object(members) = &mut value else {
        unreachable!()
    };
    members.reverse();
    let reordered = Manifest::parse(&to_canonical(&value)).unwrap();
    assert_eq!(
        reordered.artifacts,
        Manifest::parse(&bytes).unwrap().artifacts
    );
    let artifacts = array_mut(member_mut(&mut value, "artifacts"));
    artifacts.push(artifacts[0].clone());
    assert!(Manifest::parse(&to_canonical(&value)).is_err());
    for conflict in [false, true] {
        let mut release = release.clone();
        let (_, name, bytes) = release.artifacts[0].clone();
        release.artifacts.push((
            Target::LinuxX64.as_str().into(),
            name,
            if conflict {
                b"different".to_vec()
            } else {
                bytes
            },
        ));
        assert_eq!(Manifest::parse(&release.manifest()).is_err(), conflict);
    }
    let mut release = release;
    release
        .resources
        .push(("PLUGIN.JSON".into(), b"x".to_vec()));
    assert!(Manifest::parse(&release.manifest()).is_err());
}

#[test]
fn registry_rejects_duplicate_versions_leases_ids_and_capabilities() {
    let mut original = registry();
    let admission = original
        .admit(&id(GIT), &TrustStore::new(), 0, None)
        .unwrap();
    let bytes = registry_codec::encode(&original);
    for field in ["versions", "leases"] {
        let mut value = json(&bytes);
        let items = array_mut(member_mut(record_mut(&mut value), field));
        items.push(items[0].clone());
        assert!(
            registry_codec::decode(&to_canonical(&value)).is_err(),
            "{field}"
        );
    }
    let mut orphan = json(&bytes);
    let leases = array_mut(member_mut(record_mut(&mut orphan), "leases"));
    *member_mut(&mut leases[0], "digest") = Json::str(&version(2).digest.to_string());
    assert!(registry_codec::decode(&to_canonical(&orphan)).is_err());
    let mut reused_sequence = json(&bytes);
    let versions = array_mut(member_mut(record_mut(&mut reused_sequence), "versions"));
    let mut other = versions[0].clone();
    *member_mut(&mut other, "digest") = Json::str(&version(2).digest.to_string());
    versions.push(other);
    assert!(registry_codec::decode(&to_canonical(&reused_sequence)).is_err());
    let mut value = json(&bytes);
    let plugins = array_mut(member_mut(&mut value, "plugins"));
    plugins.push(plugins[0].clone());
    assert!(registry_codec::decode(&to_canonical(&value)).is_err());
    let mut value = json(&bytes);
    let versions = array_mut(member_mut(record_mut(&mut value), "versions"));
    *member_mut(&mut versions[0], "capabilities") =
        Json::Array(vec![json(br#"{"section":"required","entry":{}}"#); 2]);
    assert!(registry_codec::decode(&to_canonical(&value)).is_err());
    original.release(&admission);
    original.release(&admission);
    assert!(original.get(&id(GIT)).unwrap().leases.is_empty());
}

#[test]
fn maximum_lease_counts_refuse_overflow_and_disable_does_not_panic() {
    let mut original = registry();
    original
        .record_install(&id(GIT), Origin::Installed, version(2))
        .unwrap();
    original
        .admit(&id(GIT), &TrustStore::new(), 0, None)
        .unwrap();
    let mut value = json(&registry_codec::encode(&original));
    let leases = array_mut(member_mut(record_mut(&mut value), "leases"));
    *member_mut(&mut leases[0], "count") = Json::Int(i64::from(u32::MAX));
    let mut max = registry_codec::decode(&to_canonical(&value)).unwrap();
    let before = registry_codec::encode(&max);
    assert!(matches!(
        max.admit(&id(GIT), &TrustStore::new(), 0, None),
        Err(DistributionError::LimitExceeded { .. })
    ));
    assert_eq!(registry_codec::encode(&max), before);
    assert_eq!(max.disable(&id(GIT)).unwrap(), u32::MAX);
    let mut value = json(&before);
    let leases = array_mut(member_mut(record_mut(&mut value), "leases"));
    let mut other = leases[0].clone();
    *member_mut(&mut other, "digest") = Json::str(&version(2).digest.to_string());
    leases.push(other);
    let mut max = registry_codec::decode(&to_canonical(&value)).unwrap();
    assert!(matches!(
        max.disable(&id(GIT)),
        Err(DistributionError::LimitExceeded { .. })
    ));
}

#[test]
fn registry_management_empty_missing_repeated_and_reordered_calls_are_safe() {
    let trust = TrustStore::new();
    let id = id(GIT);
    let digest = version(1).digest;
    let mut empty = Registry::default();
    assert!(empty.get(&id).is_none());
    assert_eq!(empty.records().count(), 0);
    assert!(empty.activate(&id, &digest, &trust).is_err());
    assert!(empty.rollback(&id, &digest, "reason", &trust).is_err());
    assert!(empty.disable(&id).is_err());
    assert!(empty.remove_plugin(&id).is_err());
    assert!(empty.quarantine(&id, &digest).is_err());
    assert!(empty.remove_version(&id, &digest).is_err());
    assert!(empty.admit(&id, &trust, 0, None).is_err());
    assert!(empty.status(&id, &trust, 0).is_err());
    let mut registry = registry();
    let bytes = registry_codec::encode(&registry);
    registry
        .record_install(&id, Origin::Installed, version(1))
        .unwrap();
    registry.activate(&id, &digest, &trust).unwrap();
    assert_eq!(registry_codec::encode(&registry), bytes);
    assert!(registry.rollback(&id, &digest, "", &trust).is_err());
    assert!(registry.rollback(&id, &digest, " \n", &trust).is_err());
    let admission = registry.admit(&id, &trust, 0, None).unwrap();
    assert!(registry.remove_version(&id, &digest).is_err());
    registry.release(&admission);
    assert_eq!(registry.disable(&id).unwrap(), 0);
    let bytes = registry_codec::encode(&registry);
    registry.disable(&id).unwrap();
    assert_eq!(registry_codec::encode(&registry), bytes);
    assert!(registry.admit(&id, &trust, 0, None).is_err());
    registry.remove_plugin(&id).unwrap();
    assert!(registry.remove_plugin(&id).unwrap().is_empty());
    registry.register_bundled(&id, version(2));
    assert_eq!(registry.get(&id).unwrap().enablement, Enablement::Disabled);
}

#[test]
fn trust_approval_duplicates_revocations_and_freshness_are_deterministic() {
    let mut trust = TrustStore::new();
    let key = test_key(1);
    let key_id = KeyId::parse("fixture").unwrap();
    let signer = fingerprint(&public(&key));
    assert!(
        trust
            .approve_key(
                key_id.clone(),
                public(&key),
                &version(1).digest,
                TrustSource::Approved
            )
            .is_err()
    );
    trust
        .approve_key(key_id.clone(), public(&key), &signer, TrustSource::Approved)
        .unwrap();
    assert_eq!(
        trust.pin_official(key_id.clone(), public(&key)).unwrap(),
        signer
    );
    assert!(trust.pin_official(key_id, public(&test_key(2))).is_err());
    trust.pin_publisher(id(GIT), signer);
    trust.pin_publisher(id(GIT), signer);
    assert_eq!(trust.publisher_pin(&id(GIT)), Some(&signer));
    trust.record_revocations([signer, signer], [version(1).digest, version(1).digest], 10);
    trust.record_revocations([], [], 9);
    assert!(trust.is_key_revoked(&signer));
    assert!(trust.is_artifact_revoked(&version(1).digest));
    assert_eq!(trust.revocation_age(11), Some(1));
    assert!(trust.check_freshness(11, Some(1)).is_ok());
    assert!(trust.check_freshness(12, Some(1)).is_err());
    assert!(trust.check_freshness(u64::MAX, None).is_ok());
}

#[test]
fn authentication_refuses_identity_change_mutation_and_revoked_keys() {
    let world = World::new();
    let release = Release::git("1.0.0", 1);
    let bytes = release.manifest();
    let signature = release.signature(&world.official);
    assert_eq!(
        world
            .trust
            .authenticate(&bytes, &signature, &id(GIT))
            .unwrap(),
        world
            .trust
            .authenticate(&bytes, &signature, &id(GIT))
            .unwrap()
    );
    assert!(
        world
            .trust
            .authenticate(&bytes, &signature, &id("other/plugin"))
            .is_err()
    );
    let mut changed = bytes;
    changed.push(b' ');
    assert!(
        world
            .trust
            .authenticate(&changed, &signature, &id(GIT))
            .is_err()
    );
    let mut trust = world.trust.clone();
    trust.record_revocations([fingerprint(&public(&world.official))], [], NOW);
    assert!(
        trust
            .authenticate(&release.manifest(), &signature, &id(GIT))
            .is_err()
    );
}

#[test]
fn index_duplicate_reordered_entries_and_selection_are_stable() {
    let key = test_key(2);
    let first = Release::git("1.0.0", 1);
    let mut second = Release::git("2.0.0", 2);
    second.id = "other/plugin".into();
    let entry = index_entry(&first, &key);
    let duplicate = format!("{entry},{entry}");
    let (bytes, signature) = signed_index(&duplicate, 1, &key);
    assert!(VerifiedIndex::verify(&bytes, &signature, &public(&key)).is_err());
    let other = index_entry(&second, &key).replace("fixture/dctx-plugin-git", "fixture/other");
    for entries in [format!("{entry},{other}"), format!("{other},{entry}")] {
        let (bytes, signature) = signed_index(&entries, 1, &key);
        let index = VerifiedIndex::verify(&bytes, &signature, &public(&key)).unwrap();
        assert_eq!(index.search("").len(), 2);
        assert_eq!(index.search("git protection").len(), 2);
        assert!(index.search("no-such-entry").is_empty());
        let entry = index.entry(&id(GIT), &TrustStore::new()).unwrap();
        assert_eq!(
            entry.select(None).unwrap(),
            entry
                .select(Some(&Version::parse("1.0.0").unwrap()))
                .unwrap()
        );
        assert!(
            entry
                .select(Some(&Version::parse("9.0.0").unwrap()))
                .is_err()
        );
        assert!(index.check_newer_than(Some(2)).is_err());
        assert!(index.check_newer_than(Some(1)).is_ok());
    }
    for text in ["", "a", "a\n\u{202e}\\b", "é"] {
        assert_eq!(escape_display(text), escape_display(text));
    }
    assert_eq!(escape_display("\u{1b}\u{202e}"), "\\u{001b}\\u{202e}");
}

#[test]
fn origin_url_bounds_truncation_reordering_and_peer_classification_need_no_network() {
    let prefix = "https://203.0.113.7/";
    let exact = format!("{prefix}{}", "a".repeat(2048 - prefix.len()));
    let url = HttpsUrl::parse(&exact).unwrap();
    assert_eq!(url.to_string(), exact);
    assert!(HttpsUrl::parse(&(exact + "a")).is_err());
    for text in [
        "",
        "x",
        "https://",
        "https://203.0.113.7/#x",
        "https://a@203.0.113.7/",
        "https://203.0.113.7:99999/",
        "https://203.0.113.7/é",
        "https://203.0.113.7/\0",
    ] {
        assert!(HttpsUrl::parse(text).is_err());
    }
    for end in 0..=prefix.len() {
        let _ = HttpsUrl::parse(&prefix[..end]);
    }
    let selected = HttpsUrl::parse("https://203.0.113.7/a/plugin.json?x=y").unwrap();
    let sibling = selected.sibling(&SafeFileName::parse("data").unwrap());
    assert_eq!(sibling.to_string(), "https://203.0.113.7/a/data");
    let policy = OriginPolicy::default();
    assert!(policy.check_url(&selected).is_ok());
    assert!(policy.check_redirect(&selected, &sibling).is_ok());
    for peer in [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::from([0; 16])),
    ] {
        assert!(is_private_address(peer));
        assert!(policy.check_peer(peer).is_err());
        assert!(
            OriginPolicy {
                allow_private: true,
                ..policy.clone()
            }
            .check_peer(peer)
            .is_ok()
        );
    }
    assert!(!is_private_address(IpAddr::V4(Ipv4Addr::new(
        203, 0, 113, 7
    ))));
}

#[test]
fn offline_install_every_bundle_truncation_is_atomic_and_never_uses_network() {
    let world = World::new();
    let release = Release::git("1.0.0", 1);
    let bytes = release.bundle(&world.official);
    for end in 0..bytes.len() {
        assert!(
            world
                .dist()
                .install_bundle(&id(GIT), &bytes[..end])
                .is_err()
        );
        assert!(world.store.installed().unwrap().is_empty());
        assert!(world.store.staged().unwrap().is_empty());
    }
    let report = world.dist().install_bundle(&id(GIT), &bytes).unwrap();
    let before = world.store.load_registry().unwrap();
    assert_eq!(
        world.dist().install_bundle(&id(GIT), &bytes).unwrap(),
        report
    );
    assert_eq!(world.store.load_registry().unwrap(), before);
    assert!(world.net.requests().is_empty());
    assert!(
        world
            .dist()
            .verify(&id(GIT))
            .unwrap()
            .iter()
            .all(|o| o.result.is_ok())
    );
}

#[test]
fn facade_failure_recovery_management_and_byte_caps_are_repeatable() {
    let mut world = World::new();
    let release = Release::git("1.0.0", 1);
    let bytes = release.bundle(&world.official);
    world.policy.max_bundle_bytes = bytes.len() as u64 - 1;
    assert!(world.dist().install_bundle(&id(GIT), &bytes).is_err());
    world.policy.max_bundle_bytes += 1;
    world.policy.max_artifact_bytes = release.executable().len() as u64 - 1;
    assert!(world.dist().install_bundle(&id(GIT), &bytes).is_err());
    world.policy.max_artifact_bytes += 1;
    for fault in [Fault::Stage, Fault::Publish, Fault::SaveRegistry] {
        world.store.inject(fault);
        assert!(world.dist().install_bundle(&id(GIT), &bytes).is_err());
        world.dist().recover().unwrap();
        assert_eq!(world.dist().recover().unwrap(), RecoveryReport::default());
    }
    let report = world.dist().install_bundle(&id(GIT), &bytes).unwrap();
    assert!(world.dist().admit(&id(GIT)).is_err());
    world.dist().activate(&id(GIT), &report.key.digest).unwrap();
    let approval = ConsentApproval {
        digest: report.key.digest,
        capabilities: report.capabilities,
        scope: ConsentScope::User,
    };
    world
        .dist()
        .enable(&id(GIT), Some(&approval), Confinement::Established)
        .unwrap();
    let lease = world.dist().admit(&id(GIT)).unwrap();
    assert!(world.dist().remove(&id(GIT)).is_err());
    world.dist().release(&lease).unwrap();
    world.dist().release(&lease).unwrap();
    assert_eq!(world.dist().disable(&id(GIT)).unwrap(), 0);
    assert_eq!(world.dist().disable(&id(GIT)).unwrap(), 0);
    world.dist().remove(&id(GIT)).unwrap();
    assert_eq!(world.dist().list().unwrap().len(), 1);
    assert!(world.net.requests().is_empty());
}

#[test]
fn shared_store_locking_and_immutable_codecs_are_deterministic_under_concurrency() {
    let store = MemoryStore::new();
    let held = store.try_lock().unwrap();
    let registry = registry();
    let bytes = registry_codec::encode(&registry);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    assert_eq!(store.try_lock(), Err(DistributionError::Busy));
                    assert_eq!(
                        registry_codec::encode(&registry_codec::decode(&bytes).unwrap()),
                        bytes
                    );
                    assert_eq!(Sha256Hex::of(&bytes), Sha256Hex::of(&bytes));
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
    });
    store.unlock(LockToken::new(held.value() + 1));
    assert_eq!(store.try_lock(), Err(DistributionError::Busy));
    store.unlock(held);
    let next = store.try_lock().unwrap();
    assert_ne!(next, held);
    store.unlock(next);
}

#[test]
fn identical_artifact_digest_cannot_alias_different_release_metadata() {
    let mut registry = registry();
    let before = registry_codec::encode(&registry);
    let mut changed = version(1);
    changed.version = Version::parse("2.0.0").unwrap();
    assert_eq!(
        registry.record_install(&id(GIT), Origin::Installed, changed),
        Err(DistributionError::IntegrityFailure)
    );
    let mut changed = version(1);
    changed.signer = version(2).digest;
    assert_eq!(
        registry.record_install(&id(GIT), Origin::Installed, changed),
        Err(DistributionError::IntegrityFailure)
    );
    assert_eq!(registry_codec::encode(&registry), before);
}

#[test]
fn indexed_release_version_and_sequence_must_match_authenticated_manifest() {
    let world = World::new();
    let release = Release::git("1.0.0", 1);
    world.publish(&release, &world.official);
    let (bytes, signature) =
        signed_index(&index_entry(&release, &world.official), 1, &world.official);
    let valid = VerifiedIndex::verify(&bytes, &signature, &public(&world.official)).unwrap();
    for change_version in [false, true] {
        let mut index = valid.clone();
        let selected = &mut index.entries[0].releases[0];
        if change_version {
            selected.version = Version::parse("2.0.0").unwrap();
        } else {
            selected.release_sequence = 2;
        }
        assert!(
            world
                .dist()
                .install_from_index(&index, &id(GIT), None)
                .is_err()
        );
        assert!(world.store.installed().unwrap().is_empty());
        assert!(world.store.staged().unwrap().is_empty());
    }
    assert!(
        world
            .dist()
            .install_from_index(&valid, &id(GIT), None)
            .is_ok()
    );
}

#[test]
fn every_distribution_failure_names_a_next_step_without_document_contents() {
    for error in [
        DistributionError::Malformed {
            document: Document::Manifest,
            reason: "fixture",
        },
        DistributionError::Malformed {
            document: Document::Registry,
            reason: "fixture",
        },
        DistributionError::Malformed {
            document: Document::Locator,
            reason: "fixture",
        },
        DistributionError::Signature(dctx_ed25519::VerificationError::EquationMismatch),
        DistributionError::UnknownKey,
        DistributionError::FingerprintMismatch,
        DistributionError::KeyRevoked,
        DistributionError::UnknownPublisher,
        DistributionError::SignerChanged,
        DistributionError::IdentityMismatch,
        DistributionError::ArtifactRevoked,
        DistributionError::Downgrade {
            candidate: 1,
            floor: 2,
        },
        DistributionError::Replay { sequence: 1 },
        DistributionError::UnsupportedTarget,
        DistributionError::TargetMismatch,
        DistributionError::SizeMismatch { expected: 1 },
        DistributionError::IntegrityFailure,
        DistributionError::SumsMismatch,
        DistributionError::ManifestDigestMismatch,
        DistributionError::LimitExceeded { limit: 1 },
        DistributionError::Origin(OriginRefusal::Syntax),
        DistributionError::Network(NetworkError::Interrupted),
        DistributionError::Busy,
        DistributionError::Interrupted,
        DistributionError::NotInstalled,
        DistributionError::NotActive,
        DistributionError::Quarantined,
        DistributionError::Disabled,
        DistributionError::ConsentRequired,
        DistributionError::ConfinementRequired,
        DistributionError::StaleRevocation { age_secs: None },
        DistributionError::StaleRevocation { age_secs: Some(1) },
        DistributionError::Leased,
        DistributionError::NotFound,
    ] {
        assert!(error.to_string().contains("; next: "), "{error:?}");
    }
    let error = Manifest::parse(b"PRIVATE-CONTENT").unwrap_err();
    assert!(!error.to_string().contains("PRIVATE-CONTENT"));
}

#[test]
fn facade_rollback_inactive_removal_and_bundled_registration_preserve_metadata() {
    let world = World::new();
    let first = Release::git("1.0.0", 1);
    let second = Release::git("2.0.0", 2);
    let old = world
        .dist()
        .install_bundle(&id(GIT), &first.bundle(&world.official))
        .unwrap();
    let new = world
        .dist()
        .install_bundle(&id(GIT), &second.bundle(&world.official))
        .unwrap();
    world.dist().activate(&id(GIT), &new.key.digest).unwrap();
    world
        .dist()
        .rollback(&id(GIT), &old.key.digest, "explicit-rollback")
        .unwrap();
    world
        .dist()
        .remove_version(&id(GIT), &new.key.digest)
        .unwrap();
    assert!(
        world
            .dist()
            .remove_version(&id(GIT), &new.key.digest)
            .is_err()
    );
    assert_eq!(
        world
            .dist()
            .registry()
            .unwrap()
            .get(&id(GIT))
            .unwrap()
            .high_water,
        2
    );
    world
        .dist()
        .register_bundled(&id("fixture/bundled"), version(3))
        .unwrap();
    let before = world.store.load_registry().unwrap();
    world
        .dist()
        .register_bundled(&id("fixture/bundled"), version(3))
        .unwrap();
    assert_eq!(world.store.load_registry().unwrap(), before);
}

#[test]
fn tampered_installed_payload_truncations_quarantine_without_execution() {
    let mut release = Release::git("1.0.0", 1);
    release.artifacts[0].2 = b"a\xff\0z".to_vec();
    for end in 0..release.executable().len() {
        let world = World::new();
        let report = world
            .dist()
            .install_bundle(&id(GIT), &release.bundle(&world.official))
            .unwrap();
        assert!(
            world
                .store
                .installed_files(&report.key)
                .unwrap()
                .contains(&release.artifacts[0].1)
        );
        world.store.tamper(
            &report.key,
            &release.artifacts[0].1,
            &release.executable()[..end],
        );
        assert!(world.dist().verify(&id(GIT)).unwrap()[0].result.is_err());
        let registry = world.dist().registry().unwrap();
        assert!(
            registry
                .get(&id(GIT))
                .unwrap()
                .version(&report.key.digest)
                .unwrap()
                .quarantined
        );
        assert!(world.dist().activate(&id(GIT), &report.key.digest).is_err());
        assert!(world.net.requests().is_empty());
    }
}
