// SPDX-License-Identifier: Apache-2.0
//! `dctx cache status|gc|purge|config|auto` over the persisted facade ledger.
//! Implements ARCHITECTURE-PROPOSAL §4 and §7 row DX-D2.
//! Depends on `dctx-adapter-cache` for quotas and on [`crate::host`] for the cache root and facade auto-clean.
//! Purge apply requires `--root` or `--all`, plus `--yes`. Dry-run changes no bytes.

mod parse;
mod text;

use std::collections::BTreeMap;
use std::path::Path;

use dctx_adapter_cache::{
    ClockSample, GcRequest, Lifecycle, PurgeRequest, PurgeScope, RootId, SliceBudget,
};

use crate::exit::Failure;
use crate::host::{self, SessionCacheCall};
use parse::{Command, Scope};

/// Path or label to a registered root. The ledger never invents a root from a directory walk.
#[derive(Debug, Default, Clone)]
pub struct RootTable {
    paths: BTreeMap<String, RootId>,
}

/// Managed ledger and registered roots supplied by the host.
#[derive(Debug)]
pub struct CacheEnv<'a> {
    /// Managed ledger for this process.
    pub life: &'a mut Lifecycle,
    /// Clock sample applied before a sweep so expiry can be tested.
    pub clock: ClockSample,
    /// Registered root tokens.
    pub roots: &'a RootTable,
}

/// Hint for a wrong cache argument.
const HELP_HINT: &str = "run `dctx cache --help` for cache commands and flags";

