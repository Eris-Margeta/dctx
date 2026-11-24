// SPDX-License-Identifier: Apache-2.0
//! Unix name durability for the native store.

use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use rustix::fs::{CWD, Mode, OFlags, fsync, openat};
use rustix::io::Errno;

use crate::domain::StoreError;
use crate::ports;

pub(super) type BeforeSync = dyn Fn(&Path) -> io::Result<()> + Send + Sync;

pub(super) struct DirSync {
    pending: Mutex<Pending>,
    before_sync: Option<Box<BeforeSync>>,
}

struct Pending {
    parents: BTreeSet<PathBuf>,
    root_parents: BTreeSet<PathBuf>,
    completions: Vec<PathBuf>,
}

impl DirSync {
    pub(super) fn prepare(
        path: &Path,
        before_sync: Option<Box<BeforeSync>>,
    ) -> Result<(PathBuf, Self), StoreError> {
        if path.components().any(|part| part == Component::ParentDir) {
            return Err(StoreError::InvalidRequest { reason: "root" });
        }
        let path = std::path::absolute(path).map_err(ports::io_err)?;
        // Losing a race at the first missing component can leave a shorter suffix
        // to install (for example, two stores with a common missing parent).
        loop {
            let mut existing = path.as_path();
            loop {
                match fs::symlink_metadata(existing) {
                    Ok(_) => break,
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {
                        existing = parent(existing);
                    }
                    Err(err) => return Err(ports::io_err(err)),
                }
            }
            if existing == path {
                break;
            }
            let base = checked_install_parent(existing)?;
            let destination = base.join(path.strip_prefix(existing).expect("ancestor of root"));
            let missing: Vec<_> = destination
                .ancestors()
                .take_while(|dir| *dir != base)
                .collect();
            if install_root(&missing, before_sync.as_deref())? {
                break;
            }
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_vendor = "apple",
            target_os = "redox"
        )))]
        finish_interrupted_mkdirs(&path, before_sync.as_deref())?;
        let root = ports::prepare_root(&path)?;
        // An installed subtree can be shared by stores in different branches.
        // Only a receipt whose barrier is still pending obliges us to sync its parents.
        let mut root_parents = BTreeSet::new();
        let mut completions = Vec::new();
        for dir in root.ancestors().filter(|dir| dir.parent().is_some()) {
            let (depth, completion) = pending_depth(dir)?;
            root_parents.extend(dir.ancestors().skip(1).take(depth).map(Path::to_owned));
            completions.extend(completion);
        }
        let sync = Self {
            pending: Mutex::new(Pending {
                parents: BTreeSet::from([root.clone()]),
                root_parents,
                completions,
            }),
            before_sync,
        };
        Ok((root, sync))
    }

    pub(super) fn record(&self, name: &Path) {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending.parents.insert(parent(name).to_owned());
    }

    pub(super) fn record_rename(&self, from: &Path, to: &Path) {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending.parents.insert(parent(to).to_owned());
        pending.parents.insert(parent(from).to_owned());
    }

    pub(super) fn sync_pointer_inputs(&self, root: &Path) -> Result<(), StoreError> {
        // An interrupted writer can leave visible immutable names that the next publish reuses.
        // Reissue the name barriers before syncing any pointer into place, even after reopen.
        let mut inputs = Vec::new();
        for name in ["segments", "manifests"] {
            let dir = root.join(name);
            match fs::symlink_metadata(&dir) {
                Ok(_) => inputs.push(dir),
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(sync_error(root, &dir, err)),
            }
        }
        self.sync(root, &inputs)
    }

    pub(super) fn sync(&self, root: &Path, inputs: &[PathBuf]) -> Result<(), StoreError> {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        let mut seen = BTreeSet::new();
        let mut dirs = Vec::new();
        for parent in inputs.iter().chain(&pending.parents) {
            for dir in parent.ancestors().take_while(|dir| dir.starts_with(root)) {
                if seen.insert(dir) {
                    dirs.push(dir);
                }
            }
        }
        // Sync each directory before the ancestor that names it. Keep independent
        // directories in input order, so the pointer's segment barrier precedes its manifest.
        dirs.sort_by_cached_key(|dir| Reverse(dir.components().count()));
        // The set contains only ancestors of this root. Reverse path order puts
        // each child before its prefix and visits each affected parent once.
        for dir in dirs
            .into_iter()
            .chain(pending.root_parents.iter().rev().map(PathBuf::as_path))
        {
            self.sync_one(root, dir)
                .map_err(|err| sync_error(root, dir, err))?;
        }
        for completion in &pending.completions {
            // Create-only: concurrent openers can complete the same barrier.
            match fs::File::create_new(completion) {
                Ok(_) => {}
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                // The barrier succeeded. Without a completion file, the next
                // opener will find the receipt and sync its parents again.
                Err(err)
                    if matches!(
                        Errno::from_io_error(&err),
                        Some(Errno::ACCESS | Errno::PERM | Errno::ROFS)
                    ) => {}
                Err(err) => return Err(ports::io_err(err)),
            }
        }
        pending.completions.clear();
        pending.parents.clear();
        pending.root_parents.clear();
        Ok(())
    }

    fn sync_one(&self, root: &Path, dir: &Path) -> io::Result<()> {
        if let Some(hook) = &self.before_sync {
            hook(dir)?;
        }
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let handle = if let Ok(relative) = dir.strip_prefix(root) {
            let mut handle = openat(CWD, root, flags, Mode::empty())?;
            for name in relative {
                handle = openat(&handle, name, flags, Mode::empty())?;
            }
            handle
        } else {
            openat(CWD, dir, flags, Mode::empty())?
        };
        fsync(handle).map_err(Into::into)
    }
}

