// SPDX-License-Identifier: Apache-2.0
//! Filesystem plugin root and the product network stub.
//! Implements PLUGINS.md §5.4 locations: owner-only directories, atomic registry replace.
//! Linux pins traversal with directory descriptors: children are `/proc/self/fd/<fd>/<child>`,
//! so a later rename or symlink swap of the path does not retarget an open handle.
//! macOS and other non-Linux Unix targets cannot walk `/dev/fd/<fd>/<child>` (ENOTDIR).
//! Those targets use the canonical root, `symlink_metadata` on every component below it,
//! rejection of `..` and absolute components, and a parent re-check immediately before
//! create and rename. Locks are create-exclusive. Publication is write, sync, rename, and
//! directory sync. A gap remains between each check and the following system call: same-user
//! code can substitute a symlink in that gap and redirect the create or rename. That is
//! weaker than Linux descriptor anchoring; this module does not claim equal strength.
//! Reads are bounded before allocation.
//! Depends on std and the distribution store port; used by the CLI entry.
//! The product binary has no TLS stack, so refresh and URL installs refuse with `Unreachable`.
//! Windows permits bounded read-only registry access; mutations refuse because replacement,
//! directory sync and ACL semantics for the plugin installation store are not qualified.

#![forbid(unsafe_code)]

#[cfg(unix)]
use std::fs::{self, File, OpenOptions};
use std::io;
#[cfg(unix)]
use std::io::Read;
#[cfg(target_os = "linux")]
use std::io::Write;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
#[cfg(target_os = "linux")]
use std::sync::MutexGuard;
use std::time::{SystemTime, UNIX_EPOCH};

use dctx_plugin_distribution::{
    Clock, DistributionError, HttpsUrl, InstallKey, Network, NetworkError, Response, Target,
};
#[cfg(any(target_os = "linux", not(unix)))]
use dctx_plugin_distribution::{LockToken, SafeFileName, Store};

/// Clock from the system wall time. Failure to read it yields zero, which freshness treats as stale.
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

/// Network port that never connects. TLS is a separate reviewed adapter.
#[derive(Debug, Default)]
pub struct NoTls;

impl Network for NoTls {
    fn get(&self, _url: &HttpsUrl) -> Result<Response, NetworkError> {
        Err(NetworkError::Unreachable)
    }
}

/// Compile-time host target; libc selection never probes the running system.
pub fn host_target() -> Result<Target, super::UnsupportedTarget> {
    let env = if cfg!(target_env = "gnu") {
        "gnu"
    } else if cfg!(target_env = "musl") {
        "musl"
    } else if cfg!(target_env = "msvc") {
        "msvc"
    } else if cfg!(target_env = "uclibc") {
        "uclibc"
    } else if cfg!(target_env = "") {
        ""
    } else {
        "unknown"
    };
    target_for(std::env::consts::OS, std::env::consts::ARCH, env)
}

/// Closed artifact mapping, independent of the machine running the command.
pub(crate) fn target_for(
    os: &str,
    arch: &str,
    env: &str,
) -> Result<Target, super::UnsupportedTarget> {
    use super::UnsupportedTarget::{Cpu, Libc, Os};
    let platform = || format!("{os}-{arch}-{env}");
    if !matches!(os, "macos" | "linux" | "windows") {
        return Err(Os(platform()));
    }
    if !matches!(
        (os, arch),
        ("macos" | "linux", "aarch64" | "x86_64") | ("windows", "x86_64")
    ) {
        return Err(Cpu(platform()));
    }
    match (os, arch, env) {
        ("macos", "aarch64", "") => Ok(Target::AppleArm64),
        ("macos", "x86_64", "") => Ok(Target::AppleX64),
        ("linux", "x86_64", "gnu") => Ok(Target::LinuxX64),
        ("linux", "aarch64", "gnu") => Ok(Target::LinuxArm64),
        ("windows", "x86_64", "msvc") => Ok(Target::WindowsX64),
        _ => Err(Libc(platform())),
    }
}

/// `DCTX_PLUGIN_ROOT`, or the platform config directory. Does not create it.
///
/// Refuses when no absolute home or config directory is known, instead of falling back to a
/// root relative to the current directory (which may be an untrusted checkout).
pub fn default_root() -> Result<PathBuf, String> {
    root_from(|name| std::env::var_os(name), std::env::consts::OS)
}