/// Runs a cache command against the persisted ledger and prints the report.
/// Failures follow docs/CLI-EXIT-CODES.md: 2 for arguments, 1 for an unregistered root, 3 for cache I/O.
/// # Errors
/// Returns command parsing, cache policy, ledger, maintenance, or filesystem failures with their exit code and hint.
pub fn run(args: &[String]) -> Result<(), Failure> {
    let report = output_in(args, None)?;
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
/// # Errors
/// Returns command parsing, root/policy validation, ledger, maintenance, or filesystem failures as display text.
pub fn run_in(args: &[String], root: Option<&Path>) -> Result<String, String> {
    output_in(args, root).map_err(|failure| failure.message)
}

/// Same as [`run_in`], keeping the exit code and hint of each failure.
/// # Errors
/// Usage failures (exit 2), an unregistered `--root` (exit 1), and cache directory, policy or ledger failures (exit 3).
pub fn output_in(args: &[String], root: Option<&Path>) -> Result<String, Failure> {
    let command = parse::parse(args).map_err(|message| Failure::usage(message, HELP_HINT))?;
    if matches!(command, Command::Help) {
        return Ok(crate::help::text());
    }
    let resolved = match root {
        Some(path) => path.to_path_buf(),
        None => host::cache_root_dir().map_err(|message| {
            Failure::index(
                format!("cannot locate the cache directory: {message}"),
                "set DCTX_CACHE_DIR to a writable directory",
            )
        })?,
    };
    host::ensure_cache_dir(&resolved).map_err(|message| io_failure(&resolved, &message))?;
    match command {
        Command::Help => Ok(crate::help::text()),
        Command::Config {
            json,
            auto,
            max_bytes,
            ttl_days,
        } => {
            let policy = host::configure(&resolved, auto, max_bytes, ttl_days)
                .map_err(|message| config_failure(&resolved, message))?;
            Ok(text::config_text(&policy, json))
        }
        Command::Auto { json } => Ok(text::auto_text(
            &host::auto_report(&resolved).map_err(|message| io_failure(&resolved, &message))?,
            json,
        )),
        other => persisted(other, &resolved),
    }
}

/// Exit 3 for the cache directory, its policy or its ledger, naming the directory.
fn io_failure(root: &Path, message: &str) -> Failure {
    let hint = if message.contains("corrupt ledger") {
        format!(
            "the cache ledger cannot be read; remove the cache directory {} to start an empty cache",
            root.display()
        )
    } else {
        "check that the cache directory exists and is writable, or set DCTX_CACHE_DIR to another directory"
            .to_string()
    };
    Failure::index(
        format!("cache directory {}: {message}", root.display()),
        hint,
    )
}

/// `--max-bytes` that loosens the cap is a usage error; every other policy failure is cache I/O.
fn config_failure(root: &Path, message: String) -> Failure {
    if message.contains("only tighten") || message.contains("cannot exceed") {
        Failure::usage(
            message,
            "pass a --max-bytes no larger than the max_bytes shown by `dctx cache config`",
        )
    } else {
        io_failure(root, &message)
    }
}

/// Captures cache status for RPC with the same exit codes, messages and hints as `dctx cache status`.
/// # Errors
/// Returns the CLI failure of [`output_in`], prefixed `cache:`; a non-status command is a usage error.
pub fn status_output(args: &[String]) -> Result<crate::exit::CommandOutput, Failure> {
    let command =
        parse::parse(args).map_err(|message| Failure::usage(message, HELP_HINT).within("cache"))?;
    if !matches!(command, Command::Status { .. } | Command::Help) {
        return Err(Failure::usage("expected cache status", "use cache_status"));
    }
    let mut stdout = output_in(args, None).map_err(|failure| failure.within("cache"))?;
    if !stdout.is_empty() && !stdout.ends_with('\n') {
        stdout.push('\n');
    }
    Ok(crate::exit::CommandOutput {
        stdout,
        failure: None,
    })
}

/// Applies `args` to an injected ledger. Persistence tests use [`run_in`].
#[cfg(test)]
fn execute(args: &[String], env: CacheEnv<'_>) -> Result<String, String> {
    apply(parse::parse(args)?, env).map_err(|failure| failure.message)
}

fn persisted(command: Command, root: &Path) -> Result<String, Failure> {
    let commit = matches!(
        command,
        Command::Gc { dry_run: false, .. } | Command::Purge { dry_run: false, .. }
    );
    let mut refused = None;
    let report = host::with_ledger(root, |life| {
        let roots = RootTable::default();
        let now = host::unix_millis();
        let clock = ClockSample {
            wall: dctx::Timestamp(now),
            monotonic_ms: now,
            boot_id: 1,
        };
        let env = CacheEnv {
            life,
            clock,
            roots: &roots,
        };
        match apply(command, env) {
            Ok(report) => Ok((report, commit)),
            Err(failure) => {
                let message = failure.message.clone();
                refused = Some(failure);
                Err(message)
            }
        }
    });
    match (report, refused) {
        (Ok(report), _) => Ok(report),
        (Err(_), Some(failure)) => Err(failure),
        (Err(message), None) => Err(io_failure(root, &message)),
    }
}

fn apply(command: Command, mut env: CacheEnv<'_>) -> Result<String, Failure> {
    match command {
        Command::Help => Ok(crate::help::text()),
        Command::Config { .. } => Err(Failure::index(
            "cache config writes the persisted policy",
            HELP_HINT,
        )),
        Command::Auto { .. } => Err(Failure::index(
            "cache auto reads the persisted ledger",
            HELP_HINT,
        )),
        Command::Status { json, root } => status(&env, json, root),
        Command::Gc {
            json,
            dry_run,
            root,
            max_bytes,
        } => sweep(&mut env, json, dry_run, root, max_bytes),
        Command::Purge {
            json,
            dry_run,
            yes,
            scope,
        } => purge(&mut env, json, dry_run, yes, scope),
    }
}

/// Exit 3 for a ledger maintenance or session cache failure.
fn maintenance(message: impl Into<String>) -> Failure {
    Failure::index(
        message,
        "another dctx process may hold the cache; run the command again, or check that the cache directory is writable",
    )
}

fn status(env: &CacheEnv<'_>, json: bool, root: Option<String>) -> Result<String, Failure> {
    let session = host::session_cache(SessionCacheCall::Status).map_err(maintenance)?;
    let mut report = env.life.status();
    if let Some(token) = root {
        let id = resolve(&token, env)?;
        report.roots.retain(|row| row.id == id);
    }
    Ok(text::status(&report, &session, json))
}

fn sweep(
    env: &mut CacheEnv<'_>,
    json: bool,
    dry_run: bool,
    root: Option<String>,
    max_bytes: Option<u64>,
) -> Result<String, Failure> {
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
        env.life
            .plan_gc(request)
            .map_err(|error| maintenance(error.to_string()))?
    } else {
        env.life
            .gc(request)
            .map_err(|error| maintenance(error.to_string()))?
    };
    let session =
        host::session_cache(SessionCacheCall::Gc { apply: !dry_run }).map_err(maintenance)?;
    Ok(text::gc(&report, dry_run, &session, json))
}

