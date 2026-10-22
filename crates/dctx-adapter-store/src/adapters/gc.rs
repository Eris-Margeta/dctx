//! Reachability GC for immutable segment and manifest names.
//! Implements CACHE-v1 reclamation and the P3 mark set: pointers, writer protections, and reader leases.
//! Depends on [`super::store`] closures. Unlink never truncates a live inode.
//! [`crate::domain::MarkMode::OmitLeases`] is the falsifying mutation. A sharing violation stays pending.

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use crate::domain::{
    current_path, cursor_path, object_digest, previous_path, sha256, GcBudget, GcReport, MarkMode,
    StoreError,
};
use crate::ports::StoreFs;

use super::store::{inspect, PointerView, SegmentStore};

const CURSOR_MAGIC: [u8; 8] = *b"DCTXCUR\0";

enum Remove {
    Unlinked,
    Blocked,
    Missing,
}

impl<F: StoreFs> SegmentStore<F> {
    /// Unlinks objects outside the retained closures, writer set, and active pins.
    ///
    /// # Errors
    /// Refuses a crashed store or cancellation; propagates mark, cursor, unlink and durability errors.
    pub fn gc(&self, budget: GcBudget) -> Result<GcReport, StoreError> {
        self.gc_with_mark(budget, MarkMode::Reachability)
    }

    /// [`Self::gc`] with an explicit mark set. Production callers pass [`MarkMode::Reachability`].
    ///
    /// # Errors
    /// Refuses a crashed store or cancellation; propagates mark, cursor, unlink and durability errors.
    pub fn gc_with_mark(&self, budget: GcBudget, mode: MarkMode) -> Result<GcReport, StoreError> {
        self.ensure_live()?;
        if self.pull_cancel() {
            return Err(StoreError::Cancelled);
        }
        let mut unlinked = 0u64;
        let mut room = budget.objects;
        unlinked += self.retry_pending(mode, &mut room)?;
        if room > 0 {
            unlinked += self.sweep(mode, &mut room)?;
        }
        if unlinked > 0 {
            self.sync_dir()?;
        }
        self.report(mode, unlinked)
    }

    /// Removes the previous pointer once the current closure validates.
    ///
    /// # Errors
    /// Refuses a crashed store, invalid root, busy lock or invalid current closure; propagates unlink and directory-sync errors.
    pub fn drop_previous(&self, root: &str) -> Result<(), StoreError> {
        self.ensure_live()?;
        crate::domain::check_root(root)?;
        self.fs.try_lock(root)?;
        let result = self.drop_previous_locked(root);
        let _ = self.fs.unlock(root);
        result
    }

    /// Removes a root's current and previous pointers. Segment bytes stay until leases allow GC.
    ///
    /// # Errors
    /// Refuses a crashed store, invalid root or busy lock; propagates pointer removal and directory-sync errors.
    pub fn retire_root(&self, root: &str) -> Result<(), StoreError> {
        self.ensure_live()?;
        crate::domain::check_root(root)?;
        self.fs.try_lock(root)?;
        let result = self.unlink_pointers(root);
        let _ = self.fs.unlock(root);
        result
    }

    fn drop_previous_locked(&self, root: &str) -> Result<(), StoreError> {
        if !matches!(inspect(&self.fs, &current_path(root))?, PointerView::Closed(_)) {
            return Err(StoreError::CorruptIndex { reason: "pointer" });
        }
        self.unlink_if_present(&previous_path(root))?;
        self.sync_dir()
    }

    fn unlink_pointers(&self, root: &str) -> Result<(), StoreError> {
        self.unlink_if_present(&current_path(root))?;
        self.unlink_if_present(&previous_path(root))?;
        self.sync_dir()
    }

    fn unlink_if_present(&self, path: &str) -> Result<(), StoreError> {
        if self.fs.exists(path)? {
            self.fs.unlink(path)?;
        }
        Ok(())
    }

