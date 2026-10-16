//! Filesystem plugin root and the product network stub.
//! Implements PLUGINS.md §5.4 locations: owner-only directories, atomic registry replace,
//! no project `.dctx` creation. Depends on the distribution store port. Used by the CLI entry.
//! The product binary has no TLS stack, so refresh and URL installs refuse with `Unreachable`.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use dctx_plugin_distribution::{
    Clock, DistributionError, HttpsUrl, InstallKey, LockToken, Network, NetworkError, Response, SafeFileName, Store,
    Target,
};

/// Clock from the system wall time. Failure to read it yields zero, which freshness treats as stale.
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_secs()).unwrap_or(0)
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
    if let Some(root) = std::env::var_os("DCTX_PLUGIN_ROOT") {
        return PathBuf::from(root);
    }
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

/// Managed plugin root. Reads succeed when the directory is absent; mutations create it.
pub struct FsStore {
    root: PathBuf,
    held: Mutex<Option<u64>>,
    next: AtomicU64,
}

impl FsStore {
    /// Binds `root` without creating it.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), held: Mutex::new(None), next: AtomicU64::new(1) }
    }

    fn registry_path(&self) -> PathBuf {
        self.root.join("registry.json")
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join("registry.lock")
    }

    fn ensure_root(&self) -> Result<(), DistributionError> {
        fs::create_dir_all(&self.root).map_err(|_| DistributionError::Interrupted)?;
        private_dir(&self.root)
    }

    fn key_dir(&self, kind: &str, key: &InstallKey) -> PathBuf {
        let mut path = self.root.join(kind);
        for component in key.components() {
            path.push(component);
        }
        path
    }

    fn check(&self, token: LockToken) -> Result<(), DistributionError> {
        let held = self.held.lock().map_err(|_| DistributionError::Interrupted)?;
        if *held == Some(token.value()) { Ok(()) } else { Err(DistributionError::Busy) }
    }
}

impl Store for FsStore {
    fn try_lock(&self) -> Result<LockToken, DistributionError> {
        let mut held = self.held.lock().map_err(|_| DistributionError::Interrupted)?;
        if held.is_some() {
            return Err(DistributionError::Busy);
        }
        self.ensure_root()?;
        let file = OpenOptions::new().write(true).create_new(true).open(self.lock_path());
        if file.is_err() {
            return Err(DistributionError::Busy);
        }
        let token = self.next.fetch_add(1, Ordering::Relaxed);
        *held = Some(token);
        Ok(LockToken::new(token))
    }

    fn unlock(&self, token: LockToken) {
        let mut held = match self.held.lock() {
            Ok(held) => held,
            Err(_) => return,
        };
        if *held == Some(token.value()) {
            *held = None;
            let _ = fs::remove_file(self.lock_path());
        }
    }

    fn load_registry(&self) -> Result<Option<Vec<u8>>, DistributionError> {
        match File::open(self.registry_path()) {
            Ok(mut file) => {
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes).map_err(|_| DistributionError::Interrupted)?;
                Ok(Some(bytes))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(DistributionError::Interrupted),
        }
    }

    fn save_registry(&self, token: LockToken, bytes: &[u8]) -> Result<(), DistributionError> {
        self.check(token)?;
        self.ensure_root()?;
        let temporary = self.root.join("registry.json.tmp");
        {
            let mut file = File::create(&temporary).map_err(|_| DistributionError::Interrupted)?;
            file.write_all(bytes).map_err(|_| DistributionError::Interrupted)?;
            private_file(&temporary)?;
        }
        fs::rename(&temporary, self.registry_path()).map_err(|_| DistributionError::Interrupted)
    }

    fn stage_append(&self, token: LockToken, key: &InstallKey, file: &SafeFileName, bytes: &[u8]) -> Result<(), DistributionError> {
        self.check(token)?;
        let directory = self.key_dir("staging", key);
        fs::create_dir_all(&directory).map_err(|_| DistributionError::Interrupted)?;
        private_dir(&directory)?;
        let path = directory.join(file.as_str());
        let mut handle = OpenOptions::new().create(true).append(true).open(&path).map_err(|_| DistributionError::Interrupted)?;
        handle.write_all(bytes).map_err(|_| DistributionError::Interrupted)?;
        private_file(&path)
    }

