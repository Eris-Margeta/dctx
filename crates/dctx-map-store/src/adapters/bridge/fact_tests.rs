// SPDX-License-Identifier: Apache-2.0
//! Parse-once, expiry and cap evidence for retained `FactKeys`.
//! Implements DX-C2: three roots, one parse, reparse after grace and a lowered cap.
//! Uses the public store API. The five-verb rows are covered beside this bridge.

use crate::{
    accounted_bytes, ExtractionContract, FactKey, Retain, SqliteStore, StoreError, HARD_CAP_BYTES,
    UNREACHABLE_GRACE_MS,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn contract(language: u16, revision: &str) -> ExtractionContract {
    ExtractionContract::new(language, "rust-2024", "dctx-map-parse", revision, "plan-1", 1).unwrap()
}

struct TempDb {
    path: PathBuf,
}

impl TempDb {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("dctx-c2-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn db(&self) -> PathBuf {
        self.path.join("index.sqlite")
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn fresh_store_uses_the_stage0_cap() {
    let store = SqliteStore::in_memory().unwrap();
    assert_eq!(store.cache_cap().unwrap(), HARD_CAP_BYTES);
    assert_eq!(store.parse_count().unwrap(), 0);
    assert_eq!(store.cached_bytes().unwrap(), 0);
}

#[test]
fn three_roots_parse_once_and_gc_reopen_reparses() {
    let tmp = TempDb::new();
    let payload = b"file-local-facts";
    let key = FactKey::from_bytes(b"fn shared() {}", contract(1, "rev-1")).unwrap();
    let mut store = SqliteStore::open(tmp.db()).unwrap();
    for root in ["root-a", "root-b", "root-c"] {
        let outcome = store.retain_fact("team", root, &key, payload).unwrap();
        let expected = if root == "root-a" { Retain::Parsed } else { Retain::Reused };
        assert_eq!(outcome, expected, "{root}");
    }
    assert_eq!(store.parse_count().unwrap(), 1);
    assert_eq!(store.cached_bytes().unwrap(), accounted_bytes(payload.len() as u64).unwrap());
    assert_eq!(store.retained_fact("team", &key).unwrap().as_deref(), Some(payload.as_slice()));
    store.release_root("team", "root-a").unwrap();
    assert!(store.retained_fact("team", &key).unwrap().is_some());
    assert_eq!(store.parse_count().unwrap(), 1);
    store.release_root("team", "root-b").unwrap();
    store.release_root("team", "root-c").unwrap();
    store.set_clock(UNREACHABLE_GRACE_MS - 1);
    store.maintain().unwrap();
    assert!(store.retained_fact("team", &key).unwrap().is_some());
    drop(store);

    let mut store = SqliteStore::open(tmp.db()).unwrap();
    assert_eq!(store.parse_count().unwrap(), 1);
    store.set_clock(UNREACHABLE_GRACE_MS - 1);
    assert!(store.retained_fact("team", &key).unwrap().is_some());
    store.set_clock(UNREACHABLE_GRACE_MS);
    store.maintain().unwrap();
    assert!(store.retained_fact("team", &key).unwrap().is_none());
    drop(store);

    let mut store = SqliteStore::open(tmp.db()).unwrap();
    store.set_clock(UNREACHABLE_GRACE_MS);
    assert!(store.retained_fact("team", &key).unwrap().is_none());
    assert_eq!(store.retain_fact("team", "root-a", &key, payload).unwrap(), Retain::Parsed);
    assert_eq!(store.parse_count().unwrap(), 2);
    let revised = FactKey::from_bytes(b"fn shared() {}", contract(1, "rev-2")).unwrap();
    assert_eq!(store.retain_fact("team", "root-a", &revised, payload).unwrap(), Retain::Parsed);
    let markdown = FactKey::from_bytes(b"fn shared() {}", contract(4, "rev-1")).unwrap();
    assert_eq!(store.retain_fact("team", "root-a", &markdown, payload).unwrap(), Retain::Parsed);
    assert_eq!(store.retain_fact("other", "root-a", &key, payload).unwrap(), Retain::Parsed);
    assert_eq!(store.retain_fact("team", "root-b", &key, payload).unwrap(), Retain::Reused);
    assert_eq!(store.parse_count().unwrap(), 5);
    assert!(matches!(
        store.retain_fact("team", "root-c", &key, b"different"),
        Err(StoreError::FactConflict)
    ));
    assert!(store.retain_fact("team", ".git", &key, payload).is_err());
    assert_eq!(store.retain_fact("team", "root-c", &key, payload).unwrap(), Retain::Reused);
    assert_eq!(store.parse_count().unwrap(), 5);
    assert!(store.retained_fact("team", &revised).unwrap().is_some());
}

#[test]
fn cap_pressure_drops_unreferenced_facts_and_refuses_growth() {
    let mut store = SqliteStore::in_memory().unwrap();
    store.set_cache_cap(600).unwrap();
    let first = FactKey::from_bytes(b"a", contract(1, "rev-1")).unwrap();
    let second = FactKey::from_bytes(b"b", contract(1, "rev-1")).unwrap();
    let third = FactKey::from_bytes(b"c", contract(1, "rev-1")).unwrap();
    let body = [1u8; 32];
    assert_eq!(accounted_bytes(32).unwrap(), 288);
    store.retain_fact("team", "root-a", &first, &body).unwrap();
    store.retain_fact("team", "root-b", &second, &body).unwrap();
    let blocked = store.maintain().unwrap();
    assert_eq!(blocked.blocked_bytes, 576 - 360);
    assert_eq!(blocked.reclaimed_bytes, 0);
    assert_eq!(store.parse_count().unwrap(), 2);
    store.release_root("team", "root-b").unwrap();
    assert_eq!(store.cached_bytes().unwrap(), 288);
    assert!(store.retained_fact("team", &first).unwrap().is_some());
    assert!(store.retained_fact("team", &second).unwrap().is_none());
    assert_eq!(store.maintain().unwrap().reclaimed_bytes, 0);
    let too_big = vec![2u8; 144];
    assert_eq!(accounted_bytes(144).unwrap(), 400);
    assert!(matches!(
        store.retain_fact("team", "root-c", &third, &too_big),
        Err(StoreError::CacheLimit { requested: 400, available: 312 })
    ));
    assert_eq!(store.parse_count().unwrap(), 2);
    let fits = vec![9u8; 56];
    assert_eq!(store.retain_fact("team", "root-c", &third, &fits).unwrap(), Retain::Parsed);
    assert_eq!(store.cached_bytes().unwrap(), 600);
    assert_eq!(store.parse_count().unwrap(), 3);
    assert!(store.retained_fact("team", &first).unwrap().is_some());
}

#[test]
fn empty_namespace_is_refused_and_a_real_one_is_kept() {
    let mut store = SqliteStore::in_memory().unwrap();
    let key = FactKey::from_bytes(b"a", contract(1, "rev-1")).unwrap();
    assert!(store.retain_fact("", "root", &key, b"x").is_err());
    assert_eq!(store.parse_count().unwrap(), 0);
    assert_eq!(store.retain_fact("team", "root", &key, b"x").unwrap(), Retain::Parsed);
    assert_eq!(store.parse_count().unwrap(), 1);
}
