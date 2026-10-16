//! In-memory disk with crash cuts, a byte budget, and Windows sharing rules.
//! Implements the DX-C3 crash matrix and the Windows open-file replacement tests.
//! Depends on [`crate::ports::StoreFs`]. [`super::store::SegmentStore`] publishes through it.
//! Synced names survive `crash`. Dirty bytes and unsynced renames do not. PID age never clears a stuck lock.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::domain::{check_rel, Boundary, StoreError};
use crate::ports::{FsCapabilities, StoreFs};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Profile {
    Posix,
    Windows,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LockKind {
    Held,
    Stuck,
}

struct Inode {
    durable: Vec<u8>,
    dirty: Option<Vec<u8>>,
    holds: u32,
}

struct Mem {
    inodes: BTreeMap<u64, Inode>,
    durable_names: BTreeMap<String, u64>,
    dirty_names: Option<BTreeMap<String, u64>>,
    next_inode: u64,
    holds: BTreeMap<u64, u64>,
    next_hold: u64,
    locks: BTreeMap<String, LockKind>,
    crash_at: Option<Boundary>,
    disk_left: Option<u64>,
    profile: Profile,
    can_sync: bool,
    can_replace: bool,
    os_lock: bool,
}

impl Mem {
    fn visible(&self) -> &BTreeMap<String, u64> {
        self.dirty_names.as_ref().unwrap_or(&self.durable_names)
    }

    fn dirty(&mut self) -> Result<&mut BTreeMap<String, u64>, StoreError> {
        if self.dirty_names.is_none() {
            self.dirty_names = Some(self.durable_names.clone());
        }
        self.dirty_names.as_mut().ok_or_else(|| StoreError::io("dir"))
    }

    fn inode_mut(&mut self, id: u64) -> Result<&mut Inode, StoreError> {
        self.inodes.get_mut(&id).ok_or_else(|| StoreError::io("inode"))
    }

    fn reap(&mut self) {
        let mut live = BTreeSet::new();
        for id in self.durable_names.values() {
            live.insert(*id);
        }
        if let Some(dirty) = &self.dirty_names {
            for id in dirty.values() {
                live.insert(*id);
            }
        }
        for id in self.holds.values() {
            live.insert(*id);
        }
        self.inodes.retain(|id, inode| live.contains(id) || inode.holds > 0);
    }

    fn crash_locked(&mut self) {
        self.dirty_names = None;
        for inode in self.inodes.values_mut() {
            inode.dirty = None;
        }
        self.locks.retain(|_, kind| *kind == LockKind::Stuck);
        self.reap();
    }
}

/// Process-local disk. Clones share the same bytes.
#[derive(Clone)]
pub struct MemoryFs {
    inner: std::sync::Arc<Mutex<Mem>>,
}

impl MemoryFs {
    /// POSIX rename-over and sync.
    pub fn posix() -> Self {
        Self::build(Profile::Posix, true, true, true)
    }

    /// Replace and unlink fail while a handle is open. Names are not truncated.
    pub fn windows() -> Self {
        Self::build(Profile::Windows, true, true, true)
    }

    /// Sync returns [`StoreError::UnsupportedDurability`].
    pub fn no_sync() -> Self {
        Self::build(Profile::Posix, false, true, true)
    }

    fn build(profile: Profile, can_sync: bool, can_replace: bool, os_lock: bool) -> Self {
        Self {
            inner: std::sync::Arc::new(Mutex::new(Mem {
                inodes: BTreeMap::new(),
                durable_names: BTreeMap::new(),
                dirty_names: None,
                next_inode: 1,
                holds: BTreeMap::new(),
                next_hold: 1,
                locks: BTreeMap::new(),
                crash_at: None,
                disk_left: None,
                profile,
                can_sync,
                can_replace,
                os_lock,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Mem> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Arms the next matching publication cut. `None` disarms it.
    pub fn arm_crash(&self, boundary: Option<Boundary>) {
        self.lock().crash_at = boundary;
    }

    /// Drops dirty bytes and releases an ordinary lock. A stuck lock remains.
    pub fn crash(&self) {
        self.lock().crash_locked();
    }

    /// Remaining write budget. `None` is unlimited. `Some(0)` refuses the next non-empty write.
    pub fn set_disk_budget(&self, bytes: Option<u64>) {
        self.lock().disk_left = bytes;
    }

    /// Installs a lock that crash and unlock do not clear. This is the P8 mutation.
    pub fn stick_lock(&self, root: &str) {
        self.lock().locks.insert(root.to_string(), LockKind::Stuck);
    }

    /// Opens a handle. On the Windows profile this blocks replace and unlink.
    pub fn hold(&self, path: &str) -> Result<u64, StoreError> {
        check_rel(path)?;
        let mut mem = self.lock();
        let id = *mem.visible().get(path).ok_or_else(|| StoreError::io("missing"))?;
        let inode = mem.inode_mut(id)?;
        if inode.holds == u32::MAX {
            return Err(StoreError::InvalidRequest { reason: "hold" });
        }
        inode.holds += 1;
        let token = mem.next_hold;
        mem.next_hold += 1;
        mem.holds.insert(token, id);
        Ok(token)
    }

    /// Closes a handle from [`Self::hold`].
    pub fn release_hold(&self, token: u64) -> Result<(), StoreError> {
        let mut mem = self.lock();
        let id = mem.holds.remove(&token).ok_or(StoreError::InvalidRequest { reason: "hold" })?;
        let inode = mem.inode_mut(id)?;
        inode.holds = inode.holds.saturating_sub(1);
        Ok(())
    }

    /// Bytes still visible through an open handle, including after a POSIX replace.
    pub fn read_hold(&self, token: u64) -> Result<Vec<u8>, StoreError> {
        let mem = self.lock();
        let id = *mem.holds.get(&token).ok_or(StoreError::InvalidRequest { reason: "hold" })?;
        let inode = mem.inodes.get(&id).ok_or_else(|| StoreError::io("inode"))?;
        Ok(inode.dirty.clone().unwrap_or_else(|| inode.durable.clone()))
    }
}

impl StoreFs for MemoryFs {
    fn capabilities(&self) -> FsCapabilities {
        let mem = self.lock();
        FsCapabilities {
            sync: mem.can_sync,
            atomic_replace: mem.can_replace,
            os_lock: mem.os_lock,
        }
    }

    fn checkpoint(&self, boundary: Boundary) -> Result<(), StoreError> {
        let mut mem = self.lock();
        if mem.crash_at == Some(boundary) {
            mem.crash_at = None;
            mem.crash_locked();
            return Err(StoreError::Crashed { boundary: boundary.as_str() });
        }
        Ok(())
    }

    fn try_lock(&self, root: &str) -> Result<(), StoreError> {
        let mut mem = self.lock();
        if !mem.os_lock {
            return Err(StoreError::UnsupportedDurability { capability: "lock" });
        }
        if mem.locks.contains_key(root) {
            return Err(StoreError::Busy);
        }
        mem.locks.insert(root.to_string(), LockKind::Held);
        Ok(())
    }

    fn unlock(&self, root: &str) -> Result<(), StoreError> {
        let mut mem = self.lock();
        if matches!(mem.locks.get(root), Some(LockKind::Stuck)) {
            return Ok(());
        }
        mem.locks.remove(root);
        Ok(())
    }

    fn lock_held(&self, root: &str) -> bool {
        self.lock().locks.contains_key(root)
    }

    fn held_roots(&self) -> Vec<String> {
        self.lock().locks.keys().cloned().collect()
    }

    fn create(&self, path: &str) -> Result<(), StoreError> {
        check_rel(path)?;
        let mut mem = self.lock();
        if mem.visible().contains_key(path) {
            return Err(StoreError::io("exists"));
        }
        let id = mem.next_inode;
        mem.next_inode += 1;
        mem.inodes.insert(
            id,
            Inode {
                durable: Vec::new(),
                dirty: None,
                holds: 0,
            },
        );
        mem.dirty()?.insert(path.to_string(), id);
        Ok(())
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        check_rel(path)?;
        let mut mem = self.lock();
        let len = u64::try_from(bytes.len()).map_err(|_| StoreError::InvalidRequest { reason: "size" })?;
        let id = *mem.visible().get(path).ok_or_else(|| StoreError::io("missing"))?;
        if let Some(left) = mem.disk_left {
            if len > left {
                return Err(StoreError::DiskFull);
            }
            mem.disk_left = Some(left - len);
        }
        mem.inode_mut(id)?.dirty = Some(bytes.to_vec());
        Ok(())
    }

    fn flush(&self, path: &str) -> Result<(), StoreError> {
        check_rel(path)?;
        let mem = self.lock();
        if !mem.visible().contains_key(path) {
            return Err(StoreError::io("missing"));
        }
        Ok(())
    }

    fn sync_file(&self, path: &str) -> Result<(), StoreError> {
        check_rel(path)?;
        let mut mem = self.lock();
        if !mem.can_sync {
            return Err(StoreError::UnsupportedDurability { capability: "sync" });
        }
        let id = *mem.visible().get(path).ok_or_else(|| StoreError::io("missing"))?;
        let inode = mem.inode_mut(id)?;
        if let Some(dirty) = inode.dirty.take() {
            inode.durable = dirty;
        }
        Ok(())
    }

    fn sync_dir(&self) -> Result<(), StoreError> {
        let mut mem = self.lock();
        if !mem.can_sync {
            return Err(StoreError::UnsupportedDurability { capability: "sync" });
        }
        let visible = mem.visible().clone();
        mem.durable_names = visible;
        mem.dirty_names = None;
        mem.reap();
        Ok(())
    }

    fn rename_new(&self, from: &str, to: &str) -> Result<(), StoreError> {
        check_rel(from)?;
        check_rel(to)?;
        let mut mem = self.lock();
        if mem.visible().contains_key(to) {
            return Err(StoreError::io("exists"));
        }
        let id = *mem.visible().get(from).ok_or_else(|| StoreError::io("missing"))?;
        let dirty = mem.dirty()?;
        dirty.remove(from);
        dirty.insert(to.to_string(), id);
        Ok(())
    }

    fn rename_replace(&self, from: &str, to: &str) -> Result<(), StoreError> {
        check_rel(from)?;
        check_rel(to)?;
        let mut mem = self.lock();
        if !mem.can_replace {
            return Err(StoreError::UnsupportedDurability { capability: "atomic-replace" });
        }
        let id = *mem.visible().get(from).ok_or_else(|| StoreError::io("missing"))?;
        if let Some(dest) = mem.visible().get(to).copied()
            && dest != id
        {
            let holds = mem.inodes.get(&dest).map(|inode| inode.holds).unwrap_or(0);
            if mem.profile == Profile::Windows && holds > 0 {
                return Err(StoreError::SharingViolation);
            }
        }
        let dirty = mem.dirty()?;
        dirty.remove(from);
        dirty.insert(to.to_string(), id);
        Ok(())
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, StoreError> {
        check_rel(path)?;
        let mem = self.lock();
        let id = *mem.visible().get(path).ok_or_else(|| StoreError::io("missing"))?;
        let inode = mem.inodes.get(&id).ok_or_else(|| StoreError::io("inode"))?;
        Ok(inode.dirty.clone().unwrap_or_else(|| inode.durable.clone()))
    }

    fn exists(&self, path: &str) -> Result<bool, StoreError> {
        check_rel(path)?;
        Ok(self.lock().visible().contains_key(path))
    }

    fn len(&self, path: &str) -> Result<u64, StoreError> {
        check_rel(path)?;
        let mem = self.lock();
        let id = *mem.visible().get(path).ok_or_else(|| StoreError::io("missing"))?;
        let inode = mem.inodes.get(&id).ok_or_else(|| StoreError::io("inode"))?;
        let size = inode.dirty.as_ref().map_or(inode.durable.len(), Vec::len);
        u64::try_from(size).map_err(|_| StoreError::InvalidRequest { reason: "size" })
    }

    fn unlink(&self, path: &str) -> Result<(), StoreError> {
        check_rel(path)?;
        let mut mem = self.lock();
        let id = *mem.visible().get(path).ok_or_else(|| StoreError::io("missing"))?;
        let holds = mem.inodes.get(&id).map(|inode| inode.holds).unwrap_or(0);
        if mem.profile == Profile::Windows && holds > 0 {
            return Err(StoreError::SharingViolation);
        }
        mem.dirty()?.remove(path);
        Ok(())
    }

    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let mem = self.lock();
        Ok(mem
            .visible()
            .keys()
            .filter(|name| name.starts_with(prefix))
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_keeps_synced_names_and_drops_dirty_renames() {
        let fs = MemoryFs::posix();
        fs.create("segments/a.seg").unwrap();
        fs.write("segments/a.seg", b"one").unwrap();
        fs.sync_file("segments/a.seg").unwrap();
        fs.sync_dir().unwrap();
        fs.create("tmp/b.partial").unwrap();
        fs.write("tmp/b.partial", b"two").unwrap();
        fs.sync_file("tmp/b.partial").unwrap();
        fs.rename_new("tmp/b.partial", "segments/b.seg").unwrap();
        fs.crash();
        assert_eq!(fs.read("segments/a.seg").unwrap(), b"one");
        assert!(!fs.exists("segments/b.seg").unwrap());
        fs.create("tmp/c.partial").unwrap();
        fs.write("tmp/c.partial", b"three").unwrap();
        fs.sync_file("tmp/c.partial").unwrap();
        fs.rename_new("tmp/c.partial", "segments/c.seg").unwrap();
        fs.sync_dir().unwrap();
        fs.crash();
        assert_eq!(fs.read("segments/c.seg").unwrap(), b"three");
    }

    #[test]
    fn windows_hold_blocks_replace_and_unlink_without_truncation() {
        let fs = MemoryFs::windows();
        fs.create("roots/main/current").unwrap();
        fs.write("roots/main/current", b"old-pointer").unwrap();
        fs.sync_file("roots/main/current").unwrap();
        fs.sync_dir().unwrap();
        let held = fs.hold("roots/main/current").unwrap();
        fs.create("tmp/pointer.partial").unwrap();
        fs.write("tmp/pointer.partial", b"new-pointer").unwrap();
        assert_eq!(
            fs.rename_replace("tmp/pointer.partial", "roots/main/current").unwrap_err(),
            StoreError::SharingViolation
        );
        assert_eq!(fs.unlink("roots/main/current").unwrap_err(), StoreError::SharingViolation);
        assert_eq!(fs.read("roots/main/current").unwrap(), b"old-pointer");
        fs.release_hold(held).unwrap();
        fs.rename_replace("tmp/pointer.partial", "roots/main/current").unwrap();
        assert_eq!(fs.read("roots/main/current").unwrap(), b"new-pointer");

        let posix = MemoryFs::posix();
        posix.create("roots/main/current").unwrap();
        posix.write("roots/main/current", b"old-pointer").unwrap();
        let open = posix.hold("roots/main/current").unwrap();
        posix.create("tmp/pointer.partial").unwrap();
        posix.write("tmp/pointer.partial", b"new-pointer").unwrap();
        posix.rename_replace("tmp/pointer.partial", "roots/main/current").unwrap();
        assert_eq!(posix.read_hold(open).unwrap(), b"old-pointer");
        assert_eq!(posix.read("roots/main/current").unwrap(), b"new-pointer");
    }

    #[test]
    fn disk_budget_refuses_the_write_and_a_stuck_lock_survives_crash() {
        let fs = MemoryFs::posix();
        fs.create("segments/a.seg").unwrap();
        fs.set_disk_budget(Some(0));
        assert_eq!(fs.write("segments/a.seg", b"x").unwrap_err(), StoreError::DiskFull);
        assert!(fs.read("segments/a.seg").unwrap().is_empty());
        fs.set_disk_budget(None);
        fs.write("segments/a.seg", b"ok").unwrap();
        assert_eq!(fs.read("segments/a.seg").unwrap(), b"ok");
        fs.try_lock("main").unwrap();
        fs.crash();
        assert!(!fs.lock_held("main"));
        fs.stick_lock("main");
        fs.crash();
        assert!(fs.lock_held("main"));
        assert_eq!(fs.try_lock("main").unwrap_err(), StoreError::Busy);
        fs.unlock("main").unwrap();
        assert!(fs.lock_held("main"));
    }
}
