// SPDX-License-Identifier: Apache-2.0
//! Shared admission, persisted snapshot leases, and purge/GC for one cache root.
//! Implements CACHE-v1 publication and explicit purge on the adapter coordinator (DX-RV5H).
//! Depends on [`super::PersistedCache`] and `dctx-adapter-cache` load/store, leases, and unlink.
//! Session snapshot, purge, and collection call this module. Isolation never does.
//! A live handle holds the ledger lease. The view manifest names the segment object.

use std::path::Path;

use dctx_adapter_cache::adapters::{FsLedgerStore, FsObjectStore, LeaseHold, LedgerUpdateError};
use dctx_adapter_cache::ports::{LedgerStore, ObjectStore};
use dctx_adapter_cache::{
    CacheError, GcRequest as AdapterGc, GenerationId, LeaseId, Lifecycle, ObjectId,
    PurgeRequest as AdapterPurge, PurgeScope as AdapterScope, Quota, RootId, SliceBudget,
};

use super::{PersistedCache, clock_sample, map_cache};
use crate::domain::{DctxError, Resource};

/// Retries when another session commits the ledger or publishes the same object id.
pub(super) const COMMIT_ATTEMPTS: u32 = 4;

/// Largest object file a reuse load will read. A bigger file is refused.
const OBJECT_READ_CAP: u64 = 512 * 1024 * 1024;

/// Lease written with a snapshot. Drop releases it in the ledger; bytes stay until collection.
pub(in crate::session) struct SnapshotLease {
    root: std::path::PathBuf,
    lease: LeaseId,
    /// Proof of life: locked while this process holds the lease, so a killed reader's lease is recognised as orphaned.
    /// Dropped after the ledger release in [`Drop`], because fields drop after the body.
    _alive: Option<LeaseHold>,
}

impl Drop for SnapshotLease {
    fn drop(&mut self) {
        let _ = release_persisted(&self.root, self.lease);
    }
}

/// Bytes unlinked by an applying purge or collection, and leases that blocked destruction.
pub(in crate::session) struct DiskReclaim {
    /// Ledger bytes removed and unlinked. Zero on a dry-run.
    pub reclaimed_bytes: u64,
    /// Manifests an active lease kept.
    pub held_by_lease: u64,
}

impl PersistedCache {
    /// Publishes `bytes` under the shared cap and pins the new view with a persisted lease.
    /// One exclusive section covers the fresh load, the eviction of reclaimable generations, the object writes,
    /// the commit and the unlink of the view this one replaced. A reader holds the shared lock, so it never sees
    /// an object file this section reclaims.
    pub(super) fn admit_snapshot(
        &mut self,
        now_ms: u64,
        bytes: &[u8],
        capacity: u64,
        pack_committed: u64,
        pack_reserved: u64,
    ) -> Result<Option<SnapshotLease>, DctxError> {
        if bytes.is_empty() {
            return Ok(None);
        }
        let segment_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let manifest_len = u64::try_from(view_manifest(self.root_id, 0).len()).unwrap_or(u64::MAX);
        let need = segment_len.saturating_add(manifest_len);
        let (root_id, session) = (self.root_id, self.session);
        let mut written: Vec<ObjectId> = Vec::new();
        let outcome =
            self.ledger
                .update_reclaiming(Quota::product_default(), &self.objects, |working| {
                    ensure_root(working, root_id)?;
                    let budget = capacity
                        .saturating_sub(pack_committed)
                        .saturating_sub(pack_reserved);
                    working.observe(clock_sample(now_ms));
                    // Reclaimable generations go before the request is refused, oldest first, across roots.
                    working.evict_to_fit(budget.saturating_sub(need));
                    let available =
                        room(capacity, working.admitted(), pack_committed, pack_reserved);
                    if need > available {
                        return Err(DctxError::CacheLimit {
                            requested: need,
                            available,
                        });
                    }
                    let segment_id = working
                        .insert_segment(segment_len, 0, &[root_id])
                        .map_err(map_cache)?;
                    let generation = working
                        .publish_view(root_id, manifest_len, &[segment_id])
                        .map_err(map_cache)?;
                    let lease = working.reader_begin(root_id, session).map_err(map_cache)?;
                    let pinned = working.reader_confirm(lease).map_err(map_cache)?;
                    if pinned != generation {
                        return Err(DctxError::invalid(
                            "snapshot lease pinned a different generation",
                        ));
                    }
                    let manifest = view_manifest(root_id, segment_id.raw());
                    let manifest_object = manifest_object(working, root_id, generation)?;
                    put_fresh(&self.objects, &mut written, segment_id, bytes).map_err(map_cache)?;
                    put_fresh(&self.objects, &mut written, manifest_object, &manifest)
                        .map_err(map_cache)?;
                    let alive = self.ledger.hold_lease(lease).map_err(map_cache)?;
                    Ok(((lease, alive), true))
                });
        match outcome {
            Ok(((lease, alive), life)) => {
                self.life = life;
                Ok(Some(SnapshotLease {
                    root: self.root_path.clone(),
                    lease,
                    _alive: Some(alive),
                }))
            }
            Err(error) => {
                self.discard_unpublished(&written);
                Err(update_error(error))
            }
        }
    }

