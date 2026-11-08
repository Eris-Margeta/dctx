// SPDX-License-Identifier: Apache-2.0
//! The persisted cache policy beside the ledger and the auto-clean report over it.
//! The session entry points that use these helpers stay in the host module.

use super::*;

/// CLI policy beside the ledger. Cache purge does not own this file.
const POLICY_FILE: &str = "policy.v1";

/// Persisted auto-clean policy and the last reported reclaim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CachePolicy {
    /// Policy passed to the facade on the next open.
    pub auto: AutoCleanConfig,
    /// Bytes the last due slice reported. Zero before the first reclaim.
    pub last_reclaimed_bytes: u64,
}

/// What `dctx cache auto` prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoReport {
    /// Persisted enable flag.
    pub enabled: bool,
    /// Ledger stamp, when a slice has run.
    pub last_auto_clean_ms: Option<u64>,
    /// Bytes recorded for that slice.
    pub reclaimed_bytes: u64,
    /// Wall time the next slice becomes due.
    pub next_due_ms: u64,
}

/// Reads `policy.v1`. A missing file is the product default.
/// # Errors
/// Returns unreadable or malformed cache policy errors; a missing policy uses defaults.
pub fn load_policy(root: &Path) -> Result<CachePolicy, String> {
    match fs::symlink_metadata(root) {
        Ok(_) => {
            // Keep the cache adapter's ownership, permissions and ancestor checks
            // on the original path before resolving its trusted ancestors.
            FsLedgerStore::open(root).map_err(|error| error.to_string())?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CachePolicy::default());
        }
        Err(error) => return Err(error.to_string()),
    }
    // Resolve ancestors only: the cache root itself must remain a no-follow
    // component even if it is replaced after the cache adapter's validation.
    let root = match (root.parent(), root.file_name()) {
        (Some(parent), Some(name)) => fs::canonicalize(parent).map(|parent| parent.join(name)),
        _ => fs::canonicalize(root),
    }
    .map_err(|error| error.to_string())?;
    let path = root.join(POLICY_FILE);
    #[cfg(unix)]
    if let Ok(meta) = fs::symlink_metadata(&path) {
        use std::os::unix::fs::MetadataExt;
        let owner = fs::symlink_metadata(&root)
            .map_err(|_| "cache root metadata failed")?
            .uid();
        if meta.uid() != owner || meta.mode() & 0o022 != 0 {
            return Err("unsafe cache policy ownership or write permissions".into());
        }
    }
    use dctx_adapter_fs::{SourceFs, StdFs};
    match StdFs.read_policy(&path) {
        Ok(Some(text)) => parse_policy(&text),
        Ok(None) => Ok(CachePolicy::default()),
        Err(_) => Err("corrupt cache policy: unreadable".to_string()),
    }
}

/// Shows the policy, or stores a tightened one. A show does not create the file.
/// # Errors
/// Returns policy loading/validation, cap application, ledger, or policy persistence failures.
pub fn configure(
    root: &Path,
    auto: Option<bool>,
    max_bytes: Option<u64>,
    ttl_days: Option<u64>,
) -> Result<CachePolicy, String> {
    let mut policy = load_policy(root)?;
    if auto.is_none() && max_bytes.is_none() && ttl_days.is_none() {
        return Ok(policy);
    }
    if let Some(enabled) = auto {
        policy.auto.enabled = enabled;
    }
    if let Some(days) = ttl_days {
        let millis = days.saturating_mul(dctx_adapter_cache::DAY_MS);
        policy.auto.overlay_ttl_ms = millis;
        policy.auto.preview_ttl_ms = millis;
    }
    if let Some(cap) = max_bytes {
        apply_cap(root, &mut policy, cap)?;
    }
    store_policy(root, &policy)?;
    Ok(policy)
}

/// Last slice, recorded reclaim and the next due time. This does not run a slice.
/// # Errors
/// Returns policy or ledger loading failures.
pub fn auto_report(root: &Path) -> Result<AutoReport, String> {
    let policy = load_policy(root)?;
    let last = load_ledger(root)?.last_auto_clean().map(|stamp| stamp.0);
    let next_due_ms = match last {
        Some(stamp) => stamp.saturating_add(policy.auto.min_interval_ms),
        None => unix_millis(),
    };
    Ok(AutoReport {
        enabled: policy.auto.enabled,
        last_auto_clean_ms: last,
        reclaimed_bytes: policy.last_reclaimed_bytes,
        next_due_ms,
    })
}

fn apply_cap(root: &Path, policy: &mut CachePolicy, cap: u64) -> Result<(), String> {
    if cap > HARD_CAP_BYTES {
        return Err("system cap cannot exceed the product hard cap".to_string());
    }
    with_ledger(root, |life| {
        let current = life.status().hard_cap;
        if cap > current {
            return Err("max-bytes can only tighten the system cap".to_string());
        }
        if cap < current {
            life.set_hard_cap(cap).map_err(|error| error.to_string())?;
        }
        Ok(((), cap < current))
    })?;
    policy.auto.system_cap_bytes = cap;
    Ok(())
}

