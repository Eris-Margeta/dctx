//! `dctx cache status|gc|purge|config|auto` over the persisted facade ledger.
//! Implements ARCHITECTURE-PROPOSAL §4 and §7 row DX-D2.
//! Depends on `dctx-adapter-cache` for quotas and on [`crate::host`] for the cache root and facade auto-clean.
//! Purge apply requires `--root` or `--all`, plus `--yes`. Dry-run changes no bytes.

mod parse;
mod text;

use std::collections::BTreeMap;
use std::path::Path;

use dctx_adapter_cache::{
    ClockSample, GcRequest, KnownExport, Lifecycle, PurgeRequest, PurgeScope, RootId, SliceBudget,
};

use crate::host::{self, SessionCacheCall};
use parse::{Command, Scope};

/// Path or label to a registered root. The ledger never invents a root from a directory walk.
#[derive(Debug, Default, Clone)]
pub struct RootTable {
    paths: BTreeMap<String, RootId>,
}



/// Exports the legacy inspector may classify. The CLI does not discover these on disk.
#[derive(Debug)]
pub struct CacheEnv<'a> {
    /// Managed ledger for this process.
    pub life: &'a mut Lifecycle,
    /// Clock sample applied before a sweep so expiry can be tested.
    pub clock: ClockSample,
    /// Registered root tokens.
    pub roots: &'a RootTable,
    /// Legacy exports already read by the caller.
    pub exports: &'a [KnownExport],
}

/// Runs a cache command against the persisted ledger and prints the report.
pub fn run(args: &[String]) -> Result<(), String> {
    let report = run_in(args, None)?;
    if report.is_empty() {
        return Ok(());
    }
    print!("{report}");
    if !report.ends_with('\n') {
        println!();
    }
    Ok(())
}

/// Same as [`run`] for an explicit cache root. `None` uses [`host::cache_root_dir`].
pub fn run_in(args: &[String], root: Option<&Path>) -> Result<String, String> {
    let command = parse::parse(args)?;
    if matches!(command, Command::Help) {
        return Ok(crate::help::text());
    }
    let resolved = match root {
        Some(path) => path.to_path_buf(),
        None => host::cache_root_dir()?,
    };
    host::ensure_cache_dir(&resolved)?;
    match command {
        Command::Help => Ok(crate::help::text()),
        Command::Config { json, auto, max_bytes, ttl_days } => {
            let policy = host::configure(&resolved, auto, max_bytes, ttl_days)?;
            Ok(text::config_text(&policy, json))
        }
        Command::Auto { json } => Ok(text::auto_text(&host::auto_report(&resolved)?, json)),
        other => persisted(other, &resolved),
    }
}

/// Applies `args` to an injected ledger. Persistence tests use [`run_in`].
#[cfg(test)]
fn execute(args: &[String], env: CacheEnv<'_>) -> Result<String, String> {
    apply(parse::parse(args)?, env)
}

fn persisted(command: Command, root: &Path) -> Result<String, String> {
    let commit = matches!(command, Command::Gc { dry_run: false, .. } | Command::Purge { dry_run: false, .. });
    host::with_ledger(root, |life| {
        let roots = RootTable::default();
        let exports = Vec::new();
        let now = host::unix_millis();
        let clock = ClockSample { wall: dctx::Timestamp(now), monotonic_ms: now, boot_id: 1 };
        let env = CacheEnv { life, clock, roots: &roots, exports: &exports };
        let report = apply(command, env)?;
        Ok((report, commit))
    })
}

fn apply(command: Command, mut env: CacheEnv<'_>) -> Result<String, String> {
    match command {
        Command::Help => Ok(crate::help::text()),
        Command::Config { .. } => Err("cache config writes the persisted policy".to_string()),
        Command::Auto { .. } => Err("cache auto reads the persisted ledger".to_string()),
        Command::Status { json, root } => status(&env, json, root),
        Command::Gc { json, dry_run, root, max_bytes, legacy, yes, select } => {
            if legacy {
                legacy_gc(&mut env, json, dry_run, yes, &select)
            } else {
                sweep(&mut env, json, dry_run, root, max_bytes)
            }
        }
        Command::Purge { json, dry_run, yes, scope } => purge(&mut env, json, dry_run, yes, scope),
    }
}