fn purge(
    env: &mut CacheEnv<'_>,
    json: bool,
    dry_run: bool,
    yes: bool,
    scope: Scope,
) -> Result<String, Failure> {
    let (adapter_scope, label) = match scope {
        Scope::Missing => {
            return Err(Failure::usage(
                "purge requires --root PATH or --all",
                "name the scope, as `dctx cache purge --all --dry-run`",
            ))
        }
        Scope::Both => {
            return Err(Failure::usage(
                "purge accepts either --root PATH or --all",
                "pass only one of --root PATH and --all",
            ))
        }
        Scope::All => (PurgeScope::All, "all".to_string()),
        Scope::Root(token) => {
            let id = resolve(&token, env)?;
            (PurgeScope::Root(id), token)
        }
    };
    if !dry_run && !yes {
        return Err(Failure::usage(
            "purge requires --yes",
            "preview with --dry-run, then run the same command with --yes to drop the entries",
        ));
    }
    let request = PurgeRequest {
        scope: adapter_scope,
        confirm: yes || dry_run,
        dry_run,
    };
    let report = env
        .life
        .purge(request)
        .map_err(|error| maintenance(error.to_string()))?;
    let session =
        host::session_cache(SessionCacheCall::Purge { apply: !dry_run }).map_err(maintenance)?;
    Ok(text::purge(&report, &label, &session, json))
}