    /// Unlinks files this section wrote when the ledger does not name them: the commit never happened.
    fn discard_unpublished(&self, written: &[ObjectId]) {
        if written.is_empty() {
            return;
        }
        let named = self
            .ledger
            .read(Quota::product_default(), |life| {
                Ok::<_, DctxError>(
                    written
                        .iter()
                        .map(|id| life.object_state(*id).is_some())
                        .collect::<Vec<_>>(),
                )
            })
            .unwrap_or_else(|_| vec![true; written.len()]);
        for (id, named) in written.iter().zip(named) {
            if !named {
                let _ = self.objects.unlink_object(*id);
            }
        }
    }

    /// Drops unleased retention, unlinks those bytes, and leaves an active lease untouched.
    /// `apply: false` reports leases and writes nothing.
    pub(in crate::session) fn reclaim_unleased(
        &mut self,
        now_ms: u64,
        apply: bool,
    ) -> Result<DiskReclaim, DctxError> {
        if !apply {
            self.reload()?;
            let mut preview = self.life.clone();
            preview.observe(clock_sample(now_ms));
            let report = preview
                .purge(AdapterPurge {
                    scope: AdapterScope::Root(self.root_id),
                    confirm: false,
                    dry_run: true,
                })
                .map_err(map_cache)?;
            return Ok(DiskReclaim {
                reclaimed_bytes: 0,
                held_by_lease: report.held_by_lease,
            });
        }
        for _ in 0..COMMIT_ATTEMPTS {
            self.reload()?;
            ensure_root(&mut self.life, self.root_id)?;
            let original = self.life.clone();
            let mut working = original.clone();
            working.observe(clock_sample(now_ms));
            let purged = working
                .purge(AdapterPurge {
                    scope: AdapterScope::Root(self.root_id),
                    confirm: true,
                    dry_run: false,
                })
                .map_err(map_cache)?;
            sweep(&mut working, self.root_id).map_err(map_cache)?;
            let reclaimed = original.admitted().saturating_sub(working.admitted());
            let removed = removed_ids(&original, &working);
            if let Err(error) = unlink_all(&self.objects, &removed) {
                if is_conflict(&error) {
                    continue;
                }
                return Err(map_cache(error));
            }
            match self.ledger.store(&working) {
                Ok(()) => {
                    self.life = working;
                    return Ok(DiskReclaim {
                        reclaimed_bytes: reclaimed,
                        held_by_lease: purged.held_by_lease,
                    });
                }
                Err(error) if is_conflict(&error) => {}
                Err(error) => return Err(map_cache(error)),
            }
        }
        Err(DctxError::invalid("stale ledger"))
    }

