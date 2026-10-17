//! Filesystem ledger and the in-memory `Cache` port.
//! Implements CACHE-v1 publication of the lifecycle image: temp file, sync, then rename.
//! Depends on [`crate::Lifecycle`] and std filesystem calls. Load reports orphans and does not delete them.
//! Object bytes live in [`cache`]. A symlink is never a reason to touch a path outside the cache root.

mod cache;

use crate::domain::{Lifecycle, ObjectId, Quota, LEDGER_MAX_BYTES};
use crate::error::CacheError;
use crate::ports::{LedgerStore, LoadedLedger, ObjectStore, OrphanFile};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

pub use cache::FsObjectStore;
pub use dctx_ports::Cache;

/// Committed ledger name inside the cache root.
pub const LEDGER_FILE: &str = "ledger.v1";
/// Temp name used before the atomic rename. A crash leaves this file in place.
pub const LEDGER_TEMP: &str = "ledger.v1.tmp";

/// Versioned ledger file for one cache root.
#[derive(Debug)]
pub struct FsLedgerStore {
    root: PathBuf,
}

impl FsLedgerStore {
    /// `root` must already be a real directory. A symlink root is refused.
    pub fn open(root: &Path) -> Result<Self, CacheError> {
        Ok(Self { root: real_root(root)? })
    }

    /// Writes and syncs the temp file. The committed name is unchanged until [`Self::commit`].
    pub fn stage(&self, ledger: &Lifecycle) -> Result<(), CacheError> {
        let bytes = ledger.encode_ledger()?;
        let path = self.root.join(LEDGER_TEMP);
        write_private(&path, &bytes)
    }

    /// Renames the temp file over the committed ledger and syncs the directory.
    pub fn commit(&self) -> Result<(), CacheError> {
        let from = self.root.join(LEDGER_TEMP);
        let to = self.root.join(LEDGER_FILE);
        match fs::symlink_metadata(&from) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(CacheError::LedgerIo { reason: "missing temp" });
            }
            Err(_) => return Err(CacheError::LedgerIo { reason: "metadata" }),
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
                return Err(CacheError::OutsideCacheRoot);
            }
            Ok(_) => {}
        }
        if let Ok(meta) = fs::symlink_metadata(&to)
            && meta.file_type().is_symlink()
        {
            return Err(CacheError::OutsideCacheRoot);
        }
        fs::rename(&from, &to).map_err(|_| CacheError::LedgerIo { reason: "rename" })?;
        sync_dir(&self.root)
    }

    fn empty_load(&self, empty_quota: Quota, fault: Option<CacheError>) -> Result<LoadedLedger, CacheError> {
        Ok(LoadedLedger {
            lifecycle: Lifecycle::new(empty_quota)?,
            fault,
            orphans: list_orphans(&self.root, &BTreeSet::new())?,
        })
    }
}

impl LedgerStore for FsLedgerStore {
    fn load(&self, empty_quota: Quota) -> Result<LoadedLedger, CacheError> {
        let path = self.root.join(LEDGER_FILE);
        match read_committed(&path)? {
            LedgerRead::Absent => Ok(self.empty_load(empty_quota, None)?),
            LedgerRead::Rejected(error) => Ok(self.empty_load(empty_quota, Some(error))?),
            LedgerRead::Bytes(bytes) => match Lifecycle::decode_ledger(&bytes) {
                Ok(lifecycle) => {
                    let known = lifecycle.object_ids().into_iter().map(ObjectId::raw).collect();
                    Ok(LoadedLedger {
                        lifecycle,
                        fault: None,
                        orphans: list_orphans(&self.root, &known)?,
                    })
                }
                Err(error @ (CacheError::CorruptLedger { .. } | CacheError::UnknownLedgerVersion { .. })) => {
                    Ok(self.empty_load(empty_quota, Some(error))?)
                }
                Err(error) => Err(error),
            },
        }
    }

    fn store(&self, ledger: &Lifecycle) -> Result<(), CacheError> {
        self.stage(ledger)?;
        self.commit()
    }
}

