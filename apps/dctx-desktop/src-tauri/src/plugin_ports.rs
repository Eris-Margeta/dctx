//! Filesystem, clock and offline network adapters for desktop plugin management.
//! Implements PLUGINS.md §5.4 and §7.3 PL-APP, following the CLI filesystem store.
//! Depends on distribution ports and std; plugin_commands binds the desktop config root.
//! Reads never create directories. No adapter executes Git or connects to a network.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use dctx_plugin_distribution::{
    Clock, DistributionError, Document, HttpsUrl, InstallKey, LockToken, Network, NetworkError,
    PluginId, Response, SafeFileName, Sha256Hex, Store, Target, Version,
};

/// Wall time in Unix seconds; zero on clock failure is treated as stale.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|time| time.as_secs())
            .unwrap_or(0)
    }
}

/// Network port that refuses every fetch until a reviewed adapter is approved.
pub struct Offline;

impl Network for Offline {
    fn get(&self, _url: &HttpsUrl) -> Result<Response, NetworkError> {
        Err(NetworkError::Unreachable)
    }
}

/// Compile-time host target, matching the CLI install policy.
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

/// Managed plugin root. Only mutations create directories.
pub struct FsStore {
    root: PathBuf,
    held: Mutex<Option<u64>>,
    next: AtomicU64,
}

impl FsStore {
    /// Binds the desktop config directory's plugins folder without creating it.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            held: Mutex::new(None),
            next: AtomicU64::new(1),
        }
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join("registry.lock")
    }

    fn ensure_root(&self) -> Result<(), DistributionError> {
        fs::create_dir_all(&self.root).map_err(|_| DistributionError::Interrupted)?;
        private_permissions(&self.root, 0o700)
    }

    fn key_dir(&self, kind: &str, key: &InstallKey) -> PathBuf {
        let mut path = self.root.join(kind);
        for component in key.components() {
            path.push(component);
        }
        path
    }

    fn check(&self, token: LockToken) -> Result<(), DistributionError> {
        let held = self
            .held
            .lock()
            .map_err(|_| DistributionError::Interrupted)?;
        if *held == Some(token.value()) {
            Ok(())
        } else {
            Err(DistributionError::Busy)
        }
    }
}

impl Store for FsStore {
    fn try_lock(&self) -> Result<LockToken, DistributionError> {
        let mut held = self
            .held
            .lock()
            .map_err(|_| DistributionError::Interrupted)?;
        if held.is_some() {
            return Err(DistributionError::Busy);
        }
        self.ensure_root()?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.lock_path())
            .map_err(|_| DistributionError::Busy)?;
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
        match File::open(self.root.join("registry.json")) {
            Ok(mut file) => {
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)
                    .map_err(|_| DistributionError::Interrupted)?;
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
            private_permissions(&temporary, 0o600)?;
            file.write_all(bytes)
                .map_err(|_| DistributionError::Interrupted)?;
        }
        fs::rename(temporary, self.root.join("registry.json"))
            .map_err(|_| DistributionError::Interrupted)
    }

    fn stage_append(
        &self,
        token: LockToken,
        key: &InstallKey,
        file: &SafeFileName,
        bytes: &[u8],
    ) -> Result<(), DistributionError> {
        self.check(token)?;
        let directory = self.key_dir("staging", key);
        fs::create_dir_all(&directory).map_err(|_| DistributionError::Interrupted)?;
        private_permissions(&directory, 0o700)?;
        let path = directory.join(file.as_str());
        let mut handle = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|_| DistributionError::Interrupted)?;
        private_permissions(&path, 0o600)?;
        handle
            .write_all(bytes)
            .map_err(|_| DistributionError::Interrupted)
    }

    fn publish(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> {
        self.check(token)?;
        let installed = self.key_dir("installed", key);
        if installed.exists() {
            return Err(DistributionError::Interrupted);
        }
        if let Some(parent) = installed.parent() {
            fs::create_dir_all(parent).map_err(|_| DistributionError::Interrupted)?;
        }
        fs::rename(self.key_dir("staging", key), installed)
            .map_err(|_| DistributionError::Interrupted)
    }

    fn discard_staging(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> {
        self.check(token)?;
        remove_directory(&self.key_dir("staging", key))
    }

    fn staged(&self) -> Result<Vec<InstallKey>, DistributionError> {
        walk_keys(&self.root.join("staging"))
    }

    fn installed(&self) -> Result<Vec<InstallKey>, DistributionError> {
        walk_keys(&self.root.join("installed"))
    }

    fn read_installed(
        &self,
        key: &InstallKey,
        file: &SafeFileName,
    ) -> Result<Vec<u8>, DistributionError> {
        fs::read(self.key_dir("installed", key).join(file.as_str()))
            .map_err(|_| DistributionError::NotInstalled)
    }

    fn remove_installed(
        &self,
        token: LockToken,
        key: &InstallKey,
    ) -> Result<(), DistributionError> {
        self.check(token)?;
        remove_directory(&self.key_dir("installed", key))
    }
}

fn remove_directory(path: &Path) -> Result<(), DistributionError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(DistributionError::Interrupted),
    }
}

fn private_permissions(path: &Path, mode: u32) -> Result<(), DistributionError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|_| DistributionError::Interrupted)?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

fn walk_keys(root: &Path) -> Result<Vec<InstallKey>, DistributionError> {
    let publishers = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(DistributionError::Interrupted),
    };
    let mut keys = Vec::new();
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
                    if let Ok(key) = install_key(
                        &id,
                        &version.file_name().to_string_lossy(),
                        &digest.file_name().to_string_lossy(),
                    ) {
                        keys.push(key);
                    }
                }
            }
        }
    }
    Ok(keys)
}

fn install_key(id: &str, version: &str, digest: &str) -> Result<InstallKey, DistributionError> {
    Ok(InstallKey {
        id: PluginId::parse(id)?,
        version: Version::parse(version)?,
        digest: Sha256Hex::parse(digest, Document::Registry)?,
    })
}