    /// Bytes of the current view's segment. `Ok(None)` when the root has no readable generation.
    /// The shared lock is held from the ledger load to the last object read, so a publish cannot reclaim
    /// the generation in between and two readers never exclude each other.
    pub(in crate::session) fn load_current_segment(
        &mut self,
    ) -> Result<Option<Vec<u8>>, DctxError> {
        let (root_id, root_path) = (self.root_id, self.root_path.clone());
        let mut seen: Option<Lifecycle> = None;
        let bytes = self
            .ledger
            .read(Quota::product_default(), |life| {
                seen = Some(life.clone());
                let Some(generation) = life.current_generation(root_id) else {
                    return Ok(None);
                };
                let Ok(manifest_id) = manifest_object(life, root_id, generation) else {
                    return Ok(None);
                };
                let Some(manifest) = read_object(&root_path, manifest_id.raw())? else {
                    return Ok(None);
                };
                let Some(segment_raw) = segment_id_of(&manifest) else {
                    return Ok(None);
                };
                read_object(&root_path, segment_raw)
            })
            .map_err(update_error)?;
        if let Some(life) = seen {
            self.life = life;
        }
        Ok(bytes)
    }

    /// Pins the current view with a new reader lease. A miss is `Ok(None)` so the caller can rebuild.
    pub(in crate::session) fn pin_current(
        &mut self,
        now_ms: u64,
    ) -> Result<Option<SnapshotLease>, DctxError> {
        let (root_id, session) = (self.root_id, self.session);
        let (lease, life) = self
            .ledger
            .update_reclaiming(Quota::product_default(), &self.objects, |working| {
                ensure_root(working, root_id)?;
                let Some(generation) = working.current_generation(root_id) else {
                    return Ok((None, false));
                };
                working.observe(clock_sample(now_ms));
                let lease = match working.reader_begin(root_id, session) {
                    Ok(lease) => lease,
                    Err(CacheError::InvalidRequest { .. }) => return Ok((None, false)),
                    Err(error) => return Err(map_cache(error)),
                };
                let pinned = match working.reader_confirm(lease) {
                    Ok(pinned) => pinned,
                    Err(CacheError::InvalidRequest { .. }) => return Ok((None, false)),
                    Err(error) => return Err(map_cache(error)),
                };
                if pinned != generation {
                    return Ok((None, false));
                }
                let alive = self.ledger.hold_lease(lease).map_err(map_cache)?;
                Ok((Some((lease, alive)), true))
            })
            .map_err(update_error)?;
        self.life = life;
        Ok(lease.map(|(lease, alive)| SnapshotLease {
            root: self.root_path.clone(),
            lease,
            _alive: Some(alive),
        }))
    }
}

/// Maps a section failure onto the facade error: the edit's own refusal, or the cache's.
pub(super) fn update_error(error: LedgerUpdateError<DctxError>) -> DctxError {
    match error {
        LedgerUpdateError::Body(error) => error,
        LedgerUpdateError::Cache(error) => map_cache(error),
    }
}

/// Writes a new object file. A file left under this id by a crashed publish is not in the ledger
/// (the section holds the exclusive lock and the ledger was just loaded), so it is replaced.
fn put_fresh(
    objects: &FsObjectStore,
    written: &mut Vec<ObjectId>,
    id: ObjectId,
    bytes: &[u8],
) -> Result<(), CacheError> {
    match objects.put_object(id.raw(), bytes) {
        Err(CacheError::InvalidRequest {
            reason: "object already published",
        }) => {
            objects.unlink_object(id)?;
            objects.put_object(id.raw(), bytes)?;
        }
        other => other?,
    }
    written.push(id);
    Ok(())
}

pub(super) fn is_conflict(error: &CacheError) -> bool {
    matches!(
        error,
        CacheError::InvalidRequest {
            reason: "stale ledger" | "object already published"
        }
    )
}