fn status(env: &CacheEnv<'_>, json: bool, root: Option<String>) -> Result<String, String> {
    let session = host::session_cache(SessionCacheCall::Status)?;
    let mut report = env.life.status();
    if let Some(token) = root {
        let id = resolve(&token, env)?;
        report.roots.retain(|row| row.id == id);
    }
    Ok(text::status(&report, &session, json))
}

fn sweep(env: &mut CacheEnv<'_>, json: bool, dry_run: bool, root: Option<String>, max_bytes: Option<u64>) -> Result<String, String> {
    env.life.observe(env.clock);
    let only = match root {
        Some(token) => Some(resolve(&token, env)?),
        None => None,
    };
    let request = GcRequest {
        root: only,
        budget: SliceBudget::explicit(128),
        bypass_grace: false,
        max_bytes,
        cancel: None,
    };
    let report = if dry_run {
        env.life.plan_gc(request).map_err(|error| error.to_string())?
    } else {
        env.life.gc(request).map_err(|error| error.to_string())?
    };
    let session = host::session_cache(SessionCacheCall::Gc { apply: !dry_run })?;
    Ok(text::gc(&report, dry_run, &session, json))
}

fn purge(env: &mut CacheEnv<'_>, json: bool, dry_run: bool, yes: bool, scope: Scope) -> Result<String, String> {
    let (adapter_scope, label) = match scope {
        Scope::Missing => return Err("purge requires --root PATH or --all".to_string()),
        Scope::Both => return Err("purge accepts either --root PATH or --all".to_string()),
        Scope::All => (PurgeScope::All, "all".to_string()),
        Scope::Root(token) => {
            let id = resolve(&token, env)?;
            (PurgeScope::Root(id), token)
        }
    };
    if !dry_run && !yes {
        return Err("purge requires --yes".to_string());
    }
    let request = PurgeRequest { scope: adapter_scope, confirm: yes || dry_run, dry_run };
    let report = env.life.purge(request).map_err(|error| error.to_string())?;
    let session = host::session_cache(SessionCacheCall::Purge { apply: !dry_run })?;
    Ok(text::purge(&report, &label, &session, json))
}

fn legacy_gc(env: &mut CacheEnv<'_>, json: bool, dry_run: bool, yes: bool, select: &[u64]) -> Result<String, String> {
    let plan = env.life.legacy_dry_run(env.exports);
    let candidate_bytes = plan.candidates.iter().fold(0u64, |sum, item| sum.saturating_add(item.bytes));
    if dry_run {
        let session = host::session_cache(SessionCacheCall::Gc { apply: false })?;
        return Ok(text::legacy(true, plan.candidates.len() as u64, candidate_bytes, plan.left_alone.len() as u64, 0, &session, json));
    }
    if !yes {
        return Err("legacy apply requires --yes".to_string());
    }
    if select.is_empty() {
        return Err("legacy apply requires --select IDENTITY".to_string());
    }
    let applied = env.life.legacy_apply(env.exports, select).map_err(|error| error.to_string())?;
    let reclaimed = applied.reclaimed.iter().fold(0u64, |sum, item| sum.saturating_add(item.bytes));
    let session = host::session_cache(SessionCacheCall::Gc { apply: true })?;
    Ok(text::legacy(
        false,
        plan.candidates.len() as u64,
        candidate_bytes,
        applied.untouched.len() as u64,
        reclaimed,
        &session,
        json,
    ))
}

