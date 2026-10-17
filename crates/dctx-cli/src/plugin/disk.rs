//! Filesystem plugin root and the product network stub.
//! Implements PLUGINS.md §5.4 locations: owner-only directories, atomic registry replace,
//! Directory descriptors pin root traversal; reads are bounded before allocation.
//! Depends on std and the distribution store port; used by the CLI entry.
//! The product binary has no TLS stack, so refresh and URL installs refuse with `Unreachable`.

#![forbid(unsafe_code)]

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use dctx_plugin_distribution::{
    Clock, DistributionError, HttpsUrl, InstallKey, LockToken, Network, NetworkError, Response, SafeFileName, Store,
    Target,
};

/// Clock from the system wall time. Failure to read it yields zero, which freshness treats as stale.
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }
}

/// Network port that never connects. TLS is a separate reviewed adapter.
#[derive(Debug, Default)]
pub struct NoTls;

impl Network for NoTls {
    fn get(&self, _url: &HttpsUrl) -> Result<Response, NetworkError> { Err(NetworkError::Unreachable) }
}

/// Compile-time host target. Unknown pairs still parse as a manifest target or Apple arm64.
pub fn host_target() -> Target {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Target::AppleArm64,
        ("macos", "x86_64") => Target::AppleX64,
        ("linux", "x86_64") => Target::LinuxX64,
        ("linux", "aarch64") => Target::LinuxArm64,
        ("windows", "x86_64") => Target::WindowsX64,
        _ => Target::AppleArm64,
    }
}

/// `DCTX_PLUGIN_ROOT`, or the platform config directory. Does not create it.
pub fn default_root() -> PathBuf {
    if let Some(root) = std::env::var_os("DCTX_PLUGIN_ROOT") { return PathBuf::from(root); }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    match std::env::consts::OS {
        "macos" => home.join("Library/Application Support/dctx/plugin-config"),
        "windows" => std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home).join("dctx").join("Plugins"),
        _ => {
            let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
            config.join("dctx").join("plugins")
        }
    }
}

const REGISTRY_LIMIT: u64 = 16 << 20; // Registry codec ceiling.
const FILE_LIMIT: u64 = dctx_plugin_distribution::DEFAULT_MAX_ARTIFACT_BYTES;
static NEXT: AtomicU64 = AtomicU64::new(1);
type Ownership = Option<(u64, String)>;

/// Managed private root. Parents must not be controlled by hostile same-user code.
/// OFFLINE takeover: stop ALL CLI/app clients, preserve registry.json, remove registry.lock
/// and registry.json.tmp, then restart. Never infer takeover safety from PID/age/liveness.
/// Crashes retain a synced owner record; Drop deliberately does not unlock.
/// Windows refuses until native replacement, directory sync and ACL semantics are qualified.
pub struct FsStore {
    root: PathBuf,
    held: Mutex<Ownership>,
}

impl FsStore {
    /// Binds `root` without creating it.
    pub fn new(root: impl Into<PathBuf>) -> Self { Self { root: root.into(), held: Mutex::new(None) } }

    fn root(&self, create: bool) -> io::Result<File> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let absolute = if self.root.is_absolute() { self.root.clone() } else { std::env::current_dir()?.join(&self.root) };
            let root = descend(File::open("/")?, &absolute, create)?;
            if create { root.set_permissions(fs::Permissions::from_mode(0o700))?; }
            Ok(root)
        }
        #[cfg(not(unix))]
        { let _ = create; Err(io::Error::other("native store semantics are not qualified")) }
    }

    fn key_dir(&self, root: &File, kind: &str, key: &InstallKey, create: bool) -> io::Result<File> {
        descend(root.try_clone()?, &PathBuf::from(kind).join(key.components().iter().collect::<PathBuf>()), create)
    }

    fn check(&self, token: LockToken, root: &File) -> Result<MutexGuard<'_, Ownership>, DistributionError> {
        let held = self.held.lock().map_err(|_| DistributionError::Interrupted)?;
        let owner = held.as_ref().filter(|(value, _)| *value == token.value()).ok_or(DistributionError::Busy)?;
        if bounded_read(&entry(root, "registry.lock")?, 128)? != owner.1.as_bytes() { return Err(DistributionError::Busy); }
        Ok(held)
    }
}

impl Store for FsStore {
    fn try_lock(&self) -> Result<LockToken, DistributionError> {
        let mut held = self.held.lock().map_err(|_| DistributionError::Interrupted)?;
        if held.is_some() { return Err(DistributionError::Busy); }
        let root = disk(self.root(true))?;
        let token = NEXT.fetch_add(1, Ordering::Relaxed);
        let owner = format!("v1 {} {} {}\n", std::process::id(), SystemClock.now(), token);
        let mut file = private_options().create_new(true).open(entry(&root, "registry.lock")?).map_err(|_| DistributionError::Busy)?;
        disk(file.write_all(owner.as_bytes()))?;
        disk(file.sync_all())?;
        disk(root.sync_all())?;
        *held = Some((token, owner));
        Ok(LockToken::new(token))
    }

