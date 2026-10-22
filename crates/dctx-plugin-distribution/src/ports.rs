//! Ports the installer needs: HTTPS fetch, the managed plugin store and a clock.
//! Implements the IO boundary of PLUGINS.md §5.3/§5.4: a reviewed HTTPS adapter (not a plugin grant)
//! and an owned data root with same-filesystem staging, immutable publish and atomic registry replace.
//! Depends on domain names/digests; implemented by `fakes` here and by platform adapters later.
//! No port can execute a file: installation never runs plugin code.

use std::io::Read;
use std::net::IpAddr;

use crate::domain::digest::Sha256Hex;
use crate::domain::names::{PluginId, SafeFileName, Version};
use crate::domain::origin::HttpsUrl;
use crate::error::{DistributionError, NetworkError};

/// Response body of one HTTPS request.
pub enum Body {
    /// Content stream; the caller enforces length limits while reading.
    Content(Box<dyn Read + Send>),
    /// A redirect `Location`, checked by the caller before it is followed.
    Redirect(String),
}

/// One HTTPS response.
pub struct Response {
    /// Address the connection actually reached, after DNS.
    pub peer: IpAddr,
    /// Body or redirect.
    pub body: Body,
}

/// Credential-free HTTPS GET. Adapters must not attach ambient browser or Git credentials.
pub trait Network {
    /// Issues one request without following redirects.
    /// # Errors
    /// Returns `NetworkError` when the credential-free HTTPS request cannot complete.
    fn get(&self, url: &HttpsUrl) -> Result<Response, NetworkError>;
}

/// Opaque proof of holding the shared CLI/app operation lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockToken(u64);

impl LockToken {
    /// Wraps an adapter-chosen token value.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The adapter's token value.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Immutable version directory identity: `<publisher>/<name>/<version>/<digest>`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstallKey {
    /// Plugin identity.
    pub id: PluginId,
    /// Exact version.
    pub version: Version,
    /// Executable digest.
    pub digest: Sha256Hex,
}

impl InstallKey {
    /// Validated relative path components; never string-concatenated from untrusted input.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn components(&self) -> [String; 4] {
        let (publisher, name) = self.id.components();
        [publisher.to_owned(), name.to_owned(), self.version.as_str().to_owned(), self.digest.to_string()]
    }
}

/// The managed plugin data root. Every mutation requires the operation lock.
pub trait Store {
    /// Takes the operation lock or returns [`DistributionError::Busy`].
    /// # Errors
    /// Returns `Busy` if the operation lock is held, or a storage error if locking fails.
    fn try_lock(&self) -> Result<LockToken, DistributionError>;
    /// Releases the operation lock.
    fn unlock(&self, token: LockToken);
    /// Current registry bytes, `None` before first use.
    /// # Errors
    /// Returns a storage error if the registry cannot be read safely.
    fn load_registry(&self) -> Result<Option<Vec<u8>>, DistributionError>;
    /// Atomically replaces the registry: readers see the old or the new bytes, never a mix.
    /// # Errors
    /// Returns a lock/storage error if the registry cannot be atomically replaced.
    fn save_registry(&self, token: LockToken, bytes: &[u8]) -> Result<(), DistributionError>;
    /// Appends bytes to a staged regular file (owner-only permissions, same filesystem as installs).
    /// # Errors
    /// Returns a lock/storage error if safe staging or append fails.
    fn stage_append(&self, token: LockToken, key: &InstallKey, file: &SafeFileName, bytes: &[u8]) -> Result<(), DistributionError>;
    /// Atomically publishes the staging directory as an immutable version directory.
    /// # Errors
    /// Returns a lock/storage error if the staging directory cannot be atomically published.
    fn publish(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError>;
    /// Deletes a staging directory, if any.
    /// # Errors
    /// Returns a lock/storage error if the staging directory cannot be safely removed.
    fn discard_staging(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError>;
    /// Keys with leftover staging directories.
    /// # Errors
    /// Returns a storage error if staged install keys cannot be enumerated safely.
    fn staged(&self) -> Result<Vec<InstallKey>, DistributionError>;
    /// Keys of published version directories.
    /// # Errors
    /// Returns a storage error if published install keys cannot be enumerated safely.
    fn installed(&self) -> Result<Vec<InstallKey>, DistributionError>;
    /// Reads one published file for reverification.
    /// # Errors
    /// Returns a storage error if the published file cannot be safely read.
    fn read_installed(&self, key: &InstallKey, file: &SafeFileName) -> Result<Vec<u8>, DistributionError>;
    /// Unlinks a published version directory.
    /// # Errors
    /// Returns a lock/storage error if the immutable version directory cannot be safely removed.
    fn remove_installed(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError>;
}

/// Wall clock in Unix seconds, injected for deterministic freshness tests.
pub trait Clock {
    /// Current time.
    fn now(&self) -> u64;
}