/// [`default_root`] over an injected environment, for tests.
pub(crate) fn root_from(
    var: impl Fn(&str) -> Option<std::ffi::OsString>,
    os: &str,
) -> Result<PathBuf, String> {
    let non_empty = |name: &str| var(name).filter(|value| !value.is_empty());
    let absolute = |name: &str| {
        non_empty(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    if let Some(root) = non_empty("DCTX_PLUGIN_ROOT") {
        return Ok(PathBuf::from(root));
    }
    let home = absolute("HOME");
    let missing = || {
        "no plugin root: HOME (APPDATA on Windows) is unset or not absolute, and DCTX_PLUGIN_ROOT is not set"
            .to_owned()
    };
    match os {
        "macos" => Ok(home
            .ok_or_else(missing)?
            .join("Library/Application Support/dctx/plugin-config")),
        "windows" => Ok(absolute("APPDATA")
            .or(home)
            .ok_or_else(missing)?
            .join("dctx")
            .join("Plugins")),
        _ => {
            let config = match absolute("XDG_CONFIG_HOME") {
                Some(config) => config,
                None => home.ok_or_else(missing)?.join(".config"),
            };
            Ok(config.join("dctx").join("plugins"))
        }
    }
}

#[cfg(unix)]
const REGISTRY_LIMIT: u64 = 16 << 20; // Registry codec ceiling.
#[cfg(unix)]
const FILE_LIMIT: u64 = dctx_plugin_distribution::DEFAULT_MAX_ARTIFACT_BYTES;
#[cfg(unix)]
static NEXT: AtomicU64 = AtomicU64::new(1);
#[cfg(all(unix, not(target_os = "linux")))]
fn next_token() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}
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
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            held: Mutex::new(None),
        }
    }
}

#[cfg(target_os = "linux")]
impl FsStore {
    fn root(&self, create: bool) -> io::Result<File> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let absolute = if self.root.is_absolute() {
                self.root.clone()
            } else {
                std::env::current_dir()?.join(&self.root)
            };
            let root = descend(File::open("/")?, &absolute, create)?;
            if create {
                root.set_permissions(fs::Permissions::from_mode(0o700))?;
            }
            Ok(root)
        }
        #[cfg(not(unix))]
        {
            let _ = create;
            Err(io::Error::other("native store semantics are not qualified"))
        }
    }

    #[allow(
        clippy::unused_self,
        reason = "Keep the store directory helper on FsStore alongside its retained-root and lock operations."
    )]
    fn key_dir(&self, root: &File, kind: &str, key: &InstallKey, create: bool) -> io::Result<File> {
        descend(
            root.try_clone()?,
            &PathBuf::from(kind).join(key.components().iter().collect::<PathBuf>()),
            create,
        )
    }

    fn check(
        &self,
        token: LockToken,
        root: &File,
    ) -> Result<MutexGuard<'_, Ownership>, DistributionError> {
        let held = self
            .held
            .lock()
            .map_err(|_| DistributionError::Interrupted)?;
        let owner = held
            .as_ref()
            .filter(|(value, _)| *value == token.value())
            .ok_or(DistributionError::Busy)?;
        if bounded_read(&entry(root, "registry.lock")?, 128)? != owner.1.as_bytes() {
            return Err(DistributionError::Busy);
        }
        Ok(held)
    }
}

#[cfg(target_os = "linux")]
impl Store for FsStore {
    fn try_lock(&self) -> Result<LockToken, DistributionError> {
        let mut held = self
            .held
            .lock()
            .map_err(|_| DistributionError::Interrupted)?;
        if held.is_some() {
            return Err(DistributionError::Busy);
        }
        let root = disk(self.root(true))?;
        let token = NEXT.fetch_add(1, Ordering::Relaxed);
        let owner = format!(
            "v1 {} {} {}\n",
            std::process::id(),
            SystemClock.now(),
            token
        );
        let mut file = private_options()
            .create_new(true)
            .open(entry(&root, "registry.lock")?)
            .map_err(|_| DistributionError::Busy)?;
        disk(file.write_all(owner.as_bytes()))?;
        disk(file.sync_all())?;
        disk(root.sync_all())?;
        *held = Some((token, owner));
        Ok(LockToken::new(token))
    }