    fn retry_pending(&self, mode: MarkMode, room: &mut u32) -> Result<u64, StoreError> {
        let pending: Vec<String> = self.coord().pending.iter().cloned().collect();
        let mut unlinked = 0u64;
        for path in pending {
            if *room == 0 {
                break;
            }
            if self.pull_cancel() {
                if unlinked > 0 {
                    self.sync_dir()?;
                }
                return Err(StoreError::Cancelled);
            }
            if self.fresh_marked(&path, mode)? {
                self.coord().pending.remove(&path);
                continue;
            }
            match self.remove_name(&path)? {
                Remove::Unlinked => {
                    unlinked += 1;
                    *room -= 1;
                    self.coord().pending.remove(&path);
                }
                Remove::Missing => {
                    self.coord().pending.remove(&path);
                }
                Remove::Blocked => {}
            }
        }
        Ok(unlinked)
    }

    fn sweep(&self, mode: MarkMode, room: &mut u32) -> Result<u64, StoreError> {
        let marked = self.mark_set(mode)?;
        let cursor = self.load_cursor()?;
        let mut victims = victim_names(&self.fs, &marked)?;
        victims.retain(|name| name.as_str() >= cursor.as_str());
        let mut unlinked = 0u64;
        let mut index = 0usize;
        while index < victims.len() && *room > 0 {
            if self.pull_cancel() {
                self.sync_dir()?;
                self.store_cursor(victims.get(index).map_or("", String::as_str))?;
                return Err(StoreError::Cancelled);
            }
            let path = victims[index].clone();
            if self.fresh_marked(&path, mode)? {
                index += 1;
                continue;
            }
            match self.remove_name(&path)? {
                Remove::Unlinked => {
                    unlinked += 1;
                    *room -= 1;
                    self.coord().pending.remove(&path);
                    index += 1;
                    self.remember_cursor(&victims, index)?;
                }
                Remove::Blocked => {
                    self.coord().pending.insert(path);
                    index += 1;
                    self.remember_cursor(&victims, index)?;
                }
                Remove::Missing => index += 1,
            }
        }
        if index >= victims.len() {
            self.clear_cursor()?;
        }
        Ok(unlinked)
    }

    fn remember_cursor(&self, victims: &[String], index: usize) -> Result<(), StoreError> {
        match victims.get(index) {
            Some(next) => self.store_cursor(next),
            None => self.clear_cursor(),
        }
    }

    fn report(&self, mode: MarkMode, unlinked: u64) -> Result<GcReport, StoreError> {
        let marked = self.mark_set(mode)?;
        let left = victim_names(&self.fs, &marked)?;
        let pending = u64::try_from(self.coord().pending.len()).unwrap_or(u64::MAX);
        Ok(GcReport {
            unlinked,
            pending,
            kept: u64::try_from(marked.len()).unwrap_or(u64::MAX),
            done: left.is_empty() && pending == 0,
        })
    }

    fn fresh_marked(&self, path: &str, mode: MarkMode) -> Result<bool, StoreError> {
        if self.coord().protecting.contains(path) {
            return Ok(true);
        }
        if path.starts_with("tmp/") {
            return Ok(false);
        }
        let Some(digest) = object_digest(path) else {
            return Ok(false);
        };
        Ok(self.mark_set(mode)?.contains(&digest))
    }

    fn mark_set(&self, mode: MarkMode) -> Result<BTreeSet<[u8; 32]>, StoreError> {
        let mut marked = BTreeSet::new();
        for root in self.roots()? {
            for path in [current_path(&root), previous_path(&root)] {
                if let PointerView::Closed(closure) = inspect(&self.fs, &path)? {
                    marked.insert(closure.manifest);
                    for segment in closure.segments {
                        marked.insert(segment.digest);
                    }
                }
            }
        }
        let coord = self.coord();
        marked.extend(coord.writing.iter().copied());
        if mode == MarkMode::Reachability {
            for lease in &coord.leases {
                marked.insert(lease.manifest);
                for segment in &lease.segments {
                    marked.insert(*segment.digest());
                }
            }
        }
        Ok(marked)
    }

