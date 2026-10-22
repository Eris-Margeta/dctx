//! Shared admission, persisted snapshot leases, and purge/GC for one cache root.
//! Implements CACHE-v1 publication and explicit purge on the adapter coordinator (DX-RV5H).
//! Depends on [`super::PersistedCache`] and `dctx-adapter-cache` load/store, leases, and unlink.
//! Session snapshot, purge, and collection call this module. Isolation never does.
//! A live handle holds the ledger lease. The view manifest names the segment object.

use std::path::Path;

use dctx_adapter_cache::adapters::{FsLedgerStore, FsObjectStore};
use dctx_adapter_cache::ports::{LedgerStore, ObjectStore};
use dctx_adapter_cache::{
    CacheError, GcRequest as AdapterGc, GenerationId, LeaseId, Lifecycle, ObjectId, PurgeRequest as AdapterPurge,
    PurgeScope as AdapterScope, Quota, RootId, SliceBudget,
};

use super::{clock_sample, map_cache, PersistedCache};
use crate::domain::{DctxError, Resource};

/// Retries when another session commits the ledger or publishes the same object id.
pub(super) const COMMIT_ATTEMPTS: u32 = 4;

/// Largest object file a reuse load will read. A bigger file is refused.
const OBJECT_READ_CAP: u64 = 512 * 1024 * 1024;

/// Lease written with a snapshot. Drop releases it in the ledger; bytes stay until collection.
pub(in crate::session) struct SnapshotLease {
    root: std::path::PathBuf,
    lease: LeaseId,
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
        for _ in 0..COMMIT_ATTEMPTS {
            self.reload()?;
            ensure_root(&mut self.life, self.root_id)?;
            let available = room(capacity, self.life.admitted(), pack_committed, pack_reserved);
            if need > available {
                return Err(DctxError::CacheLimit { requested: need, available });
            }
            let mut working = self.life.clone();
            working.observe(clock_sample(now_ms));
            let segment_id = working.insert_segment(segment_len, 0, &[self.root_id]).map_err(map_cache)?;
            let generation = working
                .publish_view(self.root_id, manifest_len, &[segment_id])
                .map_err(map_cache)?;
            let lease = working.reader_begin(self.root_id, self.session).map_err(map_cache)?;
            let pinned = working.reader_confirm(lease).map_err(map_cache)?;
            if pinned != generation {
                return Err(DctxError::invalid("snapshot lease pinned a different generation"));
            }
            let manifest = view_manifest(self.root_id, segment_id.raw());
            let manifest_object = manifest_object(&working, self.root_id, generation)?;
            let mut written = Vec::new();
            if let Err(error) = write_new(
                &self.objects,
                &mut written,
                segment_id,
                bytes,
                manifest_object,
                &manifest,
            ) {
                discard(&self.objects, &written);
                if is_conflict(&error) {
                    continue;
                }
                return Err(map_cache(error));
            }
            match self.ledger.store(&working) {
                Ok(()) => {
                    self.life = working;
                    return Ok(Some(SnapshotLease { root: self.root_path.clone(), lease }));
                }
                Err(error) if is_conflict(&error) => discard(&self.objects, &written),
                Err(error) => {
                    discard(&self.objects, &written);
                    return Err(map_cache(error));
                }
            }
        }
        Err(DctxError::invalid("stale ledger"))
    }

    /// Drops unleased retention, unlinks those bytes, and leaves an active lease untouched.
    /// `apply: false` reports leases and writes nothing.
    pub(in crate::session) fn reclaim_unleased(&mut self, now_ms: u64, apply: bool) -> Result<DiskReclaim, DctxError> {
        if !apply {
            self.reload()?;
            let mut preview = self.life.clone();
            preview.observe(clock_sample(now_ms));
            let report = preview
                .purge(AdapterPurge { scope: AdapterScope::Root(self.root_id), confirm: false, dry_run: true })
                .map_err(map_cache)?;
            return Ok(DiskReclaim { reclaimed_bytes: 0, held_by_lease: report.held_by_lease });
        }
        for _ in 0..COMMIT_ATTEMPTS {
            self.reload()?;
            ensure_root(&mut self.life, self.root_id)?;
            let original = self.life.clone();
            let mut working = original.clone();
            working.observe(clock_sample(now_ms));
            let purged = working
                .purge(AdapterPurge { scope: AdapterScope::Root(self.root_id), confirm: true, dry_run: false })
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
                    return Ok(DiskReclaim { reclaimed_bytes: reclaimed, held_by_lease: purged.held_by_lease });
                }
                Err(error) if is_conflict(&error) => {}
                Err(error) => return Err(map_cache(error)),
            }
        }
        Err(DctxError::invalid("stale ledger"))
    }

    /// Bytes of the current view's segment. `Ok(None)` when the root has no readable generation.
    pub(in crate::session) fn load_current_segment(&mut self) -> Result<Option<Vec<u8>>, DctxError> {
        self.reload()?;
        let Some(generation) = self.life.current_generation(self.root_id) else {
            return Ok(None);
        };
        let Ok(manifest_id) = manifest_object(&self.life, self.root_id, generation) else { return Ok(None) };
        let Some(manifest) = read_object(&self.root_path, manifest_id.raw())? else {
            return Ok(None);
        };
        let Some(segment_raw) = segment_id_of(&manifest) else {
            return Ok(None);
        };
        read_object(&self.root_path, segment_raw)
    }

    /// Pins the current view with a new reader lease. A miss is `Ok(None)` so the caller can rebuild.
    pub(in crate::session) fn pin_current(&mut self, now_ms: u64) -> Result<Option<SnapshotLease>, DctxError> {
        for _ in 0..COMMIT_ATTEMPTS {
            self.reload()?;
            ensure_root(&mut self.life, self.root_id)?;
            let Some(generation) = self.life.current_generation(self.root_id) else {
                return Ok(None);
            };
            let mut working = self.life.clone();
            working.observe(clock_sample(now_ms));
            let lease = match working.reader_begin(self.root_id, self.session) {
                Ok(lease) => lease,
                Err(CacheError::InvalidRequest { .. }) => return Ok(None),
                Err(error) => return Err(map_cache(error)),
            };
            let pinned = match working.reader_confirm(lease) {
                Ok(pinned) => pinned,
                Err(CacheError::InvalidRequest { .. }) => return Ok(None),
                Err(error) => return Err(map_cache(error)),
            };
            if pinned != generation {
                return Ok(None);
            }
            match self.ledger.store(&working) {
                Ok(()) => {
                    self.life = working;
                    return Ok(Some(SnapshotLease { root: self.root_path.clone(), lease }));
                }
                Err(error) if is_conflict(&error) => {},
                Err(error) => return Err(map_cache(error)),
            }
        }
        Ok(None)
    }
}

