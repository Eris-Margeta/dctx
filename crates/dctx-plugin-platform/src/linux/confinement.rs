// SPDX-License-Identifier: Apache-2.0
//! Irreversible restriction of an already-loaded trusted private worker entry.
//! Implements Landlock ABI4 plus seccomp, pipe-only descriptors and parent-death handling.
//! This is not arbitrary ELF admission: the caller/bootstrap and its loaded runtime are trusted.

use super::filter;
use crate::{NoSecretEnvironment, PlatformError, Unsupported};
use landlock::{
    ABI, Access, AccessFs, AccessNet, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetStatus,
};
use rustix::fs::{FileType, fstat};
use rustix::process::{DumpableBehavior, Resource, Rlimit, Signal};
use std::fs;

/// Trusted private-entry parameters; constructing these is not host consent.
#[derive(Clone, Copy, Debug)]
pub struct RestrictedEntry {
    parent: u32,
    memory_bytes: u64,
}

impl RestrictedEntry {
    /// Binds the expected supervising parent and a 16 MiB–1 GiB address-space ceiling.
    /// No total-runtime deadline is imposed; progress policy belongs to the host.
    ///
    /// # Errors
    /// Returns a resource-limit error when the parent PID or the address-space ceiling is outside its supported range.
    pub fn new(parent: u32, memory_bytes: u64) -> Result<Self, PlatformError> {
        if parent <= 1
            || parent > i32::MAX as u32
            || !(16 * 1024 * 1024..=1024 * 1024 * 1024).contains(&memory_bytes)
        {
            return Err(PlatformError::ResourceLimit);
        }
        Ok(Self {
            parent,
            memory_bytes,
        })
    }
}

fn unsupported(reason: Unsupported) -> PlatformError {
    PlatformError::Unsupported(reason)
}

fn bootstrap() -> Result<(), PlatformError> {
    if !NoSecretEnvironment.is_clean() {
        return Err(unsupported(Unsupported::Bootstrap));
    }
    let tasks = fs::read_dir("/proc/self/task")
        .map_err(|_| unsupported(Unsupported::Bootstrap))?
        .try_fold(0, |count, entry| entry.map(|_| count + 1))
        .map_err(|_| unsupported(Unsupported::Bootstrap))?;
    if tasks != 1 {
        return Err(unsupported(Unsupported::Bootstrap));
    }
    for fd in [
        rustix::stdio::stdin(),
        rustix::stdio::stdout(),
        rustix::stdio::stderr(),
    ] {
        let stat = fstat(fd).map_err(|_| unsupported(Unsupported::Bootstrap))?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::Fifo {
            return Err(unsupported(Unsupported::Bootstrap));
        }
    }
    let descriptors = fs::read_dir("/proc/self/fd")
        .map_err(|_| unsupported(Unsupported::Bootstrap))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| unsupported(Unsupported::Bootstrap))?;
    // Collection has closed the directory iterator; its own FD must no longer exist.
    for entry in descriptors {
        let fd = entry
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.parse::<u32>().ok())
            .ok_or_else(|| unsupported(Unsupported::Bootstrap))?;
        if fd > 2 {
            match fs::symlink_metadata(entry) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(unsupported(Unsupported::Bootstrap)),
            }
        }
    }
    Ok(())
}

#[allow(
    clippy::cast_possible_wrap,
    reason = "RestrictedEntry::new has already checked that the parent PID is in 2..=i32::MAX."
)]
fn lifecycle(entry: RestrictedEntry) -> Result<(), PlatformError> {
    let expected = rustix::process::Pid::from_raw(entry.parent as i32)
        .ok_or_else(|| unsupported(Unsupported::Lifecycle))?;
    if rustix::process::getppid() != Some(expected) {
        return Err(unsupported(Unsupported::Lifecycle));
    }
    rustix::process::set_parent_process_death_signal(Some(Signal::KILL))
        .map_err(|_| unsupported(Unsupported::Lifecycle))?;
    if rustix::process::getppid() != Some(expected) {
        return Err(unsupported(Unsupported::Lifecycle));
    }
    rustix::process::set_dumpable_behavior(DumpableBehavior::NotDumpable)
        .map_err(|_| unsupported(Unsupported::Lifecycle))?;
    for (resource, ceiling) in [
        (Resource::Core, 0),
        (Resource::Fsize, 0),
        (Resource::As, entry.memory_bytes),
    ] {
        let old = rustix::process::getrlimit(resource);
        let bound = old
            .current
            .unwrap_or(ceiling)
            .min(old.maximum.unwrap_or(ceiling))
            .min(ceiling);
        rustix::process::setrlimit(
            resource,
            Rlimit {
                current: Some(bound),
                maximum: Some(bound),
            },
        )
        .map_err(|_| unsupported(Unsupported::Lifecycle))?;
    }
    rustix::thread::set_no_new_privs(true).map_err(|_| unsupported(Unsupported::Lifecycle))?;
    std::env::set_current_dir("/").map_err(|_| unsupported(Unsupported::Bootstrap))?;
    Ok(())
}

/// Denies all filesystem content access and TCP bind/connect for the calling thread.
/// Requires complete ABI4 enforcement: older/disabled kernels fail rather than degrade.
/// This irreversible primitive alone is NOT confinement (pre-opened FDs/UDP/exec remain).
/// Use only in a disposable trusted process and exit on error, including partial setup.
///
/// # Errors
/// Returns an unsupported Landlock error when complete ABI4 enforcement cannot be established, including partial setup.
pub fn restrict_filesystem() -> Result<(), PlatformError> {
    let apply = || -> Result<_, landlock::RulesetError> {
        Ruleset::default()
            .set_compatibility(CompatLevel::HardRequirement)
            .handle_access(AccessFs::from_all(ABI::V4))?
            .handle_access(AccessNet::from_all(ABI::V4))?
            .create()?
            .restrict_self()
    };
    let status = apply().map_err(|_| unsupported(Unsupported::Landlock))?;
    if status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs {
        return Err(unsupported(Unsupported::Landlock));
    }
    Ok(())
}

/// Runs already-loaded trusted worker code after restriction, then exits the process.
/// Must be the early, single-threaded private entry, before plugin/user initialization.
/// The actual environment must be clean and only three protocol pipe FDs may be open.
/// Setup errors exit 125 without calling `body`; violations die with SIGSYS for host audit.
/// No untrusted ELF/constructor, loaded runtime or arbitrary closure is authenticated here.
/// The host must use qualified image binding and supervision before advertising a provider.
pub fn run_restricted(entry: RestrictedEntry, body: impl FnOnce() -> i32) -> ! {
    let setup = || {
        bootstrap()?;
        let policy = filter::compile()?;
        lifecycle(entry)?;
        restrict_filesystem()?;
        filter::install(&policy)
    };
    if let Err(error) = setup() {
        eprintln!("{error}");
        std::process::exit(125);
    }
    std::process::exit(body());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_parent_and_memory_limits_are_rejected_without_os_changes() {
        assert!(RestrictedEntry::new(10, 256 * 1024 * 1024).is_ok());
        assert!(RestrictedEntry::new(1, 256 * 1024 * 1024).is_err());
        assert!(RestrictedEntry::new(10, 0).is_err());
        assert!(RestrictedEntry::new(10, u64::MAX).is_err());
    }
}