pub(super) fn store_policy(root: &Path, policy: &CachePolicy) -> Result<(), String> {
    ensure_cache_dir(root)?;
    let body = format!(
        "enabled={}\nmin_interval_ms={}\noverlay_ttl_ms={}\npreview_ttl_ms={}\nroot_ttl_ms={}\nstaging_ttl_ms={}\nsystem_cap_bytes={}\nlast_reclaimed_bytes={}\n",
        u8::from(policy.auto.enabled),
        policy.auto.min_interval_ms,
        policy.auto.overlay_ttl_ms,
        policy.auto.preview_ttl_ms,
        policy.auto.root_ttl_ms,
        policy.auto.staging_ttl_ms,
        policy.auto.system_cap_bytes,
        policy.last_reclaimed_bytes
    );
    let temp = root.join("policy.v1.tmp");
    write_private(&temp, body.as_bytes())?;
    fs::rename(&temp, root.join(POLICY_FILE))
        .map_err(|_| "failed to commit the cache policy".to_string())
}

fn parse_policy(text: &str) -> Result<CachePolicy, String> {
    let mut policy = CachePolicy::default();
    let mut seen = [false; 8];
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| "corrupt cache policy: line".to_string())?;
        let index = policy_key(key)?;
        if seen[index] {
            return Err("corrupt cache policy: duplicate".to_string());
        }
        seen[index] = true;
        assign_policy(&mut policy, index, value)?;
    }
    if seen.iter().any(|present| !present) {
        return Err("corrupt cache policy: missing field".to_string());
    }
    Ok(policy)
}

fn policy_key(key: &str) -> Result<usize, String> {
    let index = [
        "enabled",
        "min_interval_ms",
        "overlay_ttl_ms",
        "preview_ttl_ms",
        "root_ttl_ms",
        "staging_ttl_ms",
        "system_cap_bytes",
        "last_reclaimed_bytes",
    ]
    .iter()
    .position(|name| *name == key)
    .ok_or_else(|| "corrupt cache policy: field".to_string())?;
    Ok(index)
}

fn assign_policy(policy: &mut CachePolicy, index: usize, value: &str) -> Result<(), String> {
    let number: u64 = value
        .parse()
        .map_err(|_| "corrupt cache policy: number".to_string())?;
    match index {
        0 => {
            policy.auto.enabled = match number {
                0 => false,
                1 => true,
                _ => return Err("corrupt cache policy: enabled".to_string()),
            };
        }
        1 => policy.auto.min_interval_ms = number,
        2 => policy.auto.overlay_ttl_ms = number,
        3 => policy.auto.preview_ttl_ms = number,
        4 => policy.auto.root_ttl_ms = number,
        5 => policy.auto.staging_ttl_ms = number,
        6 => policy.auto.system_cap_bytes = number,
        _ => policy.last_reclaimed_bytes = number,
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "failed to write the cache policy".to_string())?;
    file.write_all(bytes)
        .map_err(|_| "failed to write the cache policy".to_string())?;
    Ok(())
}

#[cfg(all(test, unix))]
mod security_tests {
    use super::*;

    #[test]
    fn trusted_linked_cache_ancestors_work_but_linked_roots_and_policies_do_not() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        fs::create_dir(&real).unwrap();
        let linked = temp.path().join("linked");
        std::os::unix::fs::symlink(&real, &linked).unwrap();
        let root = linked.join("cache");
        let policy = CachePolicy {
            last_reclaimed_bytes: 42,
            ..CachePolicy::default()
        };
        store_policy(&root, &policy).unwrap();
        assert_eq!(load_policy(&root).unwrap(), policy);
        let linked_root = temp.path().join("linked-root");
        std::os::unix::fs::symlink(real.join("cache"), &linked_root).unwrap();
        assert!(load_policy(&linked_root).is_err());
        let outside = temp.path().join("outside");
        fs::write(&outside, b"enabled=0\n").unwrap();
        fs::remove_file(root.join(POLICY_FILE)).unwrap();
        std::os::unix::fs::symlink(&outside, root.join(POLICY_FILE)).unwrap();
        assert!(load_policy(&root).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"enabled=0\n");
    }

    #[test]
    fn a_world_writable_policy_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        store_policy(temp.path(), &CachePolicy::default()).unwrap();
        let path = temp.path().join(POLICY_FILE);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(load_policy(temp.path()).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(load_policy(temp.path()).unwrap(), CachePolicy::default());
    }

    #[test]
    fn linked_policy_or_staging_file_cannot_read_or_overwrite_outside_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret");
        fs::write(&secret, b"keep").unwrap();
        let policy = temp.path().join(POLICY_FILE);
        std::os::unix::fs::symlink(&secret, &policy).unwrap();
        assert!(load_policy(temp.path()).is_err());
        fs::remove_file(&policy).unwrap();
        std::os::unix::fs::symlink(&secret, temp.path().join("policy.v1.tmp")).unwrap();
        assert!(store_policy(temp.path(), &CachePolicy::default()).is_err());
        assert_eq!(fs::read(secret).unwrap(), b"keep");
    }
}

#[cfg(test)]
mod robustness {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn robustness_cache_policy(text in ".{0,4096}") {
            let _ = parse_policy(&text);
        }
    }
}