pub(super) fn is_conflict(error: &CacheError) -> bool {
    matches!(
        error,
        CacheError::InvalidRequest { reason: "stale ledger" | "object already published" }
    )
}

fn room(capacity: u64, admitted: u64, pack_committed: u64, pack_reserved: u64) -> u64 {
    capacity.saturating_sub(admitted).saturating_sub(pack_committed).saturating_sub(pack_reserved)
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
        return Err(DctxError::ResourceLimit { resource: Resource::Memory, limit: OBJECT_READ_CAP });
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

fn manifest_object(life: &Lifecycle, root: RootId, generation: GenerationId) -> Result<ObjectId, DctxError> {
    let manifest_id = life
        .manifest_of(root, generation)
        .ok_or_else(|| DctxError::invalid("snapshot manifest missing"))?;
    life.manifest_object(manifest_id).ok_or_else(|| DctxError::invalid("snapshot manifest object missing"))
}

fn write_new(
    objects: &FsObjectStore,
    written: &mut Vec<ObjectId>,
    segment: ObjectId,
    segment_bytes: &[u8],
    manifest: ObjectId,
    manifest_bytes: &[u8],
) -> Result<(), CacheError> {
    objects.put_object(segment.raw(), segment_bytes)?;
    written.push(segment);
    objects.put_object(manifest.raw(), manifest_bytes)?;
    written.push(manifest);
    Ok(())
}

fn discard(objects: &FsObjectStore, written: &[ObjectId]) {
    for id in written {
        let _ = objects.unlink_object(*id);
    }
}

fn removed_ids(before: &Lifecycle, after: &Lifecycle) -> Vec<ObjectId> {
    before.object_ids().into_iter().filter(|id| after.object_state(*id).is_none()).collect()
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
    for _ in 0..COMMIT_ATTEMPTS {
        let ledger = FsLedgerStore::open(root)?;
        let loaded = ledger.load(Quota::product_default())?;
        if loaded.fault.is_some() {
            return Ok(());
        }
        let mut life = loaded.lifecycle;
        if life.reader_release(lease).is_err() {
            return Ok(());
        }
        match ledger.store(&life) {
            Ok(()) => return Ok(()),
            Err(error) if is_conflict(&error) => {},
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_conflict;
    use dctx_adapter_cache::CacheError;

    #[test]
    fn conflict_reasons_retry_and_a_cap_refusal_does_not() {
        let stale = CacheError::InvalidRequest { reason: "stale ledger" };
        let published = CacheError::InvalidRequest { reason: "object already published" };
        let cap = CacheError::CacheLimit { requested: 4, available: 1, pinned: 0, pending: 0 };
        assert!(is_conflict(&stale));
        assert!(is_conflict(&published));
        assert!(!is_conflict(&cap));
    }
}