// This empty-data file's length counts newly installed path components. Bind
// its reserved name to the store inode, including when moving the staged tree.
fn receipt_path(dir: &Path, id: &fs::Metadata) -> PathBuf {
    dir.join(format!(".dctx-root-sync-v1-{:x}-{:x}", id.dev(), id.ino()))
}

fn pending_depth(root: &Path) -> Result<(usize, Option<PathBuf>), StoreError> {
    let directory = fs::symlink_metadata(root).map_err(ports::io_err)?;
    let path = receipt_path(root, &directory);
    let id = match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() && meta.uid() == directory.uid() && meta.len() > 0 => meta,
        Ok(_) => return Ok((0, None)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok((0, None)),
        Err(err) => return Err(ports::io_err(err)),
    };
    let depth = usize::try_from(id.len())
        .unwrap_or(usize::MAX)
        .min(root.ancestors().count() - 1);
    let completion = path.with_extension(format!("{:x}.done", id.ino()));
    match fs::symlink_metadata(&completion) {
        Ok(meta) if meta.is_file() && meta.uid() == directory.uid() => Ok((0, None)),
        Ok(_) => Ok((depth, None)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            // A store may use an ancestor owned by another trusted user. Honor
            // that receipt, but leave recording its completion to the owner.
            let owned = directory.uid() == rustix::process::geteuid().as_raw();
            Ok((depth, owned.then_some(completion)))
        }
        Err(err) => Err(ports::io_err(err)),
    }
}

fn write_receipt(path: &Path, depth: usize) -> Result<(), StoreError> {
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(ports::io_err)?;
    file.set_len(u64::try_from(depth).expect("path depth fits in u64"))
        .and_then(|()| file.sync_all())
        .map_err(ports::io_err)
}

// Apply the ancestor rules from ports::unix_root without opening or tightening
// an ancestor as a store root. Search permission is sufficient for these checks.
fn checked_install_parent(path: &Path) -> Result<PathBuf, StoreError> {
    let current = rustix::process::geteuid().as_raw();
    let check_chain = |path: &Path, allow_links: bool| {
        let refuse = StoreError::InvalidRequest { reason: "root" };
        let mut linked = false;
        for dir in path.ancestors() {
            let meta = fs::symlink_metadata(dir).map_err(ports::io_err)?;
            let trusted_owner = meta.uid() == 0 || meta.uid() == current;
            if meta.is_symlink() {
                if !allow_links || !trusted_owner {
                    return Err(refuse);
                }
                linked = true;
                continue;
            }
            if !meta.is_dir() {
                return Err(refuse);
            }
            let root_sticky = meta.uid() == 0 && meta.mode() & 0o1000 != 0;
            if trusted_owner && (meta.mode() & 0o022 == 0 || root_sticky) {
                continue;
            }
            let read_only = rustix::fs::statvfs(dir)
                .is_ok_and(|stat| stat.f_flag.contains(rustix::fs::StatVfsMountFlags::RDONLY));
            if !read_only {
                return Err(refuse);
            }
        }
        Ok(linked)
    };
    if check_chain(path, true)? {
        let resolved = fs::canonicalize(path).map_err(ports::io_err)?;
        check_chain(&resolved, false)?;
        Ok(resolved)
    } else {
        Ok(path.to_owned())
    }
}