    fn unlock(&self, token: LockToken) {
        let Ok(root) = self.root(false) else { return };
        let Ok(mut held) = self.check(token, &root) else {
            return;
        };
        if let Ok(path) = entry(&root, "registry.lock") {
            if fs::remove_file(path).and_then(|()| root.sync_all()).is_ok() {
                *held = None;
            }
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
            if e.kind() == io::ErrorKind::NotFound {
                return Ok(None);
            }
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

    fn stage_append(
        &self,
        token: LockToken,
        key: &InstallKey,
        file: &SafeFileName,
        bytes: &[u8],
    ) -> Result<(), DistributionError> {
        let root = disk(self.root(false))?;
        let _held = self.check(token, &root)?;
        let directory = disk(self.key_dir(&root, "staging", key, true))?;
        let path = entry(&directory, file.as_str())?;
        let mut handle = match fs::symlink_metadata(&path) {
            Ok(meta) => {
                if !meta.is_file() {
                    return Err(DistributionError::Interrupted);
                }
                let handle = disk(private_options().append(true).open(&path))?;
                same_file(&meta, &disk(handle.metadata())?)?;
                handle
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                disk(private_options().create_new(true).append(true).open(path))?
            }
            Err(_) => return Err(DistributionError::Interrupted),
        };
        limit(
            disk(handle.metadata())?
                .len()
                .saturating_add(bytes.len() as u64),
            FILE_LIMIT,
        )?;
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
            if !disk(item.file_type())?.is_file() {
                return Err(DistributionError::Interrupted);
            }
            disk(File::open(item.path()).and_then(|file| file.sync_all()))?;
        }
        disk(staging.sync_all())?;
        #[cfg(test)]
        store_tests::crash_cut("install-synced");
        let parts = key.components();
        let suffix = parts[..3].iter().collect::<PathBuf>();
        let destination = disk(descend(
            disk(root.try_clone())?,
            &PathBuf::from("installed").join(&suffix),
            true,
        ))?;
        let target = entry(&destination, &parts[3])?;
        if fs::symlink_metadata(&target).is_ok() {
            return Err(DistributionError::Interrupted);
        }
        let source = disk(descend(root, &PathBuf::from("staging").join(suffix), false))?;
        disk(fs::rename(entry(&source, &parts[3])?, target))?;
        #[cfg(test)]
        store_tests::crash_cut("install-renamed");
        disk(destination.sync_all())?;
        disk(source.sync_all())
    }

    fn discard_staging(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> {
        self.remove(token, "staging", key)
    }
    fn staged(&self) -> Result<Vec<InstallKey>, DistributionError> {
        self.keys("staging")
    }
    fn installed(&self) -> Result<Vec<InstallKey>, DistributionError> {
        self.keys("installed")
    }

    fn read_installed(
        &self,
        key: &InstallKey,
        file: &SafeFileName,
    ) -> Result<Vec<u8>, DistributionError> {
        let root = disk(self.root(false))?;
        let directory = self
            .key_dir(&root, "installed", key, false)
            .map_err(|_| DistributionError::NotInstalled)?;
        bounded_read(&entry(&directory, file.as_str())?, FILE_LIMIT)
    }

    fn remove_installed(
        &self,
        token: LockToken,
        key: &InstallKey,
    ) -> Result<(), DistributionError> {
        self.remove(token, "installed", key)
    }
}

fn disk<T>(result: io::Result<T>) -> Result<T, DistributionError> {
    result.map_err(|_| DistributionError::Interrupted)
}
#[cfg(unix)]
fn limit(size: u64, ceiling: u64) -> Result<(), DistributionError> {
    if size > ceiling {
        Err(DistributionError::LimitExceeded { limit: ceiling })
    } else {
        Ok(())
    }
}
#[cfg(unix)]
fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}
#[cfg(target_os = "linux")]
#[allow(
    clippy::unnecessary_wraps,
    reason = "Retain the shared platform entry interface: unsupported platforms refuse, while Linux constructs its descriptor-relative path infallibly."
)]
fn entry(directory: &File, name: impl AsRef<Path>) -> Result<PathBuf, DistributionError> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let prefix = if cfg!(target_os = "linux") {
            "/proc/self/fd"
        } else {
            "/dev/fd"
        };
        Ok(PathBuf::from(prefix)
            .join(directory.as_raw_fd().to_string())
            .join(name))
    }
    #[cfg(not(unix))]
    {
        let _ = (directory, name);
        Err(DistributionError::Interrupted)
    }
}
#[cfg(unix)]
fn same_file(before: &fs::Metadata, after: &fs::Metadata) -> Result<(), DistributionError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.is_file() && before.nlink() != 1 {
            return Err(DistributionError::Interrupted);
        }
        if before.dev() == after.dev() && before.ino() == after.ino() {
            return Ok(());
        }
    }
    #[cfg(not(unix))]
    let _ = (before, after);
    Err(DistributionError::Interrupted)
}
// Open each component through a retained directory descriptor; compare identities before use.
#[cfg(target_os = "linux")]
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
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&child) {
                Ok(()) => directory.sync_all()?,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e),
            }
        }
        let meta = fs::symlink_metadata(&child)?;
        if !meta.is_dir() {
            return Err(io::Error::other("symlink or non-directory"));
        }
        let opened = File::open(child)?;
        same_file(&meta, &opened.metadata()?).map_err(|_| io::Error::other("directory changed"))?;
        directory = opened;
    }
    Ok(directory)
}
#[cfg(unix)]
fn bounded_read(path: &Path, ceiling: u64) -> Result<Vec<u8>, DistributionError> {
    let meta = disk(fs::symlink_metadata(path))?;
    if !meta.is_file() {
        return Err(DistributionError::Interrupted);
    }
    limit(meta.len(), ceiling)?;
    let mut file = disk(File::open(path))?;
    let opened = disk(file.metadata())?;
    same_file(&meta, &opened)?;
    limit(opened.len(), ceiling)?;
    let length = usize::try_from(opened.len())
        .map_err(|_| DistributionError::LimitExceeded { limit: ceiling })?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| DistributionError::Interrupted)?;
    bytes.resize(length, 0);
    disk(file.read_exact(&mut bytes))?;
    let mut extra = [0];
    if disk(file.read(&mut extra))? != 0 {
        return Err(DistributionError::LimitExceeded { limit: ceiling });
    }
    Ok(bytes)
}
#[cfg(target_os = "linux")]
impl FsStore {
    fn remove(
        &self,
        token: LockToken,
        kind: &str,
        key: &InstallKey,
    ) -> Result<(), DistributionError> {
        let root = disk(self.root(false))?;
        let _held = self.check(token, &root)?;
        let parts = key.components();
        let parent = match descend(
            root,
            &PathBuf::from(kind).join(parts[..3].iter().collect::<PathBuf>()),
            false,
        ) {
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
        let root = match self
            .root(false)
            .and_then(|root| descend(root, Path::new(kind), false))
        {
            Ok(root) => root,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err(DistributionError::Interrupted),
        };
        let mut keys = Vec::new();
        walk_keys(&root, &mut Vec::new(), &mut keys, &mut 0)?;
        Ok(keys)
    }
}
#[cfg(target_os = "linux")]
fn walk_keys(
    root: &File,
    parts: &mut Vec<String>,
    keys: &mut Vec<InstallKey>,
    visited: &mut u64,
) -> Result<(), DistributionError> {
    for item in disk(fs::read_dir(entry(root, "")?))? {
        *visited += 1;
        limit(*visited, 4096)?;
        let item = disk(item)?;
        let name = item.file_name();
        let Some(name) = name.to_str() else { continue };
        let child = disk(descend(disk(root.try_clone())?, Path::new(name), false))?;
        parts.push(name.to_owned());
        if parts.len() < 4 {
            walk_keys(&child, parts, keys, visited)?;
        } else if let Ok(key) =
            install_key(&format!("{}/{}", parts[0], parts[1]), &parts[2], &parts[3])
        {
            keys.push(key);
        }
        parts.pop();
    }
    Ok(())
}
#[cfg(unix)]
fn install_key(id: &str, version: &str, digest: &str) -> Result<InstallKey, DistributionError> {
    use dctx_plugin_distribution::{Document, PluginId, Sha256Hex, Version};
    Ok(InstallKey {
        id: PluginId::parse(id)?,
        version: Version::parse(version)?,
        digest: Sha256Hex::parse(digest, Document::Registry)?,
    })
}

#[cfg(not(unix))]
impl FsStore {
    /// Typed refusal until Windows replacement, directory sync and ACLs are qualified.
    fn unsupported<T>(&self) -> io::Result<T> {
        let _ = (&self.root, &self.held);
        Err(io::Error::other("unsupported on this platform"))
    }
}

#[cfg(not(unix))]
impl Store for FsStore {
    fn try_lock(&self) -> Result<LockToken, DistributionError> {
        disk(self.unsupported())
    }
    fn unlock(&self, _token: LockToken) {
        let _ = self.unsupported::<()>();
    }
    fn load_registry(&self) -> Result<Option<Vec<u8>>, DistributionError> {
        #[cfg(windows)]
        return disk_readonly::load_registry(&self.root);
        #[cfg(not(windows))]
        disk(self.unsupported())
    }
    fn save_registry(&self, _token: LockToken, _bytes: &[u8]) -> Result<(), DistributionError> {
        disk(self.unsupported())
    }
    fn stage_append(
        &self,
        _token: LockToken,
        _key: &InstallKey,
        _file: &SafeFileName,
        _bytes: &[u8],
    ) -> Result<(), DistributionError> {
        disk(self.unsupported())
    }
    fn publish(&self, _token: LockToken, _key: &InstallKey) -> Result<(), DistributionError> {
        disk(self.unsupported())
    }
    fn discard_staging(
        &self,
        _token: LockToken,
        _key: &InstallKey,
    ) -> Result<(), DistributionError> {
        disk(self.unsupported())
    }
    fn staged(&self) -> Result<Vec<InstallKey>, DistributionError> {
        disk(self.unsupported())
    }
    fn installed(&self) -> Result<Vec<InstallKey>, DistributionError> {
        disk(self.unsupported())
    }
    fn read_installed(
        &self,
        _key: &InstallKey,
        _file: &SafeFileName,
    ) -> Result<Vec<u8>, DistributionError> {
        disk(self.unsupported())
    }
    fn remove_installed(
        &self,
        _token: LockToken,
        _key: &InstallKey,
    ) -> Result<(), DistributionError> {
        disk(self.unsupported())
    }
}

#[cfg(windows)]
mod disk_readonly;

#[cfg(all(unix, not(target_os = "linux")))]
#[path = "disk_portable.rs"]
mod disk_portable;

#[cfg(test)]
#[path = "store_tests.rs"]
mod store_tests;

#[cfg(all(test, unix, not(target_os = "linux")))]
fn crash_cut(phase: &str) {
    store_tests::crash_cut(phase);
}

#[cfg(test)]
mod root_tests {
    use super::root_from;
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let pairs: Vec<(String, OsString)> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn missing_or_relative_home_refuses_instead_of_using_the_working_directory() {
        for os in ["linux", "macos", "windows"] {
            for vars in [&[][..], &[("HOME", "")], &[("HOME", "relative/home")]] {
                let error = root_from(env(vars), os).unwrap_err();
                assert!(error.contains("HOME"), "{os}: {error}");
            }
        }
        let error = root_from(env(&[("XDG_CONFIG_HOME", "rel")]), "linux").unwrap_err();
        assert!(error.contains("HOME"), "{error}");
    }

    #[test]
    fn absolute_locations_and_the_explicit_override_resolve() {
        let home = if cfg!(windows) {
            "C:\\home\\u"
        } else {
            "/home/u"
        };
        let root = root_from(env(&[("HOME", home)]), "linux").unwrap();
        assert_eq!(
            root,
            PathBuf::from(home)
                .join(".config")
                .join("dctx")
                .join("plugins")
        );
        let root = root_from(env(&[("HOME", home), ("XDG_CONFIG_HOME", "rel")]), "linux").unwrap();
        assert!(root.starts_with(home));
        let root = root_from(env(&[("HOME", home)]), "macos").unwrap();
        assert!(root.starts_with(home));
        let root = root_from(env(&[("APPDATA", home)]), "windows").unwrap();
        assert_eq!(root, PathBuf::from(home).join("dctx").join("Plugins"));
        let root = root_from(env(&[("DCTX_PLUGIN_ROOT", "chosen")]), "linux").unwrap();
        assert_eq!(root, PathBuf::from("chosen"));
    }
}
