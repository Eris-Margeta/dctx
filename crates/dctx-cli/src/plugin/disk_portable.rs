//! Canonical-path store for Unix targets other than Linux.
//! `symlink_metadata` checks are not descriptor anchoring. Same-user code can still
//! replace a directory with a symlink in the gap before the next create, open, or rename.
#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::MutexGuard;

use dctx_plugin_distribution::{Clock, DistributionError, InstallKey, LockToken, SafeFileName, Store};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

use super::FsStore;

fn bad_path() -> io::Error { io::Error::other("non-normal store path") }
fn not_dir() -> io::Error { io::Error::other("symlink or non-directory") }

fn is_real_dir(meta: &fs::Metadata) -> bool { meta.is_dir() && !meta.file_type().is_symlink() }
fn is_real_file(meta: &fs::Metadata) -> bool { meta.is_file() && !meta.file_type().is_symlink() }

fn same_inode(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    before.dev() == after.dev() && before.ino() == after.ino()
}

fn sync_dir(path: &Path) -> io::Result<()> {
    // macOS rejects F_FULLFSYNC on some directories; fsync is the directory sync, with a fallback.
    let file = File::open(path)?;
    match file.sync_data() {
        Ok(()) => Ok(()),
        Err(_) => file.sync_all(),
    }
}

fn create_private(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder.create(path)
}

fn normal_component(name: &str) -> io::Result<&str> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(name),
        _ => Err(bad_path()),
    }
}

fn key_path(kind: &str, parts: &[String]) -> PathBuf {
    let mut path = PathBuf::from(kind);
    for part in parts { path.push(part); }
    path
}