// Build the missing suffix privately. The first name a second opener can see
// already contains the record it needs, even if the creating process exits.
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple",
    target_os = "redox"
))]
fn install_root(missing: &[&Path], hook: Option<&BeforeSync>) -> Result<bool, StoreError> {
    use rustix::fs::{RenameFlags, renameat_with};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let outer = *missing.last().expect("missing suffix");
    let base = parent(outer);
    let stage = loop {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let candidate = base.join(format!(".dctx-init-{}-{serial}", std::process::id()));
        if candidate == outer {
            continue;
        }
        match fs::DirBuilder::new().mode(0o700).create(&candidate) {
            Ok(()) => break candidate,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(ports::io_err(err)),
        }
    };

    // Only undo entries this invocation created in its exclusive staging directory.
    let tree = stage.join("tree");
    let mut dirs = Vec::new();
    let mut receipts = Vec::new();
    let result = (|| {
        for &dir in missing.iter().rev() {
            let next = tree.join(dir.strip_prefix(outer).expect("missing suffix"));
            let mode = if dir == missing[0] { 0o700 } else { 0o777 };
            fs::DirBuilder::new()
                .mode(mode)
                .create(&next)
                .map_err(ports::io_err)?;
            dirs.push(next);
        }
        let root = dirs.last().expect("staged root");
        // The leaf records its missing suffix. The top records the one name
        // installed by rename, visible to every opener beneath that name.
        // The intervening directories are synced in staging before exposure.
        for (dir, depth) in [(root, missing.len()), (&tree, 1)] {
            let id = fs::symlink_metadata(dir).map_err(ports::io_err)?;
            let path = receipt_path(dir, &id);
            if receipts.contains(&path) {
                continue;
            }
            receipts.push(path.clone());
            write_receipt(&path, depth)?;
        }
        for dir in dirs.iter().rev() {
            sync_setup_dir(missing[0], dir, hook)?;
        }
        match renameat_with(CWD, &tree, CWD, outer, RenameFlags::NOREPLACE) {
            Ok(()) => Ok(true),
            Err(Errno::EXIST | Errno::NOTEMPTY) => Ok(false),
            Err(err) => Err(ports::io_err(err.into())),
        }
    })();
    if !matches!(result, Ok(true)) {
        for receipt in receipts {
            match fs::remove_file(receipt) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(ports::io_err(err)),
            }
        }
        for dir in dirs.iter().rev() {
            fs::remove_dir(dir).map_err(ports::io_err)?;
        }
    }
    fs::remove_dir(&stage).map_err(ports::io_err)?;
    if matches!(result, Ok(true)) {
        // A later store can share the newly installed ancestor. Finish its
        // containing-directory rename before returning the initialized store.
        sync_setup_dir(missing[0], parent(&stage), hook)?;
    }
    result
}

// POSIX mkdir is exclusive but cannot install an initialized subtree. On
// these systems persist each parent during setup. A private sticky directory
// signals the interval between mkdir and fsync to a concurrent or later opener.
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple",
    target_os = "redox"
)))]
fn install_root(missing: &[&Path], hook: Option<&BeforeSync>) -> Result<bool, StoreError> {
    use std::os::unix::fs::PermissionsExt;
    for &dir in missing.iter().rev() {
        let mode = if dir == missing[0] { 0o1700 } else { 0o1777 };
        match fs::DirBuilder::new().mode(mode).create(dir) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                finish_interrupted_mkdirs(dir, hook)?;
                return Ok(false);
            }
            Err(err) => return Err(ports::io_err(err)),
        }
        let fd = fs::File::open(dir).map_err(ports::io_err)?;
        if dir == missing[0] {
            let id = fd.metadata().map_err(ports::io_err)?;
            write_receipt(&receipt_path(dir, &id), missing.len())?;
        }
        sync_setup_dir(missing[0], parent(dir), hook)?;
        let mode = fd.metadata().map_err(ports::io_err)?.permissions().mode();
        fd.set_permissions(fs::Permissions::from_mode(mode & !0o1000))
            .map_err(ports::io_err)?;
    }
    Ok(true)
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple",
    target_os = "redox"
)))]
fn finish_interrupted_mkdirs(path: &Path, hook: Option<&BeforeSync>) -> Result<(), StoreError> {
    for dir in path.ancestors().filter(|p| p.parent().is_some()) {
        match fs::symlink_metadata(dir) {
            Ok(meta)
                if meta.is_dir()
                    && meta.mode() & 0o1022 == 0o1000
                    && meta.uid() == rustix::process::geteuid().as_raw() =>
            {
                sync_setup_dir(path, parent(dir), hook)?;
            }
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(ports::io_err(err)),
        }
    }
    Ok(())
}

fn sync_setup_dir(root: &Path, dir: &Path, hook: Option<&BeforeSync>) -> Result<(), StoreError> {
    if let Some(hook) = hook {
        hook(dir).map_err(|err| sync_error(root, dir, err))?;
    }
    fs::File::open(dir)
        .and_then(|fd| fd.sync_all())
        .map_err(|err| sync_error(root, dir, err))
}

fn parent(name: &Path) -> &Path {
    name.parent().expect("confined file name has a parent")
}

fn sync_error(root: &Path, dir: &Path, err: io::Error) -> StoreError {
    match ports::io_err(err) {
        StoreError::Io { detail } => match dir.strip_prefix(root) {
            Ok(relative) => StoreError::io(format_args!("sync directory {relative:?}: {detail:?}")),
            Err(_) => StoreError::io(format_args!("sync store ancestor: {detail:?}")),
        },
        other => other,
    }
}
