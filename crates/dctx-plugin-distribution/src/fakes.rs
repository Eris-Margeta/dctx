//! Deterministic in-memory port doubles: fake HTTPS network, fault-injecting store, fixed clock.
//! Implements the "fake network" and interruption seams of PLUGINS.md §7.2 row PL-I.
//! Depends on ports only; used by this crate's tests and by PL-CLI/PL-APP tests. Never a production adapter.
//! The store records every staged byte so tests can prove partial payloads were never published.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::domain::names::SafeFileName;
use crate::domain::origin::HttpsUrl;
use crate::error::{DistributionError, NetworkError};
use crate::ports::{Body, Clock, InstallKey, LockToken, Network, Response, Store};

/// Public documentation address used as the default fake peer.
pub const PUBLIC_PEER: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));

/// One scripted response.
#[derive(Clone, Debug)]
pub enum FakeResponse {
    /// Body bytes from `peer`; `fail_after` truncates the stream with an IO error.
    Content {
        /// Bytes served.
        bytes: Vec<u8>,
        /// Connected address.
        peer: IpAddr,
        /// Bytes delivered before an interruption.
        fail_after: Option<usize>,
    },
    /// Redirect to a location.
    Redirect(String),
    /// Typed network failure.
    Fail(NetworkError),
}

/// Scripted HTTPS network keyed by exact URL text; unknown URLs are `NotFound`.
#[derive(Debug, Default)]
pub struct FakeNetwork {
    routes: Mutex<BTreeMap<String, FakeResponse>>,
    requests: Mutex<Vec<String>>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl FakeNetwork {
    /// Empty network.
    pub fn new() -> Self {
        Self::default()
    }

    /// Scripts `url`.
    pub fn route(&self, url: &str, response: FakeResponse) {
        locked(&self.routes).insert(url.to_owned(), response);
    }

    /// Serves `bytes` at `url` from a public peer.
    pub fn serve(&self, url: &str, bytes: &[u8]) {
        self.route(url, FakeResponse::Content { bytes: bytes.to_vec(), peer: PUBLIC_PEER, fail_after: None });
    }

    /// Removes a route.
    pub fn unroute(&self, url: &str) {
        locked(&self.routes).remove(url);
    }

    /// Every URL requested, in order.
    pub fn requests(&self) -> Vec<String> {
        locked(&self.requests).clone()
    }
}

struct Interrupting {
    bytes: Vec<u8>,
    at: usize,
    fail_after: Option<usize>,
}

impl Read for Interrupting {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let end = self.fail_after.unwrap_or(self.bytes.len()).min(self.bytes.len());
        if self.at >= end {
            return match self.fail_after {
                Some(_) => Err(io::Error::new(io::ErrorKind::ConnectionReset, "fake interruption")),
                None => Ok(0),
            };
        }
        let count = out.len().min(end - self.at).min(4096);
        out[..count].copy_from_slice(&self.bytes[self.at..self.at + count]);
        self.at += count;
        Ok(count)
    }
}

impl Network for FakeNetwork {
    fn get(&self, url: &HttpsUrl) -> Result<Response, NetworkError> {
        let text = url.to_string();
        locked(&self.requests).push(text.clone());
        match locked(&self.routes).get(&text).cloned() {
            None => Err(NetworkError::NotFound),
            Some(FakeResponse::Fail(error)) => Err(error),
            Some(FakeResponse::Redirect(location)) => Ok(Response { peer: PUBLIC_PEER, body: Body::Redirect(location) }),
            Some(FakeResponse::Content { bytes, peer, fail_after }) => {
                Ok(Response { peer, body: Body::Content(Box::new(Interrupting { bytes, at: 0, fail_after })) })
            }
        }
    }
}

/// Store operation at which a one-shot fault fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// While appending staged bytes.
    Stage,
    /// While publishing a version directory.
    Publish,
    /// While replacing the registry.
    SaveRegistry,
}

type Files = BTreeMap<String, Vec<u8>>;

#[derive(Debug, Default)]
struct MemoryState {
    lock: Option<u64>,
    next_token: u64,
    registry: Option<Vec<u8>>,
    staging: BTreeMap<InstallKey, Files>,
    installed: BTreeMap<InstallKey, Files>,
    fault: Option<Fault>,
}

impl MemoryState {
    fn check(&self, token: LockToken) -> Result<(), DistributionError> {
        if self.lock == Some(token.value()) { Ok(()) } else { Err(DistributionError::Busy) }
    }