fn room(capacity: u64, admitted: u64, pack_committed: u64, pack_reserved: u64) -> u64 {
    capacity
        .saturating_sub(admitted)
        .saturating_sub(pack_committed)
        .saturating_sub(pack_reserved)
}

fn view_manifest(root: RootId, segment_raw: u64) -> Vec<u8> {
    let mut body = Vec::with_capacity(25);
    body.extend_from_slice(b"dctxview1");
    body.extend_from_slice(&root.raw().to_le_bytes());
    body.extend_from_slice(&segment_raw.to_le_bytes());
    body
}

fn segment_id_of(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < 25 || bytes.get(..9)? != b"dctxview1" {
        return None;
    }
    let raw: [u8; 8] = bytes.get(17..25)?.try_into().ok()?;
    Some(u64::from_le_bytes(raw))
}

fn read_object(root: &Path, raw: u64) -> Result<Option<Vec<u8>>, DctxError> {
    let path = root.join("objects").join(raw.to_string());
    if !path.starts_with(root) {
        return Err(DctxError::invalid("cache object is outside the root"));
    }
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(DctxError::io("object metadata")),
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(DctxError::invalid("cache object is not a file"));
    }
    if meta.len() > OBJECT_READ_CAP {
        return Err(DctxError::ResourceLimit {
            resource: Resource::Memory,
            limit: OBJECT_READ_CAP,
        });
    }
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(DctxError::io("object read")),
    }
}

fn ensure_root(life: &mut Lifecycle, root: RootId) -> Result<(), DctxError> {
    if life.status().roots.iter().any(|item| item.id == root) {
        return Ok(());
    }
    life.register_root(root).map_err(map_cache)
}

fn manifest_object(
    life: &Lifecycle,
    root: RootId,
    generation: GenerationId,
) -> Result<ObjectId, DctxError> {
    let manifest_id = life
        .manifest_of(root, generation)
        .ok_or_else(|| DctxError::invalid("snapshot manifest missing"))?;
    life.manifest_object(manifest_id)
        .ok_or_else(|| DctxError::invalid("snapshot manifest object missing"))
}

fn removed_ids(before: &Lifecycle, after: &Lifecycle) -> Vec<ObjectId> {
    before
        .object_ids()
        .into_iter()
        .filter(|id| after.object_state(*id).is_none())
        .collect()
}

fn unlink_all(objects: &FsObjectStore, ids: &[ObjectId]) -> Result<(), CacheError> {
    for id in ids {
        objects.unlink_object(*id)?;
    }
    Ok(())
}

fn sweep(life: &mut Lifecycle, root: RootId) -> Result<(), CacheError> {
    // One slice marks unreachable bytes pending and advances the cursor past them.
    // The next slice, with the cursor back at zero, drops that pending charge.
    for _ in 0..8 {
        let before = life.admitted();
        let report = life.gc(AdapterGc {
            root: Some(root),
            budget: SliceBudget::explicit(128),
            bypass_grace: true,
            max_bytes: None,
            cancel: None,
        })?;
        if report.examined == 0 && report.reclaimed_bytes == 0 && life.admitted() == before {
            return Ok(());
        }
    }
    Ok(())
}

fn release_persisted(root: &Path, lease: LeaseId) -> Result<(), CacheError> {
    let ledger = FsLedgerStore::open(root)?;
    let objects = FsObjectStore::open(root)?;
    let released = ledger.update_reclaiming(Quota::product_default(), &objects, |life| {
        // An unknown lease is already gone. A superseded view this lease held is retired by the release and unlinked here.
        Ok::<_, CacheError>(((), life.reader_release(lease).is_ok()))
    });
    match released {
        Ok(_)
        | Err(LedgerUpdateError::Cache(
            CacheError::CorruptLedger { .. } | CacheError::UnknownLedgerVersion { .. },
        )) => Ok(()),
        Err(LedgerUpdateError::Cache(error) | LedgerUpdateError::Body(error)) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::is_conflict;
    use dctx_adapter_cache::CacheError;

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
}