    fn unlock(&self, token: LockToken) {
        let Ok(root) = self.root(false) else { return };
        let Ok(mut held) = self.check(token, &root) else { return };
        if let Ok(path) = entry(&root, "registry.lock") {
            if fs::remove_file(path).and_then(|()| root.sync_all()).is_ok() { *held = None; }
        }
    }

    fn load_registry(&self) -> Result<Option<Vec<u8>>, DistributionError> {
        let root = match self.root(false) {
            Ok(root) => root,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(DistributionError::Interrupted),
        };
        let path = entry(&root, "registry.json")?;
        if let Err(e) = fs::symlink_metadata(&path) {
            if e.kind() == io::ErrorKind::NotFound { return Ok(None); }
            return Err(DistributionError::Interrupted);
        }
        bounded_read(&path, REGISTRY_LIMIT).map(Some)
    }

    fn save_registry(&self, token: LockToken, bytes: &[u8]) -> Result<(), DistributionError> {
        limit(bytes.len() as u64, REGISTRY_LIMIT)?;
        let root = disk(self.root(false))?;
        let _held = self.check(token, &root)?;
        let temporary = entry(&root, "registry.json.tmp")?;
        let mut file = disk(private_options().create_new(true).open(&temporary))?;
        disk(file.write_all(bytes))?;
        disk(file.sync_all())?;
        #[cfg(test)]
        store_tests::crash_cut("registry-synced");
        disk(fs::rename(temporary, entry(&root, "registry.json")?))?;
        #[cfg(test)]
        store_tests::crash_cut("registry-renamed");
        disk(root.sync_all())
    }