    fn fire(&mut self, at: Fault) -> Result<(), DistributionError> {
        if self.fault == Some(at) {
            self.fault = None;
            return Err(DistributionError::Interrupted);
        }
        Ok(())
    }
}

/// Shared in-memory data root; clones share state like CLI and app sharing one registry.
#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
    state: Arc<Mutex<MemoryState>>,
}

impl MemoryStore {
    /// Empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Arms a one-shot fault.
    pub fn inject(&self, fault: Fault) {
        locked(&self.state).fault = Some(fault);
    }

    /// Simulates another process holding the lock.
    pub fn hold_lock(&self) -> LockToken {
        let mut state = locked(&self.state);
        state.next_token += 1;
        state.lock = Some(state.next_token);
        LockToken::new(state.next_token)
    }

    /// Files of a published directory.
    pub fn installed_files(&self, key: &InstallKey) -> Option<Vec<String>> {
        locked(&self.state).installed.get(key).map(|files| files.keys().cloned().collect())
    }

    /// Overwrites one published file, simulating tampering on disk.
    pub fn tamper(&self, key: &InstallKey, file: &str, bytes: &[u8]) {
        if let Some(files) = locked(&self.state).installed.get_mut(key) {
            files.insert(file.to_owned(), bytes.to_vec());
        }
    }
}

impl Store for MemoryStore {
    fn try_lock(&self) -> Result<LockToken, DistributionError> {
        let mut state = locked(&self.state);
        if state.lock.is_some() {
            return Err(DistributionError::Busy);
        }
        state.next_token += 1;
        state.lock = Some(state.next_token);
        Ok(LockToken::new(state.next_token))
    }

    fn unlock(&self, token: LockToken) {
        let mut state = locked(&self.state);
        if state.lock == Some(token.value()) {
            state.lock = None;
        }
    }

    fn load_registry(&self) -> Result<Option<Vec<u8>>, DistributionError> {
        Ok(locked(&self.state).registry.clone())
    }

    fn save_registry(&self, token: LockToken, bytes: &[u8]) -> Result<(), DistributionError> {
        let mut state = locked(&self.state);
        state.check(token)?;
        state.fire(Fault::SaveRegistry)?;
        state.registry = Some(bytes.to_vec());
        Ok(())
    }

    fn stage_append(&self, token: LockToken, key: &InstallKey, file: &SafeFileName, bytes: &[u8]) -> Result<(), DistributionError> {
        let mut state = locked(&self.state);
        state.check(token)?;
        state.fire(Fault::Stage)?;
        state.staging.entry(key.clone()).or_default().entry(file.as_str().to_owned()).or_default().extend_from_slice(bytes);
        Ok(())
    }

    fn publish(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> {
        let mut state = locked(&self.state);
        state.check(token)?;
        state.fire(Fault::Publish)?;
        if state.installed.contains_key(key) {
            return Err(DistributionError::Interrupted);
        }
        let files = state.staging.remove(key).ok_or(DistributionError::NotInstalled)?;
        state.installed.insert(key.clone(), files);
        Ok(())
    }

    fn discard_staging(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> {
        let mut state = locked(&self.state);
        state.check(token)?;
        state.staging.remove(key);
        Ok(())
    }

    fn staged(&self) -> Result<Vec<InstallKey>, DistributionError> {
        Ok(locked(&self.state).staging.keys().cloned().collect())
    }

    fn installed(&self) -> Result<Vec<InstallKey>, DistributionError> {
        Ok(locked(&self.state).installed.keys().cloned().collect())
    }

    fn read_installed(&self, key: &InstallKey, file: &SafeFileName) -> Result<Vec<u8>, DistributionError> {
        let state = locked(&self.state);
        state.installed.get(key).and_then(|files| files.get(file.as_str())).cloned().ok_or(DistributionError::NotInstalled)
    }

    fn remove_installed(&self, token: LockToken, key: &InstallKey) -> Result<(), DistributionError> {
        let mut state = locked(&self.state);
        state.check(token)?;
        state.installed.remove(key);
        Ok(())
    }
}

/// Settable clock.
#[derive(Debug, Default)]
pub struct FixedClock(AtomicU64);

impl FixedClock {
    /// Clock at `now`.
    pub fn at(now: u64) -> Self {
        Self(AtomicU64::new(now))
    }

    /// Moves the clock.
    pub fn set(&self, now: u64) {
        self.0.store(now, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