    fn publish(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> {
        self.check(token)?;
        let staging = self.key_dir("staging", key);
        let installed = self.key_dir("installed", key);
        if installed.exists() {
            return Err(DistributionError::Interrupted);
        }
        if let Some(parent) = installed.parent() {
            fs::create_dir_all(parent).map_err(|_| DistributionError::Interrupted)?;
        }
        fs::rename(staging, installed).map_err(|_| DistributionError::Interrupted)
    }

    fn discard_staging(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> {
        self.check(token)?;
        let directory = self.key_dir("staging", key);
        if directory.exists() {
            fs::remove_dir_all(directory).map_err(|_| DistributionError::Interrupted)?;
        }
        Ok(())
    }

    fn staged(&self) -> Result<Vec<InstallKey>, DistributionError> {
        walk_keys(&self.root.join("staging"))
    }

    fn installed(&self) -> Result<Vec<InstallKey>, DistributionError> {
        walk_keys(&self.root.join("installed"))
    }

    fn read_installed(&self, key: &InstallKey, file: &SafeFileName) -> Result<Vec<u8>, DistributionError> {
        let path = self.key_dir("installed", key).join(file.as_str());
        fs::read(path).map_err(|_| DistributionError::NotInstalled)
    }

    fn remove_installed(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> {
        self.check(token)?;
        let directory = self.key_dir("installed", key);
        if directory.exists() {
            fs::remove_dir_all(directory).map_err(|_| DistributionError::Interrupted)?;
        }
        Ok(())
    }
}

fn private_dir(path: &Path) -> Result<(), DistributionError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).map_err(|_| DistributionError::Interrupted)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions).map_err(|_| DistributionError::Interrupted)?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn private_file(path: &Path) -> Result<(), DistributionError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).map_err(|_| DistributionError::Interrupted)?.permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(path, permissions).map_err(|_| DistributionError::Interrupted)?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn walk_keys(root: &Path) -> Result<Vec<InstallKey>, DistributionError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut keys = Vec::new();
    let publishers = fs::read_dir(root).map_err(|_| DistributionError::Interrupted)?;
    for publisher in publishers {
        let publisher = publisher.map_err(|_| DistributionError::Interrupted)?;
        let names = match fs::read_dir(publisher.path()) {
            Ok(names) => names,
            Err(_) => continue,
        };
        for name in names {
            let name = name.map_err(|_| DistributionError::Interrupted)?;
            let versions = match fs::read_dir(name.path()) {
                Ok(versions) => versions,
                Err(_) => continue,
            };
            for version in versions {
                let version = version.map_err(|_| DistributionError::Interrupted)?;
                let digests = match fs::read_dir(version.path()) {
                    Ok(digests) => digests,
                    Err(_) => continue,
                };
                for digest in digests {
                    let digest = digest.map_err(|_| DistributionError::Interrupted)?;
                    let id = format!(
                        "{}/{}",
                        publisher.file_name().to_string_lossy(),
                        name.file_name().to_string_lossy()
                    );
                    let parsed = install_key(&id, &version.file_name().to_string_lossy(), &digest.file_name().to_string_lossy());
                    if let Ok(key) = parsed {
                        keys.push(key);
                    }
                }
            }
        }
    }
    Ok(keys)
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
mod tests {
    use super::*;

    #[test]
    fn a_missing_root_is_not_created_by_a_registry_read() {
        let root = std::env::temp_dir().join(format!("dctx-cli-disk-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let store = FsStore::new(&root);
        assert_eq!(store.load_registry().unwrap(), None);
        assert!(!root.exists());
        assert!(store.staged().unwrap().is_empty());
    }
}