/// A `--root` token the ledger does not hold is a named target with no answer: exit 1.
fn resolve(token: &str, env: &CacheEnv<'_>) -> Result<RootId, Failure> {
    if let Some(id) = env.roots.paths.get(token) {
        return Ok(*id);
    }
    if let Ok(raw) = token.parse::<u64>() {
        let id = RootId::new(raw);
        if env.life.status().roots.iter().any(|row| row.id == id) {
            return Ok(id);
        }
    }
    Err(Failure::no_result(
        format!("root is not registered: {token}"),
        "pass a root id from the `root` lines of `dctx cache status`, or use --all",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_adapter_cache::{FakeClock, STAGING_TTL_MS};

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    fn env<'a>(life: &'a mut Lifecycle, roots: &'a RootTable) -> CacheEnv<'a> {
        CacheEnv {
            life,
            clock: FakeClock::at(STAGING_TTL_MS).sample(),
            roots,
        }
    }

    #[test]
    fn status_reports_the_product_cap_and_facade_bytes() {
        let mut life = Lifecycle::product_default();
        let roots = RootTable::default();
        let text = execute(&args(&["cache", "status"]), env(&mut life, &roots)).expect("status");
        assert!(text.contains(&format!("hard_cap: {}", dctx_adapter_cache::HARD_CAP_BYTES)));
        assert!(text.contains("live_bytes: 0"));
        assert!(text.contains("reclaimable_bytes: 0"));
        assert!(text.contains("pending: 0"));
        assert!(text.contains("session_committed_bytes: 0"));
        let json = execute(
            &args(&["cache", "status", "--json"]),
            env(&mut life, &roots),
        )
        .expect("json");
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
        let dry =
            execute(&args(&["cache", "gc", "--dry-run"]), env(&mut life, &roots)).expect("dry");
        assert!(dry.contains("dry_run: true"));
        assert!(dry.contains("reclaimed_bytes: "));
        assert!(dry.contains("session_reclaimed_bytes: 0"));
        assert_eq!(life.admitted(), before);
        let limited = execute(
            &args(&["cache", "gc", "--dry-run", "--max-bytes", "1"]),
            env(&mut life, &roots),
        )
        .expect("limit");
        assert!(limited.contains("examined: 1"));
        assert_eq!(life.admitted(), before);
        let applied = execute(&args(&["cache", "gc"]), env(&mut life, &roots)).expect("apply");
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
        let missing = execute(&args(&["cache", "purge", "--yes"]), env(&mut life, &roots))
            .expect_err("scope");
        assert!(missing.contains("--root"));
        assert_eq!(life.admitted(), admitted);
        let unconfirmed = execute(
            &args(&["cache", "purge", "--root", "/work/app"]),
            env(&mut life, &roots),
        )
        .expect_err("yes");
        assert!(unconfirmed.contains("--yes"));
        assert_eq!(life.logical_revision(), revision);
        let dry = execute(
            &args(&["cache", "purge", "--all", "--dry-run"]),
            env(&mut life, &roots),
        )
        .expect("dry");
        assert!(dry.contains("dry_run: true"));
        assert!(dry.contains("dropped: 1"));
        assert!(dry.contains("session_reclaimed_bytes: 0"));
        assert_eq!(life.logical_revision(), revision);
        let both = execute(
            &args(&["cache", "purge", "--all", "--root", "/work/app", "--yes"]),
            env(&mut life, &roots),
        )
        .expect_err("both");
        assert!(both.contains("either"));
        let applied = execute(
            &args(&["cache", "purge", "--root", "/work/app", "--yes"]),
            env(&mut life, &roots),
        )
        .expect("apply");
        assert!(applied.contains("dry_run: false"));
        assert!(applied.contains("dropped: 1"));
        assert!(life.logical_revision() > revision);
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
        assert_eq!(
            std::fs::read(root.join("ledger.v1")).expect("ledger"),
            before
        );
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
        let dry = run_in(
            &args(&["cache", "purge", "--all", "--dry-run"]),
            Some(&root),
        )
        .expect("dry");
        assert!(dry.contains("dry_run: true"));
        assert!(dry.contains("dropped: 1"));
        assert_eq!(
            std::fs::read(root.join("ledger.v1")).expect("ledger"),
            before
        );
        let applied =
            run_in(&args(&["cache", "purge", "--all", "--yes"]), Some(&root)).expect("apply");
        assert!(applied.contains("dry_run: false"));
        assert!(applied.contains("dropped: 1"));
        assert_ne!(
            std::fs::read(root.join("ledger.v1")).expect("ledger"),
            before
        );
    }

    #[test]
    fn config_persists_across_two_shows_and_a_corrupt_ledger_is_named() {
        let root = scratch("config");
        let set = run_in(
            &args(&[
                "cache",
                "config",
                "auto",
                "off",
                "--max-bytes",
                "1000000",
                "--ttl-days",
                "3",
            ]),
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
        let raised = run_in(
            &args(&["cache", "config", "--max-bytes", "2000000"]),
            Some(&root),
        )
        .expect_err("raise");
        assert!(raised.contains("only tighten"));
        std::fs::write(root.join("ledger.v1"), b"not-a-ledger").expect("corrupt");
        let err = run_in(&args(&["cache", "status"]), Some(&root)).expect_err("status");
        assert!(err.contains("corrupt ledger"), "{err}");
        assert_eq!(
            std::fs::read(root.join("ledger.v1")).expect("kept"),
            b"not-a-ledger"
        );
    }

    #[test]
    fn failures_carry_contract_codes_hints_and_the_cache_path() {
        let root = scratch("codes");
        let code = |parts: &[&str]| output_in(&args(parts), Some(&root)).expect_err("refused");
        for parts in [
            &["cache", "purge"][..],
            &["cache", "purge", "--all"],
            &["cache", "purge", "--all", "--root", "7", "--yes"],
            &["cache", "frobnicate"],
            &["cache", "gc", "--max-bytes", "0"],
            &["cache", "config", "--max-bytes", "999999999999"],
        ] {
            let failure = code(parts);
            assert_eq!(failure.code, crate::exit::USAGE, "{parts:?}: {failure:?}");
            assert!(!failure.hint.is_empty(), "{parts:?}");
        }
        let unknown = code(&["cache", "status", "--root", "424242"]);
        assert_eq!(unknown.code, crate::exit::NO_RESULT);
        assert!(unknown.hint.contains("dctx cache status"));
        std::fs::write(root.join("ledger.v1"), b"not-a-ledger").expect("corrupt");
        let corrupt = code(&["cache", "status"]);
        assert_eq!(corrupt.code, crate::exit::INDEX);
        assert!(
            corrupt.message.contains(&root.display().to_string()),
            "{}",
            corrupt.message
        );
        assert!(!corrupt.hint.is_empty());
        let file = root.join("not-a-dir");
        std::fs::write(&file, b"x").expect("file");
        let blocked = output_in(&args(&["cache", "status"]), Some(&file)).expect_err("file");
        assert_eq!(blocked.code, crate::exit::INDEX);
        assert!(blocked.message.contains(&file.display().to_string()));
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("dctx-cli-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("dir");
        path
    }
}
