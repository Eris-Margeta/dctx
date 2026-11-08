// SPDX-License-Identifier: Apache-2.0
//! Manifest reads return current verified bytes, refuse drift and honor omitted ancestors.
use dctx_core::budget::PackLimits;
use dctx_core::error::DctxError;
use dctx_core::facts::PackSnapshot;
use dctx_core::ids::{RelativePath, Timestamp};
use dctx_core::selection::{RuleTexts, SelectionPolicy};
use dctx_core::version::Version;
use dctx_pack::{encode_legacy, read_snapshot_file, scan, LegacyLimits, ScanOptions};
use dctx_ports::memory::{CancelFlag, MemorySource};

#[test]
fn manifest_read_reproduces_export_bytes_and_refuses_same_size_drift() {
    let cancel = CancelFlag::new();
    let mut source = MemorySource::new();
    source.insert_file("a.rs", b"fn old() {}\n", 1).unwrap();
    source.insert_file("empty.txt", b"", 1).unwrap();
    let policy = SelectionPolicy::from_texts(&RuleTexts::default());
    let scan = scan(
        &source,
        &cancel,
        ScanOptions {
            policy: &policy,
            debug_ignored: false,
            limits: PackLimits::unbounded(),
        },
    )
    .unwrap();
    let version = Version::new_v1();
    let expected = encode_legacy(&scan.tree, &version, LegacyLimits::default()).unwrap();
    let manifest = PackSnapshot::new(scan.tree, version.clone(), Timestamp(1), scan.digests)
        .unwrap()
        .into_manifest();
    let mut tree = manifest.tree().clone();
    assert!(tree
        .children
        .iter()
        .all(|child| child.file_content.is_none()));
    for child in &mut tree.children {
        child.file_content = Some(
            read_snapshot_file(
                &manifest,
                &RelativePath::parse(&child.relative_path).unwrap(),
                &source,
                &cancel,
                1024,
            )
            .unwrap(),
        );
    }
    assert_eq!(
        encode_legacy(&tree, &version, LegacyLimits::default()).unwrap(),
        expected
    );
    source.insert_file("a.rs", b"fn new() {}\n", 1).unwrap();
    assert_eq!(
        read_snapshot_file(
            &manifest,
            &RelativePath::parse("a.rs").unwrap(),
            &source,
            &cancel,
            1024
        ),
        Err(DctxError::SourceChanged)
    );
    assert_eq!(
        read_snapshot_file(
            &manifest,
            &RelativePath::parse("absent.rs").unwrap(),
            &source,
            &cancel,
            1024
        ),
        Err(DctxError::PolicyDenied)
    );
}

#[test]
fn manifest_read_honors_ancestor_policy_even_with_a_recorded_digest() {
    let mut source = MemorySource::new();
    source.insert_file("private/a.rs", b"body", 1).unwrap();
    let cancel = CancelFlag::new();
    let policy = SelectionPolicy::from_texts(&RuleTexts::default());
    let mut scan = scan(
        &source,
        &cancel,
        ScanOptions {
            policy: &policy,
            debug_ignored: false,
            limits: PackLimits::unbounded(),
        },
    )
    .unwrap();
    scan.tree.children[0].handling_decision = dctx_core::FileHandlingDecision::OmitContent;
    let manifest =
        PackSnapshot::new(scan.tree, Version::new_v1(), Timestamp(0), scan.digests).unwrap();
    assert_eq!(
        read_snapshot_file(
            &manifest,
            &RelativePath::parse("private/a.rs").unwrap(),
            &source,
            &cancel,
            1024
        ),
        Err(DctxError::PolicyDenied)
    );
}