fn resolve(token: &str, env: &CacheEnv<'_>) -> Result<RootId, String> {
    if let Some(id) = env.roots.paths.get(token) {
        return Ok(*id);
    }
    if let Ok(raw) = token.parse::<u64>() {
        let id = RootId::new(raw);
        if env.life.status().roots.iter().any(|row| row.id == id) {
            return Ok(id);
        }
    }
    Err(format!("root is not registered: {token}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_adapter_cache::{ExportEvidence, FakeClock, KnownExport, OutputPlace, STAGING_TTL_MS};

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    fn env<'a>(life: &'a mut Lifecycle, roots: &'a RootTable, exports: &'a [KnownExport]) -> CacheEnv<'a> {
        CacheEnv { life, clock: FakeClock::at(STAGING_TTL_MS).sample(), roots, exports }
    }

    #[test]
    fn status_reports_the_product_cap_and_facade_bytes() {
        let mut life = Lifecycle::product_default();
        let roots = RootTable::default();
        let text = execute(&args(&["cache", "status"]), env(&mut life, &roots, &[])).expect("status");
        assert!(text.contains(&format!("hard_cap: {}", dctx_adapter_cache::HARD_CAP_BYTES)));
        assert!(text.contains("live_bytes: 0"));
        assert!(text.contains("reclaimable_bytes: 0"));
        assert!(text.contains("pending: 0"));
        assert!(text.contains("session_committed_bytes: 0"));
        let json = execute(&args(&["cache", "status", "--json"]), env(&mut life, &roots, &[])).expect("json");
        assert!(json.contains("\"hard_cap\":"));
        assert!(json.contains("\"session_reclaimed_bytes\":0"));
    }

    #[test]
    fn gc_dry_run_reports_bytes_and_apply_reclaims_them() {
        let mut life = Lifecycle::product_default();
        life.load_staged(1_000, 0).expect("staged");
        life.load_staged(1_000, 0).expect("staged");
        let roots = RootTable::default();
        let before = life.admitted();
        let dry = execute(&args(&["cache", "gc", "--dry-run"]), env(&mut life, &roots, &[])).expect("dry");
        assert!(dry.contains("dry_run: true"));
        assert!(dry.contains("reclaimed_bytes: "));
        assert!(dry.contains("session_reclaimed_bytes: 0"));
        assert_eq!(life.admitted(), before);
        let limited = execute(&args(&["cache", "gc", "--dry-run", "--max-bytes", "1"]), env(&mut life, &roots, &[])).expect("limit");
        assert!(limited.contains("examined: 1"));
        assert_eq!(life.admitted(), before);
        let applied = execute(&args(&["cache", "gc"]), env(&mut life, &roots, &[])).expect("apply");
        assert!(applied.contains("dry_run: false"));
        assert!(applied.contains("reclaimed_bytes: 2000"));
        assert_eq!(life.admitted(), 0);
    }

    #[test]
    fn purge_refuses_a_missing_scope_or_missing_yes_and_apply_drops() {
        let mut life = Lifecycle::product_default();
        let root = RootId::new(7);
        life.register_root(root).expect("root");
        life.publish_pack(root, 40, &[]).expect("pack");
        let mut roots = RootTable::default();
        roots.paths.insert("/work/app".to_string(), root);
        let admitted = life.admitted();
        let revision = life.logical_revision();
        let missing = execute(&args(&["cache", "purge", "--yes"]), env(&mut life, &roots, &[])).expect_err("scope");
        assert!(missing.contains("--root"));
        assert_eq!(life.admitted(), admitted);
        let unconfirmed = execute(&args(&["cache", "purge", "--root", "/work/app"]), env(&mut life, &roots, &[])).expect_err("yes");
        assert!(unconfirmed.contains("--yes"));
        assert_eq!(life.logical_revision(), revision);
        let dry = execute(&args(&["cache", "purge", "--all", "--dry-run"]), env(&mut life, &roots, &[])).expect("dry");
        assert!(dry.contains("dry_run: true"));
        assert!(dry.contains("dropped: 1"));
        assert!(dry.contains("session_reclaimed_bytes: 0"));
        assert_eq!(life.logical_revision(), revision);
        let both = execute(&args(&["cache", "purge", "--all", "--root", "/work/app", "--yes"]), env(&mut life, &roots, &[])).expect_err("both");
        assert!(both.contains("either"));
        let applied = execute(&args(&["cache", "purge", "--root", "/work/app", "--yes"]), env(&mut life, &roots, &[])).expect("apply");
        assert!(applied.contains("dry_run: false"));
        assert!(applied.contains("dropped: 1"));
        assert!(life.logical_revision() > revision);
    }

    #[test]
    fn legacy_dry_run_does_not_scan_or_delete_and_apply_needs_selection() {
        let mut life = Lifecycle::product_default();
        let owned = KnownExport {
            place: OutputPlace::Registry,
            label: "snapshot.txt".into(),
            bytes: 12,
            evidence: ExportEvidence::OwnedText { identity: 4 },
        };
        let extension = KnownExport {
            place: OutputPlace::Registry,
            label: "notes.txt".into(),
            bytes: 9,
            evidence: ExportEvidence::ExtensionOnly,
        };
        life.adopt_legacy(&owned).expect("adopt");
        let roots = RootTable::default();
        let exports = [owned, extension];
        let dry = execute(&args(&["cache", "gc", "--legacy", "--dry-run"]), env(&mut life, &roots, &exports)).expect("dry");
        assert!(dry.contains("home_scan: false"));
        assert!(dry.contains("candidates: 1"));
        assert!(dry.contains("candidate_bytes: 12"));
        assert!(dry.contains("left_alone: 1"));
        assert!(dry.contains("reclaimed_bytes: 0"));
        assert_eq!(life.adopted_labels(), vec!["snapshot.txt".to_string()]);
        let refused = execute(&args(&["cache", "gc", "--legacy"]), env(&mut life, &roots, &exports)).expect_err("yes");
        assert!(refused.contains("--yes"));
        let unselected = execute(&args(&["cache", "gc", "--legacy", "--yes"]), env(&mut life, &roots, &exports)).expect_err("select");
        assert!(unselected.contains("--select"));
        assert_eq!(life.adopted_labels(), vec!["snapshot.txt".to_string()]);
        let applied = execute(
            &args(&["cache", "gc", "--legacy", "--yes", "--select", "4"]),
            env(&mut life, &roots, &exports),
        )
        .expect("apply");
        assert!(applied.contains("reclaimed_bytes: 12"));
        assert!(life.adopted_labels().is_empty());
    }

    #[test]
    fn persisted_dry_run_keeps_bytes_and_apply_reclaims_them() {
        let root = scratch("gc");
        let mut life = Lifecycle::product_default();
        life.load_staged(1_000, 0).expect("staged");
        life.load_staged(1_000, 0).expect("staged");
        host::store_ledger(&root, &life).expect("store");
        let before = std::fs::read(root.join("ledger.v1")).expect("ledger");
        let dry = run_in(&args(&["cache", "gc", "--dry-run"]), Some(&root)).expect("dry");
        assert!(dry.contains("dry_run: true"));
        assert!(dry.contains("reclaimed_bytes: 2000"));
        assert_eq!(std::fs::read(root.join("ledger.v1")).expect("ledger"), before);
        assert_eq!(host::load_ledger(&root).expect("load").admitted(), 2_000);
        let applied = run_in(&args(&["cache", "gc"]), Some(&root)).expect("apply");
        assert!(applied.contains("dry_run: false"));
        assert!(applied.contains("reclaimed_bytes: 2000"));
        assert_eq!(host::load_ledger(&root).expect("load").admitted(), 0);
    }

    #[test]
    fn persisted_purge_dry_run_keeps_the_ledger() {
        let root = scratch("purge");
        let mut life = Lifecycle::product_default();
        let id = RootId::new(7);
        life.register_root(id).expect("root");
        life.publish_pack(id, 40, &[]).expect("pack");
        host::store_ledger(&root, &life).expect("store");
        let before = std::fs::read(root.join("ledger.v1")).expect("ledger");
        let dry = run_in(&args(&["cache", "purge", "--all", "--dry-run"]), Some(&root)).expect("dry");
        assert!(dry.contains("dry_run: true"));
        assert!(dry.contains("dropped: 1"));
        assert_eq!(std::fs::read(root.join("ledger.v1")).expect("ledger"), before);
        let applied = run_in(&args(&["cache", "purge", "--all", "--yes"]), Some(&root)).expect("apply");
        assert!(applied.contains("dry_run: false"));
        assert!(applied.contains("dropped: 1"));
        assert_ne!(std::fs::read(root.join("ledger.v1")).expect("ledger"), before);
    }

    #[test]
    fn config_persists_across_two_shows_and_a_corrupt_ledger_is_named() {
        let root = scratch("config");
        let set = run_in(
            &args(&["cache", "config", "auto", "off", "--max-bytes", "1000000", "--ttl-days", "3"]),
            Some(&root),
        )
        .expect("set");
        assert!(set.contains("auto: off"));
        assert!(set.contains("max_bytes: 1000000"));
        assert!(set.contains("ttl_days: 3"));
        let first = run_in(&args(&["cache", "config"]), Some(&root)).expect("first");
        let second = run_in(&args(&["cache", "config"]), Some(&root)).expect("second");
        assert_eq!(first, set);
        assert_eq!(second, first);
        let raised = run_in(&args(&["cache", "config", "--max-bytes", "2000000"]), Some(&root)).expect_err("raise");
        assert!(raised.contains("only tighten"));
        std::fs::write(root.join("ledger.v1"), b"not-a-ledger").expect("corrupt");
        let err = run_in(&args(&["cache", "status"]), Some(&root)).expect_err("status");
        assert!(err.contains("corrupt ledger"), "{err}");
        assert_eq!(std::fs::read(root.join("ledger.v1")).expect("kept"), b"not-a-ledger");
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("dctx-cli-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("dir");
        path
    }
}
