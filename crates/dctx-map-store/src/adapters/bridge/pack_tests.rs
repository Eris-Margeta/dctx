//! FactStore roundtrip, corruption, cap refusal and view expiry.
//! Implements the frozen load/publish port on the comparison bridge.
//! Uses dctx-core snapshots. A failed publish leaves the previous generation visible.

use crate::{SqliteStore, VIEW_IDLE_MS, GIT_IDENTITY};
use dctx_core::datatypes::{DirContextTreeNode, NodeType};
use dctx_core::domain::FileHandlingDecision;
use dctx_core::error::DctxError;
use dctx_core::facts::{ContentDigests, PackSnapshot};
use dctx_core::ids::{Timestamp, ViewKey};
use dctx_core::version::Version;
use dctx_hash::Sha256;
use dctx_ports::FactStore;
use std::path::PathBuf;

fn empty() -> PackSnapshot {
    PackSnapshot::new(
        DirContextTreeNode::new(NodeType::Directory, "", PathBuf::new()),
        Version::new_v1(),
        Timestamp(1),
        ContentDigests::new(),
    )
    .unwrap()
}

fn rich() -> PackSnapshot {
    let mut root = DirContextTreeNode::new(NodeType::Directory, "", PathBuf::from("/work"));
    root.generated_id_for_llm = "D000".into();
    let mut source = DirContextTreeNode::new(NodeType::File, "src/lib.rs", PathBuf::from("/work/src/lib.rs"));
    source.last_modified_timestamp = 17;
    source.content_offset_in_data_section = 4;
    source.content_size = 16;
    source.generated_id_for_llm = "F001".into();
    source.file_content = Some(b"pub fn work() {}".to_vec());
    let mut secret = DirContextTreeNode::new(NodeType::File, "secret.bin", PathBuf::from("/work/secret.bin"));
    secret.handling_decision = FileHandlingDecision::OmitContent;
    secret.is_binary_file = true;
    secret.binary_file_type = Some("octet-stream".into());
    secret.content_size = 3;
    secret.is_ignored = true;
    root.children.push(source);
    root.children.push(secret);
    let mut digests = ContentDigests::new();
    digests.insert("src/lib.rs".into(), Sha256::digest(b"pub fn work() {}").unwrap());
    PackSnapshot::new(root, Version { major: 2, minor: 4 }, Timestamp(42), digests).unwrap()
}

fn same(left: &DirContextTreeNode, right: &DirContextTreeNode) {
    assert_eq!(left.node_type, right.node_type);
    assert_eq!(left.relative_path, right.relative_path);
    assert_eq!(left.last_modified_timestamp, right.last_modified_timestamp);
    assert_eq!(left.content_offset_in_data_section, right.content_offset_in_data_section);
    assert_eq!(left.content_size, right.content_size);
    assert_eq!(left.disk_path, right.disk_path);
    assert_eq!(left.generated_id_for_llm, right.generated_id_for_llm);
    assert_eq!(left.file_content, right.file_content);
    assert_eq!(left.is_binary_file, right.is_binary_file);
    assert_eq!(left.binary_file_type, right.binary_file_type);
    assert_eq!(left.is_ignored, right.is_ignored);
    assert_eq!(left.handling_decision, right.handling_decision);
    assert_eq!(left.children.len(), right.children.len());
    for (child, other) in left.children.iter().zip(&right.children) {
        same(child, other);
    }
}

#[test]
fn publish_roundtrips_and_a_second_view_stays_intact() {
    let mut store = SqliteStore::in_memory().unwrap();
    let alpha = ViewKey::parse("alpha").unwrap();
    let beta = ViewKey::parse("beta").unwrap();
    assert!(store.load(&ViewKey::parse("missing").unwrap()).unwrap().is_none());
    store.publish(&alpha, &empty()).unwrap();
    store.publish(&beta, &rich()).unwrap();
    let loaded = store.load(&beta).unwrap().unwrap();
    let original = rich();
    assert_eq!(loaded.digest(), original.digest());
    assert_eq!(loaded.version(), original.version());
    assert_eq!(loaded.taken_at(), original.taken_at());
    assert_eq!(loaded.digests(), original.digests());
    same(loaded.tree(), original.tree());
    assert_eq!(store.load(&alpha).unwrap().unwrap().digest(), empty().digest());
    store.publish(&alpha, &original).unwrap();
    assert_eq!(store.load(&alpha).unwrap().unwrap().digest(), original.digest());
    assert_eq!(store.load(&beta).unwrap().unwrap().digest(), original.digest());
}

#[test]
fn corrupt_bytes_are_refused_and_a_valid_snapshot_still_loads() {
    let mut store = SqliteStore::in_memory().unwrap();
    let view = ViewKey::parse("alpha").unwrap();
    store.publish(&view, &rich()).unwrap();
    let mut blob: Vec<u8> = store
        .connection
        .query_row("SELECT payload FROM pack_snapshots WHERE view_key = 'alpha'", [], |row| row.get(0))
        .unwrap();
    blob[10] ^= 0xff;
    store.connection.execute("UPDATE pack_snapshots SET payload = ?1 WHERE view_key = 'alpha'", [blob]).unwrap();
    assert_eq!(
        store.load(&view).unwrap_err(),
        DctxError::CorruptIndex { reason: "pack snapshot" }
    );
    store.publish(&view, &empty()).unwrap();
    assert_eq!(store.load(&view).unwrap().unwrap().digest(), empty().digest());
}

#[test]
fn failed_publish_keeps_the_previous_snapshot() {
    let mut store = SqliteStore::in_memory().unwrap();
    store.set_cache_cap(50_000).unwrap();
    let view = ViewKey::parse("alpha").unwrap();
    store.publish(&view, &empty()).unwrap();
    let first = store.load(&view).unwrap().unwrap().digest();
    let used = store.cached_bytes().unwrap();
    store.set_cache_cap(used).unwrap();
    assert!(matches!(store.publish(&view, &rich()).unwrap_err(), DctxError::CacheLimit { .. }));
    assert_eq!(store.load(&view).unwrap().unwrap().digest(), first);
}

#[test]
fn git_view_keys_are_refused_and_plain_keys_publish() {
    let mut store = SqliteStore::in_memory().unwrap();
    let git = ViewKey::parse("git:da39a3ee5e6b4b0d3255bfef95601890afd80709").unwrap();
    assert_eq!(
        store.publish(&git, &empty()).unwrap_err(),
        DctxError::InvalidRequest { reason: GIT_IDENTITY }
    );
    let view = ViewKey::parse("pack-root").unwrap();
    store.publish(&view, &empty()).unwrap();
    assert!(store.load(&view).unwrap().is_some());
}

#[test]
fn idle_views_expire_and_a_backward_clock_does_not() {
    let mut store = SqliteStore::in_memory().unwrap();
    let view = ViewKey::parse("pack-root").unwrap();
    store.publish(&view, &empty()).unwrap();
    store.set_clock(VIEW_IDLE_MS - 1);
    store.maintain().unwrap();
    assert!(store.load(&view).unwrap().is_some());
    store.set_clock(VIEW_IDLE_MS);
    store.maintain().unwrap();
    assert!(store.load(&view).unwrap().is_none());
    store.publish(&view, &empty()).unwrap();
    assert!(store.load(&view).unwrap().is_some());
    store.set_clock(VIEW_IDLE_MS + 100);
    store.publish(&view, &rich()).unwrap();
    store.set_clock(VIEW_IDLE_MS + 40);
    let report = store.maintain().unwrap();
    assert!(report.clock_rewound);
    assert_eq!(store.load(&view).unwrap().unwrap().digest(), rich().digest());
}