    fn remove_name(&self, path: &str) -> Result<Remove, StoreError> {
        if !self.fs.exists(path)? {
            return Ok(Remove::Missing);
        }
        match self.fs.unlink(path) {
            Ok(()) => Ok(Remove::Unlinked),
            Err(StoreError::SharingViolation) => Ok(Remove::Blocked),
            Err(StoreError::Io { detail }) if detail == "missing" => Ok(Remove::Missing),
            Err(err) => Err(err),
        }
    }

    fn pull_cancel(&self) -> bool {
        self.cancel.swap(false, Ordering::SeqCst)
    }

    /// A corrupt cursor restarts the scan so a bad maintenance file cannot stall GC.
    fn load_cursor(&self) -> Result<String, StoreError> {
        let path = cursor_path();
        if !self.fs.exists(path)? {
            return Ok(String::new());
        }
        let bytes = self.fs.read(path)?;
        Ok(decode_cursor(&bytes).unwrap_or_default())
    }

    fn store_cursor(&self, name: &str) -> Result<(), StoreError> {
        if name.is_empty() {
            return self.clear_cursor();
        }
        let bytes = encode_cursor(name)?;
        self.write_durable("maintenance-cursor.partial", cursor_path(), &bytes, true, |_| Ok(()))
    }

    fn clear_cursor(&self) -> Result<(), StoreError> {
        let path = cursor_path();
        if self.fs.exists(path)? {
            self.fs.unlink(path)?;
            self.sync_dir()?;
        }
        Ok(())
    }
}

fn victim_names<F: StoreFs>(fs: &F, marked: &BTreeSet<[u8; 32]>) -> Result<Vec<String>, StoreError> {
    let mut names = Vec::new();
    for prefix in ["segments/", "manifests/", "tmp/"] {
        for path in fs.list_prefix(prefix)? {
            if path.starts_with("tmp/") {
                names.push(path);
                continue;
            }
            match object_digest(&path) {
                Some(digest) if marked.contains(&digest) => {}
                _ => names.push(path),
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

fn encode_cursor(name: &str) -> Result<Vec<u8>, StoreError> {
    let raw = name.as_bytes();
    let len = u32::try_from(raw.len()).map_err(|_| StoreError::InvalidRequest { reason: "cursor" })?;
    let mut body = Vec::with_capacity(46 + raw.len());
    body.extend_from_slice(&CURSOR_MAGIC);
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&len.to_le_bytes());
    body.extend_from_slice(raw);
    let sum = sha256(&body)?;
    body.extend_from_slice(&sum);
    Ok(body)
}

fn decode_cursor(bytes: &[u8]) -> Result<String, StoreError> {
    if bytes.len() < 46 || bytes[..8] != CURSOR_MAGIC {
        return Err(StoreError::CorruptIndex { reason: "cursor" });
    }
    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version != 1 {
        return Err(StoreError::UnsupportedVersion { found: version });
    }
    let len = usize::try_from(u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]))
        .map_err(|_| StoreError::CorruptIndex { reason: "cursor" })?;
    let end = 14usize.checked_add(len).ok_or(StoreError::CorruptIndex { reason: "cursor" })?;
    let sum_at = end.checked_add(32).ok_or(StoreError::CorruptIndex { reason: "cursor" })?;
    if bytes.len() != sum_at {
        return Err(StoreError::CorruptIndex { reason: "cursor" });
    }
    let mut stored = [0u8; 32];
    stored.copy_from_slice(&bytes[end..sum_at]);
    if sha256(&bytes[..end])? != stored {
        return Err(StoreError::CorruptIndex { reason: "cursor" });
    }
    let name = std::str::from_utf8(&bytes[14..end]).map_err(|_| StoreError::CorruptIndex { reason: "cursor" })?;
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trips_and_a_flipped_checksum_is_corrupt() {
        let bytes = encode_cursor("segments/ab.seg").unwrap();
        assert_eq!(decode_cursor(&bytes).unwrap(), "segments/ab.seg");
        let mut bad = bytes.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0xff;
        assert!(matches!(decode_cursor(&bad), Err(StoreError::CorruptIndex { .. })));
        assert_eq!(decode_cursor(&encode_cursor("").unwrap()).unwrap(), "");
    }
}
