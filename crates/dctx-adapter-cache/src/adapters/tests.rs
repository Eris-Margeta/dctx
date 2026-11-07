// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::{CacheError, LEDGER_VERSION, Lifecycle, Quota};
use std::fs;

fn root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("cache");
    fs::create_dir(&root).expect("dir");
    (dir, root)
}

#[test]
fn crash_before_rename_keeps_the_previous_ledger_and_the_temp_file() {
    let (_dir, root) = root();
    let store = FsLedgerStore::open(&root).expect("open");
    let first = Lifecycle::new(Quota::scaled(80)).expect("quota");
    store.store(&first).expect("store");
    let bytes = first.encode_ledger().expect("encode");
    let mut second = first.clone();
    second.register_root(crate::RootId::new(1)).expect("root");
    store.stage(&second).expect("stage");
    let staged = store.staged_path().expect("temp");
    assert!(staged.is_file());
    assert_ne!(
        staged.file_name().and_then(|name| name.to_str()),
        Some(LEDGER_TEMP)
    );
    let loaded = store.load(Quota::scaled(10)).expect("load");
    assert!(loaded.fault.is_none());
    assert_eq!(loaded.lifecycle.encode_ledger().expect("encode"), bytes);
    assert!(staged.is_file());
    store.commit().expect("commit");
    let loaded = store.load(Quota::scaled(10)).expect("load");
    assert_eq!(
        loaded.lifecycle.encode_ledger().expect("encode"),
        second.encode_ledger().expect("encode")
    );
    assert!(!staged.exists());
}

#[test]
fn corrupt_and_unknown_versions_fall_back_without_deleting_strangers() {
    let (_dir, root) = root();
    let store = FsLedgerStore::open(&root).expect("open");
    let life = Lifecycle::new(Quota::scaled(40)).expect("quota");
    store.store(&life).expect("store");
    let path = root.join(LEDGER_FILE);
    let mut bytes = fs::read(&path).expect("read");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&path, &bytes).expect("write");
    fs::write(root.join("notes.txt"), b"keep").expect("notes");
    let loaded = store.load(Quota::scaled(15)).expect("load");
    assert!(matches!(
        loaded.fault,
        Some(CacheError::CorruptLedger { reason: "checksum" })
    ));
    assert_eq!(loaded.lifecycle.status().hard_cap, 15);
    assert_eq!(loaded.lifecycle.admitted(), 0);
    assert_eq!(fs::read(root.join("notes.txt")).expect("notes"), b"keep");
    assert!(path.is_file());
    assert!(
        loaded
            .orphans
            .iter()
            .any(|file| file.relative == "notes.txt")
    );

    let mut unknown = life.encode_ledger().expect("encode");
    unknown[8..12].copy_from_slice(&99u32.to_le_bytes());
    assert_ne!(99, LEDGER_VERSION);
    fs::write(&path, &unknown).expect("write");
    fs::write(root.join("stray.bin"), b"x").expect("stray");
    let loaded = store.load(Quota::scaled(15)).expect("load");
    assert!(matches!(
        loaded.fault,
        Some(CacheError::UnknownLedgerVersion { found: 99 })
    ));
    assert_eq!(fs::read(root.join("stray.bin")).expect("stray"), b"x");
    assert_eq!(fs::read(root.join("notes.txt")).expect("notes"), b"keep");
}

#[test]
fn update_commits_one_observed_revision_and_a_rival_store_stays_stale() {
    let (_dir, root) = root();
    let quota = Quota::scaled(80);
    let store = FsLedgerStore::open(&root).expect("open");
    let rival = FsLedgerStore::open(&root).expect("rival");
    store
        .update(quota, |life| life.load_staged(10, 0).map(|_| ((), true)))
        .expect("create");
    rival.load(quota).expect("observe");
    let before = fs::read(root.join(LEDGER_FILE)).expect("bytes");
    let admitted = store
        .update(quota, |life| {
            let admitted = life.admitted();
            life.load_staged(4, 0)?;
            Ok::<(u64, bool), CacheError>((admitted, false))
        })
        .expect("keep");
    assert_eq!(admitted, 10);
    assert_eq!(fs::read(root.join(LEDGER_FILE)).expect("bytes"), before);
    store
        .update(quota, |life| life.load_staged(4, 0).map(|_| ((), true)))
        .expect("commit");
    let stale = rival.store(&Lifecycle::new(quota).expect("fresh"));
    assert!(matches!(
        stale,
        Err(CacheError::InvalidRequest {
            reason: "stale ledger"
        })
    ));
    let reloaded = rival.load(quota).expect("reload");
    assert!(reloaded.fault.is_none());
    rival.store(&reloaded.lifecycle).expect("current");
}
