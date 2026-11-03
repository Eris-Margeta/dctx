// SPDX-License-Identifier: Apache-2.0
//! Argument rules for `dctx cache status|gc|purge|config|auto`.
//! Implements ARCHITECTURE-PROPOSAL §4 public cache commands.
//! Depends on nothing but the argument strings. [`super::execute`] applies the parsed command.
//! Purge is invalid until the caller has `--root` or `--all`. `--yes` is checked later for apply.

/// One cache invocation after the `cache` word.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Help was requested.
    Help,
    /// Point-in-time buckets.
    Status {
        /// JSON projection.
        json: bool,
        /// Optional root path or numeric id.
        root: Option<String>,
    },
    /// Retention sweep, or a legacy export inspection when `legacy` is set.
    Gc {
        /// JSON projection.
        json: bool,
        /// Report without unlinking.
        dry_run: bool,
        /// Optional root path or numeric id.
        root: Option<String>,
        /// Stop after this many reclaimed bytes.
        max_bytes: Option<u64>,
        /// Inspect registered legacy exports instead of sweeping segments.
        legacy: bool,
        /// Consent for a legacy apply.
        yes: bool,
        /// Ownership identities selected for a legacy apply.
        select: Vec<u64>,
    },
    /// Drop managed references for one root or every root.
    Purge {
        /// JSON projection.
        json: bool,
        /// Report without dropping references.
        dry_run: bool,
        /// Consent for an apply.
        yes: bool,
        /// Which scope flags were present.
        scope: Scope,
    },
    /// Show or update the persisted auto-clean policy.
    Config {
        /// JSON projection.
        json: bool,
        /// `Some` sets auto on or off. `None` leaves the stored flag.
        auto: Option<bool>,
        /// Tighten the system cap. `None` leaves it.
        max_bytes: Option<u64>,
        /// Idle overlay and preview lifetime in days. `None` leaves it.
        ttl_days: Option<u64>,
    },
    /// Last auto-clean slice, bytes reclaimed and the next due time.
    Auto {
        /// JSON projection.
        json: bool,
    },
}

/// Root selection before it is resolved to a ledger id.
#[derive(Debug, PartialEq, Eq)]
pub enum Scope {
    /// Neither `--root` nor `--all`.
    Missing,
    /// `--all`.
    All,
    /// `--root PATH`.
    Root(String),
    /// Both selectors. This is refused.
    Both,
}

/// Parses arguments that start with `cache`.
pub fn parse(args: &[String]) -> Result<Command, String> {
    if args.first().map(String::as_str) != Some("cache") {
        return Err("expected cache".to_string());
    }
    if args.len() == 1 || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(Command::Help);
    }
    match args[1].as_str() {
        "status" => status(&args[2..]),
        "gc" => gc(&args[2..]),
        "purge" => purge(&args[2..]),
        "config" => config(&args[2..]),
        "auto" => auto_cmd(&args[2..]),
        other => Err(format!("unknown cache command: {other}. Use status, gc, purge, config or auto")),
    }
}

fn status(args: &[String]) -> Result<Command, String> {
    let mut json = false;
    let mut root = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => json = flag_once(json, "--json")?,
            "--root" => root = Some(value(args, &mut index, "--root")?),
            other => return Err(format!("unknown status flag: {other}")),
        }
        index += 1;
    }
    Ok(Command::Status { json, root })
}

fn gc(args: &[String]) -> Result<Command, String> {
    let mut json = false;
    let mut dry_run = false;
    let mut root = None;
    let mut max_bytes = None;
    let mut legacy = false;
    let mut yes = false;
    let mut select = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => json = flag_once(json, "--json")?,
            "--dry-run" => dry_run = flag_once(dry_run, "--dry-run")?,
            "--legacy" => legacy = flag_once(legacy, "--legacy")?,
            "--yes" => yes = flag_once(yes, "--yes")?,
            "--root" => root = Some(value(args, &mut index, "--root")?),
            "--max-bytes" => {
                if max_bytes.is_some() {
                    return Err("--max-bytes was already set".to_string());
                }
                let raw = value(args, &mut index, "--max-bytes")?;
                let parsed: u64 = raw.parse().map_err(|_| format!("invalid --max-bytes: {raw}"))?;
                if parsed == 0 {
                    return Err("--max-bytes must be at least 1".to_string());
                }
                max_bytes = Some(parsed);
            }
            "--select" => {
                let raw = value(args, &mut index, "--select")?;
                let parsed: u64 = raw.parse().map_err(|_| format!("invalid --select: {raw}"))?;
                select.push(parsed);
            }
            other => return Err(format!("unknown gc flag: {other}")),
        }
        index += 1;
    }
    Ok(Command::Gc { json, dry_run, root, max_bytes, legacy, yes, select })
}

