// SPDX-License-Identifier: Apache-2.0
use super::{check_object_size, is_conflict};
use crate::domain::{AutoCleanConfig, DctxError, Resource};
use crate::session::ledger::PersistedCache;
use dctx_adapter_cache::CacheError;

#[test]
fn admitted_objects_are_readable_after_reopen_at_a_scaled_boundary() {
    let dir = tempfile::tempdir().expect("cache");
    let policy = AutoCleanConfig {
        enabled: false,
        ..AutoCleanConfig::default()
    };
    let mut cold = PersistedCache::open(dir.path(), 42, policy, 0).expect("cold");
    // Production reads cover every object that the product cache can admit,
    // including generations larger than the former 512 MiB read ceiling.
    assert_eq!(cold.object_read_cap, dctx_adapter_cache::HARD_CAP_BYTES);
    check_object_size(629_335_341, cold.object_read_cap).expect("reported regression fits");
    cold.object_read_cap = 64;
    let bytes = vec![7; 64];
    let lease = cold
        .admit_snapshot(0, &bytes, 1024, 0, 0, None)
        .expect("admit boundary");
    drop(lease);
    drop(cold);

    let mut warm = PersistedCache::open(dir.path(), 42, policy, 1).expect("warm");
    warm.object_read_cap = 64;
    assert_eq!(
        warm.load_current_segment().expect("warm read"),
        Some(bytes.clone())
    );
    let admitted = warm.life.admitted();
    let error = warm
        .admit_snapshot(1, &[0; 65], 1024, 0, 0, None)
        .err()
        .expect("reject oversized");
    assert_eq!(
        error,
        DctxError::ResourceLimit {
            resource: Resource::ObjectBytes,
            limit: 64
        }
    );
    assert_eq!(
        warm.life.admitted(),
        admitted,
        "refusal must not charge or evict"
    );
    assert_eq!(
        warm.load_current_segment()
            .expect("old generation preserved"),
        Some(bytes)
    );
}

#[test]
fn oversized_stored_objects_name_the_object_read_limit() {
    let dir = tempfile::tempdir().expect("cache");
    std::fs::create_dir(dir.path().join("objects")).expect("objects");
    std::fs::write(dir.path().join("objects/7"), [0; 65]).expect("object");
    assert_eq!(
        super::read_object(dir.path(), 7, 64),
        Err(DctxError::ResourceLimit {
            resource: Resource::ObjectBytes,
            limit: 64,
        })
    );
}

#[test]
fn conflict_reasons_retry_and_a_cap_refusal_does_not() {
    let stale = CacheError::InvalidRequest {
        reason: "stale ledger",
    };
    let published = CacheError::InvalidRequest {
        reason: "object already published",
    };
    let cap = CacheError::CacheLimit {
        requested: 4,
        available: 1,
        pinned: 0,
        pending: 0,
    };
    assert!(is_conflict(&stale));
    assert!(is_conflict(&published));
    assert!(!is_conflict(&cap));
}

#[cfg(unix)]
#[test]
fn objects_with_other_user_write_access_or_linked_storage_are_refused() {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};
    let temp = tempfile::tempdir().unwrap();
    let objects = temp.path().join("objects");
    fs::create_dir(&objects).unwrap();
    fs::write(objects.join("7"), b"object").unwrap();
    fs::set_permissions(objects.join("7"), fs::Permissions::from_mode(0o666)).unwrap();
    assert!(super::read_object(temp.path(), 7, 64).is_err());
    fs::set_permissions(objects.join("7"), fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        super::read_object(temp.path(), 7, 64).unwrap().unwrap(),
        b"object"
    );
    fs::set_permissions(&objects, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(super::read_object(temp.path(), 7, 64).is_err());
    fs::set_permissions(&objects, fs::Permissions::from_mode(0o700)).unwrap();
    fs::rename(&objects, temp.path().join("outside")).unwrap();
    symlink(temp.path().join("outside"), &objects).unwrap();
    assert!(super::read_object(temp.path(), 7, 64).is_err());
}

#[test]
fn a_superseded_inventory_can_neither_pin_nor_refresh_the_new_view() {
    let dir = tempfile::tempdir().expect("cache");
    let policy = AutoCleanConfig {
        enabled: false,
        ..AutoCleanConfig::default()
    };
    let mut first = PersistedCache::open(dir.path(), 42, policy, 0).expect("first");
    drop(
        first
            .admit_snapshot(0, b"old", 1024, 0, 0, None)
            .expect("publish old"),
    );
    assert_eq!(
        first.load_current_segment().expect("load old"),
        Some(b"old".to_vec())
    );
    let expected = first
        .life
        .current_generation(first.root_id)
        .expect("generation");
    let mut second = PersistedCache::open(dir.path(), 42, policy, 1).expect("second");
    drop(
        second
            .admit_snapshot(1, b"new", 1024, 0, 0, None)
            .expect("publish new"),
    );
    assert!(matches!(
        first.admit_snapshot(2, b"refreshed old", 1024, 0, 0, Some(expected)),
        Err(DctxError::Busy)
    ));
    assert!(first.pin_current(2).expect("pin superseded").is_none());
    assert_eq!(
        first.load_current_segment().expect("load new"),
        Some(b"new".to_vec())
    );
    assert!(first.pin_current(3).expect("pin checked view").is_some());
}