/// Runs one auto-clean slice, unlinks objects the slice dropped, and stores the ledger when it ran.
pub fn auto_clean(
    ledger: &FsLedgerStore,
    objects: &FsObjectStore,
    life: &mut Lifecycle,
    clock: crate::ClockSample,
    policy: crate::AutoCleanPolicy,
) -> Result<crate::MaintainReport, CacheError> {
    let before = life.object_ids();
    let report = life.auto_clean(clock, policy)?;
    for id in before {
        if life.object_state(id).is_none() {
            objects.unlink_object(id)?;
        }
    }
    if report.ran {
        ledger.store(life)?;
    }
    Ok(report)
}

pub(super) fn real_root(root: &Path) -> Result<PathBuf, CacheError> {
    if !root.is_absolute() {
        return Err(CacheError::InvalidRequest { reason: "cache root must be absolute" });
    }
    if root.components().any(|component| matches!(component, Component::ParentDir)) {
        return Err(CacheError::OutsideCacheRoot);
    }
    let meta = fs::symlink_metadata(root).map_err(|_| CacheError::LedgerIo { reason: "metadata" })?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(CacheError::InvalidRequest { reason: "cache root is not a directory" });
    }
    Ok(root.to_path_buf())
}

pub(super) fn locate(root: &Path, relative: &Path) -> Result<Option<PathBuf>, CacheError> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(CacheError::OutsideCacheRoot);
    }
    let mut cursor = root.to_path_buf();
    let mut saw_name = false;
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => {
                match fs::symlink_metadata(&cursor) {
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                    Err(_) => return Err(CacheError::LedgerIo { reason: "metadata" }),
                    Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                        return Err(CacheError::OutsideCacheRoot);
                    }
                    Ok(_) => {}
                }
                cursor.push(name);
                if !cursor.starts_with(root) {
                    return Err(CacheError::OutsideCacheRoot);
                }
                saw_name = true;
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CacheError::OutsideCacheRoot);
            }
        }
    }
    if !saw_name || cursor == root {
        return Err(CacheError::OutsideCacheRoot);
    }
    Ok(Some(cursor))
}

pub(super) fn ensure_dir(path: &Path) -> Result<(), CacheError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => return Err(CacheError::OutsideCacheRoot),
        Ok(meta) if meta.is_dir() => return Ok(()),
        Ok(_) => {
            return Err(CacheError::InvalidRequest { reason: "cache object path is not a directory" });
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(CacheError::LedgerIo { reason: "metadata" }),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(path).map_err(|_| CacheError::LedgerIo { reason: "create directory" })?;
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path).map_err(|_| CacheError::LedgerIo { reason: "create directory" })?;
    }
    Ok(())
}

pub(super) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), CacheError> {
    if let Ok(meta) = fs::symlink_metadata(path)
        && (meta.file_type().is_symlink() || meta.is_dir())
    {
        return Err(CacheError::OutsideCacheRoot);
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        options.custom_flags(no_follow());
    }
    let mut file = options.open(path).map_err(|_| CacheError::LedgerIo { reason: "create" })?;
    file.write_all(bytes).map_err(|_| CacheError::LedgerIo { reason: "write" })?;
    file.sync_all().map_err(|_| CacheError::LedgerIo { reason: "sync" })?;
    Ok(())
}