fn purge(args: &[String]) -> Result<Command, String> {
    let mut json = false;
    let mut dry_run = false;
    let mut yes = false;
    let mut all = false;
    let mut root = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => json = flag_once(json, "--json")?,
            "--dry-run" => dry_run = flag_once(dry_run, "--dry-run")?,
            "--yes" => yes = flag_once(yes, "--yes")?,
            "--all" => all = flag_once(all, "--all")?,
            "--root" => root = Some(value(args, &mut index, "--root")?),
            other => return Err(format!("unknown purge flag: {other}")),
        }
        index += 1;
    }
    let scope = match (all, root) {
        (false, None) => Scope::Missing,
        (true, None) => Scope::All,
        (false, Some(path)) => Scope::Root(path),
        (true, Some(_)) => Scope::Both,
    };
    Ok(Command::Purge { json, dry_run, yes, scope })
}

fn config(args: &[String]) -> Result<Command, String> {
    let mut json = false;
    let mut auto = None;
    let mut max_bytes = None;
    let mut ttl_days = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => json = flag_once(json, "--json")?,
            "auto" => {
                if auto.is_some() {
                    return Err("auto was already set".to_string());
                }
                let raw = value(args, &mut index, "auto")?;
                auto = Some(match raw.as_str() {
                    "on" => true,
                    "off" => false,
                    _ => return Err("auto expects on or off".to_string()),
                });
            }
            "--max-bytes" => max_bytes = Some(positive(args, &mut index, "--max-bytes", max_bytes)?),
            "--ttl-days" => ttl_days = Some(positive(args, &mut index, "--ttl-days", ttl_days)?),
            other => return Err(format!("unknown config flag: {other}")),
        }
        index += 1;
    }
    Ok(Command::Config { json, auto, max_bytes, ttl_days })
}

fn auto_cmd(args: &[String]) -> Result<Command, String> {
    let mut json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => json = flag_once(json, "--json")?,
            other => return Err(format!("unknown auto flag: {other}")),
        }
    }
    Ok(Command::Auto { json })
}

fn positive(args: &[String], index: &mut usize, name: &str, already: Option<u64>) -> Result<u64, String> {
    if already.is_some() {
        return Err(format!("{name} was already set"));
    }
    let raw = value(args, index, name)?;
    let parsed: u64 = raw.parse().map_err(|_| format!("invalid {name}: {raw}"))?;
    if parsed == 0 {
        return Err(format!("{name} must be at least 1"));
    }
    Ok(parsed)
}

fn flag_once(already: bool, name: &str) -> Result<bool, String> {
    if already {
        Err(format!("{name} was already set"))
    } else {
        Ok(true)
    }
}

fn value(args: &[String], index: &mut usize, name: &str) -> Result<String, String> {
    *index += 1;
    let value = args.get(*index).ok_or_else(|| format!("{name} requires a value"))?;
    if value.starts_with('-') {
        return Err(format!("{name} requires a value"));
    }
    Ok(value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_slice(args: &[&str]) -> Result<Command, String> {
        parse(&args.iter().map(|arg| (*arg).to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn purge_requires_a_scope_token_and_gc_rejects_zero_budget() {
        let missing = parse_slice(&["cache", "purge", "--yes"]).expect("parse");
        assert!(matches!(missing, Command::Purge { scope: Scope::Missing, .. }));
        let both = parse_slice(&["cache", "purge", "--all", "--root", "/work"]).expect("parse");
        assert!(matches!(both, Command::Purge { scope: Scope::Both, .. }));
        let rooted = parse_slice(&["cache", "purge", "--root", "/work", "--dry-run"]).expect("parse");
        assert!(matches!(rooted, Command::Purge { dry_run: true, scope: Scope::Root(path), .. } if path == "/work"));
        assert!(parse_slice(&["cache", "gc", "--max-bytes", "0"]).is_err());
        assert!(parse_slice(&["cache", "status", "--json"]).is_ok());
        let config = parse_slice(&["cache", "config", "auto", "off", "--max-bytes", "1000", "--ttl-days", "3"]).expect("config");
        assert!(matches!(
            config,
            Command::Config { auto: Some(false), max_bytes: Some(1000), ttl_days: Some(3), json: false }
        ));
        assert!(matches!(parse_slice(&["cache", "config"]).expect("show"), Command::Config { auto: None, .. }));
        assert!(matches!(parse_slice(&["cache", "auto", "--json"]).expect("auto"), Command::Auto { json: true }));
        assert!(parse_slice(&["cache", "config", "--ttl-days", "0"]).is_err());
        assert!(parse_slice(&["cache", "config", "auto", "maybe"]).is_err());
        assert!(parse_slice(&["cache", "tune"]).is_err());
    }
}