    fn stage_append(&self, token: LockToken, key: &InstallKey, file: &SafeFileName, bytes: &[u8]) -> Result<(), DistributionError> {
        let root = disk(self.root(false))?;
        let _held = self.check(token, &root)?;
        let directory = disk(self.key_dir(&root, "staging", key, true))?;
        let path = entry(&directory, file.as_str())?;
        let mut handle = match fs::symlink_metadata(&path) {
            Ok(meta) => {
                if !meta.is_file() { return Err(DistributionError::Interrupted); }
                let handle = disk(private_options().append(true).open(&path))?;
                same_file(&meta, &disk(handle.metadata())?)?;
                handle
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => disk(private_options().create_new(true).append(true).open(path))?,
            Err(_) => return Err(DistributionError::Interrupted),
        };
        limit(disk(handle.metadata())?.len().saturating_add(bytes.len() as u64), FILE_LIMIT)?;
        disk(handle.write_all(bytes))?;
        disk(handle.sync_all())?;
        disk(directory.sync_all())
    }

    fn publish(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> {
        let root = disk(self.root(false))?;
        let _held = self.check(token, &root)?;
        let staging = disk(self.key_dir(&root, "staging", key, false))?;
        for item in disk(fs::read_dir(entry(&staging, "")?))? {
            let item = disk(item)?;
            if !disk(item.file_type())?.is_file() { return Err(DistributionError::Interrupted); }
            disk(File::open(item.path()).and_then(|file| file.sync_all()))?;
        }
        disk(staging.sync_all())?;
        #[cfg(test)]
        store_tests::crash_cut("install-synced");
        let parts = key.components();
        let suffix = parts[..3].iter().collect::<PathBuf>();
        let destination = disk(descend(disk(root.try_clone())?, &PathBuf::from("installed").join(&suffix), true))?;
        let target = entry(&destination, &parts[3])?;
        if fs::symlink_metadata(&target).is_ok() { return Err(DistributionError::Interrupted); }
        let source = disk(descend(root, &PathBuf::from("staging").join(suffix), false))?;
        disk(fs::rename(entry(&source, &parts[3])?, target))?;
        #[cfg(test)]
        store_tests::crash_cut("install-renamed");
        disk(destination.sync_all())?;
        disk(source.sync_all())
    }

    fn discard_staging(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> { self.remove(token, "staging", key) }
    fn staged(&self) -> Result<Vec<InstallKey>, DistributionError> { self.keys("staging") }
    fn installed(&self) -> Result<Vec<InstallKey>, DistributionError> { self.keys("installed") }

    fn read_installed(&self, key: &InstallKey, file: &SafeFileName) -> Result<Vec<u8>, DistributionError> {
        let root = disk(self.root(false))?;
        let directory = self.key_dir(&root, "installed", key, false).map_err(|_| DistributionError::NotInstalled)?;
        bounded_read(&entry(&directory, file.as_str())?, FILE_LIMIT)
    }

    fn remove_installed(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> { self.remove(token, "installed", key) }
}

fn disk<T>(result: io::Result<T>) -> Result<T, DistributionError> { result.map_err(|_| DistributionError::Interrupted) }
fn limit(size: u64, ceiling: u64) -> Result<(), DistributionError> {
    if size > ceiling { Err(DistributionError::LimitExceeded { limit: ceiling }) } else { Ok(()) }
}
fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    options
}
fn entry(directory: &File, name: impl AsRef<Path>) -> Result<PathBuf, DistributionError> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let prefix = if cfg!(target_os = "linux") { "/proc/self/fd" } else { "/dev/fd" };
        Ok(PathBuf::from(prefix).join(directory.as_raw_fd().to_string()).join(name))
    }
    #[cfg(not(unix))]
    { let _ = (directory, name); Err(DistributionError::Interrupted) }
}
fn same_file(before: &fs::Metadata, after: &fs::Metadata) -> Result<(), DistributionError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.is_file() && before.nlink() != 1 { return Err(DistributionError::Interrupted); }
        if before.dev() == after.dev() && before.ino() == after.ino() { return Ok(()); }
    }
    #[cfg(not(unix))]
    let _ = (before, after);
    Err(DistributionError::Interrupted)
}
// Open each component through a retained directory descriptor; compare identities before use.
fn descend(mut directory: File, path: &Path, create: bool) -> io::Result<File> {
    for part in path.components() {
        let name = match part {
            std::path::Component::Normal(name) => name,
            std::path::Component::CurDir | std::path::Component::RootDir => continue,
            _ => return Err(io::Error::other("non-normal store path")),
        };
        let child = entry(&directory, name).map_err(|_| io::Error::other("directory anchor"))?;
        if create {
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }
            match builder.create(&child) {
                Ok(()) => directory.sync_all()?,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e),
            }
        }
        let meta = fs::symlink_metadata(&child)?;
        if !meta.is_dir() { return Err(io::Error::other("symlink or non-directory")); }
        let opened = File::open(child)?;
        same_file(&meta, &opened.metadata()?).map_err(|_| io::Error::other("directory changed"))?;
        directory = opened;
    }
    Ok(directory)
}
fn bounded_read(path: &Path, ceiling: u64) -> Result<Vec<u8>, DistributionError> {
    let meta = disk(fs::symlink_metadata(path))?;
    if !meta.is_file() { return Err(DistributionError::Interrupted); }
    limit(meta.len(), ceiling)?;
    let mut file = disk(File::open(path))?;
    let opened = disk(file.metadata())?;
    same_file(&meta, &opened)?;
    limit(opened.len(), ceiling)?;
    let length = usize::try_from(opened.len()).map_err(|_| DistributionError::LimitExceeded { limit: ceiling })?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| DistributionError::Interrupted)?;
    bytes.resize(length, 0);
    disk(file.read_exact(&mut bytes))?;
    let mut extra = [0];
    if disk(file.read(&mut extra))? != 0 { return Err(DistributionError::LimitExceeded { limit: ceiling }); }
    Ok(bytes)
}
impl FsStore {
    fn remove(&self, token: LockToken, kind: &str, key: &InstallKey) -> Result<(), DistributionError> {
        let root = disk(self.root(false))?;
        let _held = self.check(token, &root)?;
        let parts = key.components();
        let parent = match descend(root, &PathBuf::from(kind).join(parts[..3].iter().collect::<PathBuf>()), false) {
            Ok(parent) => parent,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(DistributionError::Interrupted),
        };
        let path = entry(&parent, &parts[3])?;
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Ok(meta) if meta.is_dir() => (),
            _ => return Err(DistributionError::Interrupted),
        }
        disk(fs::remove_dir_all(path))?;
        disk(parent.sync_all())
    }
    fn keys(&self, kind: &str) -> Result<Vec<InstallKey>, DistributionError> {
        let root = match self.root(false).and_then(|root| descend(root, Path::new(kind), false)) {
            Ok(root) => root,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err(DistributionError::Interrupted),
        };
        let mut keys = Vec::new();
        walk_keys(&root, &mut Vec::new(), &mut keys, &mut 0)?;
        Ok(keys)
    }
}
fn walk_keys(root: &File, parts: &mut Vec<String>, keys: &mut Vec<InstallKey>, visited: &mut u64) -> Result<(), DistributionError> {
    for item in disk(fs::read_dir(entry(root, "")?))? {
        *visited += 1;
        limit(*visited, 4096)?;
        let item = disk(item)?;
        let name = item.file_name();
        let Some(name) = name.to_str() else { continue };
        let child = disk(descend(disk(root.try_clone())?, Path::new(name), false))?;
        parts.push(name.to_owned());
        if parts.len() < 4 { walk_keys(&child, parts, keys, visited)?; }
        else if let Ok(key) = install_key(&format!("{}/{}", parts[0], parts[1]), &parts[2], &parts[3]) { keys.push(key); }
        parts.pop();
    }
    Ok(())
}
fn install_key(id: &str, version: &str, digest: &str) -> Result<InstallKey, DistributionError> {
    use dctx_plugin_distribution::{Document, PluginId, Sha256Hex, Version};
    Ok(InstallKey {
        id: PluginId::parse(id)?,
        version: Version::parse(version)?,
        digest: Sha256Hex::parse(digest, Document::Registry)?,
    })
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod store_tests;