/// Lexical walk: reject `..` and absolute components, and reject symlink components.
fn walk_real(path: &Path, create: bool) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir()?.join(path) };
    let mut current = PathBuf::new();
    let mut saw = false;
    for part in absolute.components() {
        match part {
            Component::RootDir => {
                current.push(part);
                continue;
            }
            Component::CurDir => continue,
            Component::Normal(name) => current.push(name),
            _ => return Err(bad_path()),
        }
        saw = true;
        match fs::symlink_metadata(&current) {
            Ok(meta) if is_real_dir(&meta) => {}
            Ok(_) => return Err(not_dir()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if !create { return Err(error); }
                match create_private(&current) {
                    Ok(()) => {
                        if let Some(parent) = current.parent() { sync_dir(parent)?; }
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                let meta = fs::symlink_metadata(&current)?;
                if !is_real_dir(&meta) { return Err(not_dir()); }
            }
            Err(error) => return Err(error),
        }
    }
    if !saw { return Err(bad_path()); }
    Ok(current)
}

fn canonicalize_same(path: &Path) -> io::Result<PathBuf> {
    let before = fs::symlink_metadata(path)?;
    if !is_real_dir(&before) { return Err(not_dir()); }
    let canon = fs::canonicalize(path)?;
    let after = fs::symlink_metadata(&canon)?;
    if !is_real_dir(&after) || !same_inode(&before, &after) { return Err(io::Error::other("root path changed")); }
    Ok(canon)
}

fn verify_real_chain(path: &Path) -> io::Result<()> {
    let mut current = PathBuf::new();
    let mut saw = false;
    for part in path.components() {
        match part {
            Component::RootDir => {
                current.push(part);
                continue;
            }
            Component::CurDir => continue,
            Component::Normal(name) => current.push(name),
            _ => return Err(bad_path()),
        }
        saw = true;
        if !is_real_dir(&fs::symlink_metadata(&current)?) { return Err(not_dir()); }
    }
    if !saw { Err(bad_path()) } else { Ok(()) }
}

fn open_root(path: &Path, create: bool) -> io::Result<PathBuf> {
    let walked = walk_real(path, create)?;
    let canon = canonicalize_same(&walked)?;
    verify_real_chain(&canon)?;
    if create {
        if !is_real_dir(&fs::symlink_metadata(&canon)?) { return Err(not_dir()); }
        fs::set_permissions(&canon, fs::Permissions::from_mode(0o700))?;
    }
    Ok(canon)
}

/// Re-check that `path` is a real directory inside `anchor` immediately before create or rename.
fn confine(anchor: &Path, path: &Path) -> io::Result<PathBuf> {
    let canon = canonicalize_same(path)?;
    let anchor = canonicalize_same(anchor)?;
    if canon != anchor && !canon.starts_with(&anchor) { return Err(io::Error::other("escaped root")); }
    let relative = canon.strip_prefix(&anchor).map_err(|_| io::Error::other("escaped root"))?;
    let mut cursor = anchor;
    for part in relative.components() {
        let Component::Normal(name) = part else { return Err(bad_path()); };
        cursor.push(name);
        if !is_real_dir(&fs::symlink_metadata(&cursor)?) { return Err(not_dir()); }
    }
    Ok(cursor)
}

fn descend(anchor: &Path, relative: &Path, create: bool) -> io::Result<PathBuf> {
    let mut current = confine(anchor, anchor)?;
    for part in relative.components() {
        let name = match part {
            Component::Normal(name) => name,
            Component::CurDir => continue,
            _ => return Err(bad_path()),
        };
        let child = current.join(name);
        match fs::symlink_metadata(&child) {
            Ok(meta) if is_real_dir(&meta) => current = confine(anchor, &child)?,
            Ok(_) => return Err(not_dir()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if !create { return Err(error); }
                let parent = confine(anchor, &current)?;
                match create_private(&parent.join(name)) {
                    Ok(()) => sync_dir(&parent)?,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                current = confine(anchor, &parent.join(name))?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(current)
}

impl FsStore {
    fn check(&self, token: LockToken, root: &Path) -> Result<MutexGuard<'_, super::Ownership>, DistributionError> {
        let held = self.held.lock().map_err(|_| DistributionError::Interrupted)?;
        let owner = held.as_ref().filter(|(value, _)| *value == token.value()).ok_or(DistributionError::Busy)?;
        let root = super::disk(confine(root, root))?;
        if super::bounded_read(&root.join("registry.lock"), 128)? != owner.1.as_bytes() { return Err(DistributionError::Busy); }
        Ok(held)
    }

    fn open(&self, create: bool) -> io::Result<PathBuf> { open_root(&self.root, create) }

    fn remove_key(&self, token: LockToken, kind: &str, key: &InstallKey) -> Result<(), DistributionError> {
        let root = super::disk(self.open(false))?;
        let _held = self.check(token, &root)?;
        let parts = key.components();
        let parent = match descend(&root, &key_path(kind, &parts[..3]), false) {
            Ok(parent) => parent,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(DistributionError::Interrupted),
        };
        let name = super::disk(normal_component(&parts[3]))?;
        let parent = super::disk(confine(&root, &parent))?;
        let path = parent.join(name);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Ok(meta) if is_real_dir(&meta) => {}
            _ => return Err(DistributionError::Interrupted),
        }
        super::disk(fs::remove_dir_all(&path))?;
        super::disk(sync_dir(&parent))
    }

    fn keys(&self, kind: &str) -> Result<Vec<InstallKey>, DistributionError> {
        let root = match self.open(false) {
            Ok(root) => root,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err(DistributionError::Interrupted),
        };
        let directory = match descend(&root, Path::new(kind), false) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err(DistributionError::Interrupted),
        };
        let mut keys = Vec::new();
        walk_keys(&root, &directory, &mut Vec::new(), &mut keys, &mut 0)?;
        Ok(keys)
    }
}

fn walk_keys(
    anchor: &Path, dir: &Path, parts: &mut Vec<String>, keys: &mut Vec<InstallKey>, visited: &mut u64,
) -> Result<(), DistributionError> {
    let dir = super::disk(confine(anchor, dir))?;
    for item in super::disk(fs::read_dir(&dir))? {
        *visited += 1;
        super::limit(*visited, 4096)?;
        let item = super::disk(item)?;
        let name = item.file_name();
        let Some(name) = name.to_str() else { continue };
        if normal_component(name).is_err() { return Err(DistributionError::Interrupted); }
        let child = super::disk(confine(anchor, &dir.join(name)))?;
        parts.push(name.to_owned());
        if parts.len() < 4 { walk_keys(anchor, &child, parts, keys, visited)?; }
        else if let Ok(key) = super::install_key(&format!("{}/{}", parts[0], parts[1]), &parts[2], &parts[3]) { keys.push(key); }
        parts.pop();
    }
    Ok(())
}

impl Store for FsStore {
    fn try_lock(&self) -> Result<LockToken, DistributionError> {
        let mut held = self.held.lock().map_err(|_| DistributionError::Interrupted)?;
        if held.is_some() { return Err(DistributionError::Busy); }
        let root = super::disk(self.open(true))?;
        let token = super::next_token();
        let owner = format!("v1 {} {} {}\n", std::process::id(), Clock::now(&super::SystemClock), token);
        let parent = super::disk(confine(&root, &root))?;
        let mut file = super::private_options().create_new(true).open(parent.join("registry.lock")).map_err(|_| DistributionError::Busy)?;
        super::disk(file.write_all(owner.as_bytes()))?;
        super::disk(file.sync_all())?;
        super::disk(sync_dir(&parent))?;
        *held = Some((token, owner));
        Ok(LockToken::new(token))
    }

    fn unlock(&self, token: LockToken) {
        let Ok(root) = self.open(false) else { return };
        let Ok(mut held) = self.check(token, &root) else { return };
        let Ok(root) = confine(&root, &root) else { return };
        if fs::remove_file(root.join("registry.lock")).and_then(|()| sync_dir(&root)).is_ok() { *held = None; }
    }

    fn load_registry(&self) -> Result<Option<Vec<u8>>, DistributionError> {
        let root = match self.open(false) {
            Ok(root) => root,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(DistributionError::Interrupted),
        };
        let path = root.join("registry.json");
        if let Err(error) = fs::symlink_metadata(&path) {
            if error.kind() == io::ErrorKind::NotFound { return Ok(None); }
            return Err(DistributionError::Interrupted);
        }
        super::bounded_read(&path, super::REGISTRY_LIMIT).map(Some)
    }

    fn save_registry(&self, token: LockToken, bytes: &[u8]) -> Result<(), DistributionError> {
        super::limit(bytes.len() as u64, super::REGISTRY_LIMIT)?;
        let root = super::disk(self.open(false))?;
        let _held = self.check(token, &root)?;
        let parent = super::disk(confine(&root, &root))?;
        let mut file = super::disk(super::private_options().create_new(true).open(parent.join("registry.json.tmp")))?;
        super::disk(file.write_all(bytes))?;
        super::disk(file.sync_all())?;
        #[cfg(test)]
        super::crash_cut("registry-synced");
        let parent = super::disk(confine(&root, &root))?;
        super::disk(fs::rename(parent.join("registry.json.tmp"), parent.join("registry.json")))?;
        #[cfg(test)]
        super::crash_cut("registry-renamed");
        super::disk(sync_dir(&parent))
    }

    fn stage_append(&self, token: LockToken, key: &InstallKey, file: &SafeFileName, bytes: &[u8]) -> Result<(), DistributionError> {
        let root = super::disk(self.open(false))?;
        let _held = self.check(token, &root)?;
        let directory = super::disk(descend(&root, &key_path("staging", &key.components()), true))?;
        let name = super::disk(normal_component(file.as_str()))?;
        let path = directory.join(name);
        let mut handle = match fs::symlink_metadata(&path) {
            Ok(meta) => {
                if !is_real_file(&meta) { return Err(DistributionError::Interrupted); }
                let parent = super::disk(confine(&root, &directory))?;
                let handle = super::disk(super::private_options().append(true).open(parent.join(name)))?;
                super::same_file(&meta, &super::disk(handle.metadata())?)?;
                handle
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let parent = super::disk(confine(&root, &directory))?;
                super::disk(super::private_options().create_new(true).append(true).open(parent.join(name)))?
            }
            Err(_) => return Err(DistributionError::Interrupted),
        };
        super::limit(super::disk(handle.metadata())?.len().saturating_add(bytes.len() as u64), super::FILE_LIMIT)?;
        super::disk(handle.write_all(bytes))?;
        super::disk(handle.sync_all())?;
        let parent = super::disk(confine(&root, &directory))?;
        super::disk(sync_dir(&parent))
    }

    fn publish(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> {
        let root = super::disk(self.open(false))?;
        let _held = self.check(token, &root)?;
        let parts = key.components();
        let staging = super::disk(descend(&root, &key_path("staging", &parts), false))?;
        let staging = super::disk(confine(&root, &staging))?;
        for item in super::disk(fs::read_dir(&staging))? {
            let item = super::disk(item)?;
            let path = item.path();
            let meta = super::disk(fs::symlink_metadata(&path))?;
            if !is_real_file(&meta) { return Err(DistributionError::Interrupted); }
            super::disk(File::open(&path).and_then(|file| file.sync_all()))?;
        }
        super::disk(sync_dir(&staging))?;
        #[cfg(test)]
        super::crash_cut("install-synced");
        let destination = super::disk(descend(&root, &key_path("installed", &parts[..3]), true))?;
        let name = super::disk(normal_component(&parts[3]))?;
        let source_parent = super::disk(descend(&root, &key_path("staging", &parts[..3]), false))?;
        let destination = super::disk(confine(&root, &destination))?;
        let source_parent = super::disk(confine(&root, &source_parent))?;
        let target = destination.join(name);
        if fs::symlink_metadata(&target).is_ok() { return Err(DistributionError::Interrupted); }
        super::disk(fs::rename(source_parent.join(name), target))?;
        #[cfg(test)]
        super::crash_cut("install-renamed");
        super::disk(sync_dir(&destination))?;
        super::disk(sync_dir(&source_parent))
    }

    fn discard_staging(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> { self.remove_key(token, "staging", key) }
    fn staged(&self) -> Result<Vec<InstallKey>, DistributionError> { self.keys("staging") }
    fn installed(&self) -> Result<Vec<InstallKey>, DistributionError> { self.keys("installed") }

    fn read_installed(&self, key: &InstallKey, file: &SafeFileName) -> Result<Vec<u8>, DistributionError> {
        let root = super::disk(self.open(false))?;
        let parts = key.components();
        let directory = descend(&root, &key_path("installed", &parts), false).map_err(|_| DistributionError::NotInstalled)?;
        let name = normal_component(file.as_str()).map_err(|_| DistributionError::Interrupted)?;
        super::bounded_read(&directory.join(name), super::FILE_LIMIT)
    }

    fn remove_installed(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> { self.remove_key(token, "installed", key) }
}
