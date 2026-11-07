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
        .admit_snapshot(0, &bytes, 1024, 0, 0)
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
        .admit_snapshot(1, &[0; 65], 1024, 0, 0)
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