pub(super) fn sync_dir(path: &Path) -> Result<(), CacheError> {
    #[cfg(unix)]
    {
        let dir = File::open(path).map_err(|_| CacheError::LedgerIo { reason: "directory sync" })?;
        dir.sync_all().map_err(|_| CacheError::LedgerIo { reason: "directory sync" })?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

pub(super) fn list_orphans(root: &Path, known: &BTreeSet<u64>) -> Result<Vec<OrphanFile>, CacheError> {
    let mut found = Vec::new();
    walk(root, "", known, &mut found)?;
    found.sort();
    Ok(found)
}

fn walk(dir: &Path, prefix: &str, known: &BTreeSet<u64>, found: &mut Vec<OrphanFile>) -> Result<(), CacheError> {
    let entries = fs::read_dir(dir).map_err(|_| CacheError::LedgerIo { reason: "read directory" })?;
    for entry in entries {
        let entry = entry.map_err(|_| CacheError::LedgerIo { reason: "read directory" })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|_| CacheError::LedgerIo { reason: "metadata" })?;
        if meta.file_type().is_symlink() {
            if relative != LEDGER_FILE {
                found.push(OrphanFile { relative });
            }
            continue;
        }
        if meta.is_dir() {
            walk(&path, &relative, known, found)?;
            continue;
        }
        if relative == LEDGER_FILE || known_object(&relative, known) {
            continue;
        }
        found.push(OrphanFile { relative });
    }
    Ok(())
}

fn known_object(relative: &str, known: &BTreeSet<u64>) -> bool {
    let Some(name) = relative.strip_prefix("objects/") else {
        return false;
    };
    if name.contains('/') {
        return false;
    }
    let Ok(id) = name.parse::<u64>() else {
        return false;
    };
    id.to_string() == name && known.contains(&id)
}

enum LedgerRead {
    Absent,
    Bytes(Vec<u8>),
    Rejected(CacheError),
}

fn read_committed(path: &Path) -> Result<LedgerRead, CacheError> {
    let limit = u64::try_from(LEDGER_MAX_BYTES).unwrap_or(u64::MAX);
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(LedgerRead::Absent),
        Err(_) => return Err(CacheError::LedgerIo { reason: "metadata" }),
        Ok(meta) if meta.file_type().is_symlink() => {
            return Ok(LedgerRead::Rejected(CacheError::CorruptLedger { reason: "ledger symlink" }));
        }
        Ok(meta) if !meta.is_file() => {
            return Ok(LedgerRead::Rejected(CacheError::CorruptLedger { reason: "ledger magic" }));
        }
        Ok(meta) if meta.len() > limit => {
            return Ok(LedgerRead::Rejected(CacheError::CorruptLedger { reason: "ledger is too large" }));
        }
        Ok(_) => {}
    }
    match fs::read(path) {
        Ok(bytes) => Ok(LedgerRead::Bytes(bytes)),
        Err(_) => Err(CacheError::LedgerIo { reason: "read" }),
    }
}

#[cfg(unix)]
const fn no_follow() -> i32 {
    #[cfg(target_os = "linux")]
    {
        0x20000
    }
    #[cfg(target_os = "macos")]
    {
        0x0100
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CacheError, Lifecycle, Quota, LEDGER_VERSION};
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
        assert!(root.join(LEDGER_TEMP).is_file());
        let loaded = store.load(Quota::scaled(10)).expect("load");
        assert!(loaded.fault.is_none());
        assert_eq!(loaded.lifecycle.encode_ledger().expect("encode"), bytes);
        assert!(root.join(LEDGER_TEMP).is_file());
        store.commit().expect("commit");
        let loaded = store.load(Quota::scaled(10)).expect("load");
        assert_eq!(loaded.lifecycle.encode_ledger().expect("encode"), second.encode_ledger().expect("encode"));
        assert!(!root.join(LEDGER_TEMP).exists());
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
        assert!(matches!(loaded.fault, Some(CacheError::CorruptLedger { reason: "checksum" })));
        assert_eq!(loaded.lifecycle.status().hard_cap, 15);
        assert_eq!(loaded.lifecycle.admitted(), 0);
        assert_eq!(fs::read(root.join("notes.txt")).expect("notes"), b"keep");
        assert!(path.is_file());
        assert!(loaded.orphans.iter().any(|file| file.relative == "notes.txt"));

        let mut unknown = life.encode_ledger().expect("encode");
        unknown[8..12].copy_from_slice(&99u32.to_le_bytes());
        assert_ne!(99, LEDGER_VERSION);
        fs::write(&path, &unknown).expect("write");
        fs::write(root.join("stray.bin"), b"x").expect("stray");
        let loaded = store.load(Quota::scaled(15)).expect("load");
        assert!(matches!(loaded.fault, Some(CacheError::UnknownLedgerVersion { found: 99 })));
        assert_eq!(fs::read(root.join("stray.bin")).expect("stray"), b"x");
        assert_eq!(fs::read(root.join("notes.txt")).expect("notes"), b"keep");
    }
}
