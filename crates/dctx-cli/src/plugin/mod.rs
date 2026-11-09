// SPDX-License-Identifier: Apache-2.0
//! Plugin management commands and the same-binary worker switch.
//! Implements PLUGINS.md §5.1 and §7.3 row PL-CLI: every management command goes through
//! `dctx-plugin-distribution`, and the bundled git catalog comes from the dctx facade.
//! `list` and `help` do not lock the registry, fetch, or start a worker. Used by `main`.

mod bundled;
pub(crate) mod disk;
mod fetch;
mod ops;
mod ops_catalog;
mod ops_enablement;
mod ops_install;
mod ops_output;
mod ops_run;
mod parse;
mod render;
mod worker;

use dctx::PluginHost;
use dctx_plugin_distribution::{Clock, InstallPolicy, Network, Store, TrustStore};

pub use worker::enter as enter_worker;

use ops::Outcome;
use parse::Action;

/// A host with no compatible artifact. The payload preserves OS, CPU and environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UnsupportedTarget {
    /// No artifacts for this operating system.
    Os(String),
    /// No artifacts for this CPU on the selected operating system.
    Cpu(String),
    /// No compatible libc or ABI for an otherwise supported OS and CPU.
    Libc(String),
}

impl std::fmt::Display for UnsupportedTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (Self::Os(platform) | Self::Cpu(platform) | Self::Libc(platform)) = self;
        write!(f, "unsupported platform: {platform}")
    }
}

impl std::error::Error for UnsupportedTarget {}

/// Cached signed discovery index. Search reads it; only `--refresh` replaces it.
#[derive(Clone, Debug, Default)]
pub struct IndexCache {
    /// Exact index bytes, when a verified copy is cached.
    pub bytes: Option<Vec<u8>>,
    /// Detached signature over `bytes`.
    pub signature: Option<Vec<u8>>,
    /// Unix time the cache was stored.
    pub fetched_at: Option<u64>,
    /// Verified index sequence, used to refuse a downgrade on refresh.
    pub sequence: Option<u64>,
}

/// Ports for one management command. The facade host is observed and never launched here.
pub struct Env<'a> {
    /// Installer HTTPS port. The product binary has no TLS adapter.
    pub network: &'a dyn Network,
    /// Registry and version directories.
    pub store: &'a dyn Store,
    /// Injected clock.
    pub clock: &'a dyn Clock,
    /// Pinned publisher keys.
    pub trust: &'a TrustStore,
    /// Target and origin policy.
    pub policy: &'a InstallPolicy,
    /// Discovery cache. Not the execution registry.
    pub index: &'a mut IndexCache,
    /// Index document URL. Signature is the sibling `index.json.sig` when the path ends in `index.json`.
    pub index_url: &'a str,
    /// Pinned index signing key. Absent in the product binary until a trust file is supplied.
    pub index_key: Option<[u8; 32]>,
    /// Facade plugin host. Management reads its spawn counters and does not admit hooks.
    pub host: &'a PluginHost,
}

impl<'a> Env<'a> {
    /// Product wiring: filesystem root, no TLS, empty trust, host target, inert facade host.
    pub fn product(
        store: &'a dyn Store,
        clock: &'a dyn Clock,
        trust: &'a TrustStore,
        policy: &'a InstallPolicy,
        index: &'a mut IndexCache,
        network: &'a dyn Network,
        host: &'a PluginHost,
    ) -> Self {
        Self {
            network,
            store,
            clock,
            trust,
            policy,
            index,
            index_url: bundled::DEFAULT_INDEX_URL,
            index_key: None,
            host,
        }
    }
}

/// True when `arg` is claimed before snapshot, map, pack, cache or template.
pub fn is_early(arg: &str) -> bool {
    matches!(arg, "plugin" | "git" | "--internal-plugin-worker")
}

/// Runs a plugin or `git` alias command. Help and list do not create `root`.
/// Without a resolvable plugin root (no `DCTX_PLUGIN_ROOT` and no usable home), only help runs.
pub fn enter(args: &[String]) -> u8 {
    match disk::default_root() {
        Ok(root) => enter_at(args, &root),
        Err(reason) => {
            let outcome = match parse::parse_argv(args) {
                Ok(request) => (request.json, no_root(args, &request, &reason)),
                Err(error) => return parse_failure(&error),
            };
            print_body(outcome.0, &outcome.1);
            outcome.1.code
        }
    }
}

fn no_root(args: &[String], request: &parse::Request, reason: &str) -> Outcome {
    if matches!(request.action, Action::Help) {
        return help_outcome(&render::help_text(), request.json);
    }
    let command = command_name(args);
    let body = if request.json {
        format!(
            "{{\"ok\":false,\"command\":{},\"error\":\"NoPluginRoot\",\"detail\":{}}}",
            render::json_string(command),
            render::json_string(reason)
        )
    } else {
        ops_output::failure_text(
            command,
            reason,
            "set HOME (or DCTX_PLUGIN_ROOT) to an absolute directory",
        )
    };
    Outcome { code: 1, body }
}

/// Commands that write the plugin store: refused up front where the store is read-only, so they
/// never surface as a store failure.
fn changes_store(action: &Action) -> bool {
    match action {
        Action::Install(_)
        | Action::Enable { .. }
        | Action::Disable { .. }
        | Action::Remove { .. } => true,
        Action::Update { check, .. } => !check,
        _ => false,
    }
}

fn command_name(args: &[String]) -> &str {
    if args.get(1).is_some_and(|arg| arg == "plugin") {
        args.get(2).map_or("plugin", String::as_str)
    } else {
        "git"
    }
}

fn parse_failure(error: &str) -> u8 {
    eprintln!(
        "{}",
        ops_output::failure_text(
            "plugin",
            error,
            "run `dctx plugin help` for the command shapes"
        )
    );
    2
}

/// Same as [`enter`] with an explicit plugin root. A missing root is left missing for read-only commands.
pub fn enter_at(args: &[String], root: &std::path::Path) -> u8 {
    match outcome_at(args, root, disk::host_target) {
        Ok((json, outcome)) => {
            print_body(json, &outcome);
            outcome.code
        }
        Err(error) => parse_failure(&error),
    }
}

/// Resolves the target lazily, after commands that only read metadata.
pub(crate) fn outcome_at(
    args: &[String],
    root: &std::path::Path,
    target: impl FnOnce() -> Result<dctx_plugin_distribution::Target, UnsupportedTarget>,
) -> Result<(bool, Outcome), String> {
    let request = parse::parse_argv(args)?;
    if matches!(request.action, Action::Help) {
        return Ok((
            request.json,
            help_outcome(&render::help_text(), request.json),
        ));
    }
    if matches!(request.action, Action::List) {
        return Ok((request.json, list_at(root, request.json)));
    }
    let target = match target() {
        Ok(target) => target,
        Err(error) => {
            let command = command_name(args);
            let body = if request.json {
                format!(
                    "{{\"ok\":false,\"command\":{},\"error\":\"UnsupportedTarget\",\"detail\":{}}}",
                    render::json_string(command),
                    render::json_string(&error.to_string())
                )
            } else {
                ops_output::failure_text(
                    command,
                    &format!("{error} (UnsupportedTarget)"),
                    "plugins can be installed on macOS (arm64, x86_64) and Linux glibc (arm64, x86_64)",
                )
            };
            return Ok((request.json, Outcome { code: 1, body }));
        }
    };
    if !disk::STORE_WRITABLE && changes_store(&request.action) {
        return Ok((
            request.json,
            ops_output::read_only_store(command_name(args), request.json),
        ));
    }
    let store = disk::FsStore::new(root);
    let clock = disk::SystemClock;
    let trust = TrustStore::new();
    let policy = InstallPolicy::new(target);
    let network = disk::NoTls;
    let host = PluginHost::platform();
    let mut index = IndexCache::default();
    let mut env = Env::product(&store, &clock, &trust, &policy, &mut index, &network, &host);
    Ok((request.json, ops::execute(&request, &mut env)))
}

// Distribution's list API requires an install policy. Read its registry codec directly
// so listing has no artifact target, including on hosts that cannot install anything.
fn list_at(root: &std::path::Path, json: bool) -> Outcome {
    use dctx_plugin_distribution::{Enablement, Origin, PluginStatus, Registry};
    let store = disk::FsStore::new(root);
    let trust = TrustStore::new();
    let now = disk::SystemClock.now();
    let rows = (|| {
        let registry = match store.load_registry()? {
            Some(bytes) => dctx_plugin_distribution::domain::registry_codec::decode(&bytes)?,
            None => Registry::default(),
        };
        let mut rows = registry
            .records()
            .map(|(id, _)| registry.status(id, &trust, now))
            .collect::<Result<Vec<_>, _>>()?;
        if let (Ok(id), Ok(version)) = (bundled::id(), bundled::installed_version()) {
            if !rows.iter().any(|row| row.id == id) {
                rows.push(PluginStatus {
                    id,
                    origin: Origin::Bundled,
                    version: Some(version.version),
                    enablement: if bundled::worker_linked() {
                        Enablement::Enabled
                    } else {
                        Enablement::Disabled
                    },
                    needs_consent: bundled::worker_linked(),
                    blocked: !bundled::worker_linked(),
                    revocation_age_secs: trust.revocation_age(now),
                });
            }
        }
        Ok::<_, dctx_plugin_distribution::DistributionError>(rows)
    })();
    let rows = match rows {
        Ok(rows) => rows,
        Err(error) => {
            let name = match error {
                dctx_plugin_distribution::DistributionError::Malformed { .. } => "Malformed",
                dctx_plugin_distribution::DistributionError::LimitExceeded { .. } => {
                    "LimitExceeded"
                }
                _ => "Interrupted",
            };
            let body = if json {
                format!(
                    "{{\"ok\":false,\"command\":\"list\",\"error\":\"{name}\",\"detail\":{}}}",
                    render::json_string(&error.to_string())
                )
            } else {
                ops_output::failure_text(
                    "list",
                    &format!("{error} ({name})"),
                    "the plugin registry could not be read; check the plugin root directory",
                )
            };
            return Outcome { code: 1, body };
        }
    };
    let rendered = rows
        .iter()
        .map(|row| {
            let bundled = row.id.as_str() == "dctx/git";
            let alias = if bundled {
                bundled::catalog().alias
            } else {
                String::new()
            };
            let excluded = bundled && !bundled::worker_linked();
            let consent = if excluded {
                "unavailable"
            } else {
                render::consent_name(row.needs_consent, row.enablement)
            };
            let worker = if excluded {
                "excluded"
            } else if row.origin == Origin::Bundled && bundled::worker_linked() {
                "same-binary"
            } else {
                "host-target"
            };
            render::status_line(row, &alias, consent, worker, json)
        })
        .collect::<Vec<_>>();
    let body = if json {
        format!("{{\"ok\":true,\"command\":\"list\",\"facade_git\":{},\"facade_spawns\":0,\"facade_git_children\":0,\"plugins\":[{}]}}",
            dctx::Capabilities::current().git, rendered.join(","))
    } else {
        rendered.join("\n")
    };
    Outcome { code: 0, body }
}

fn help_outcome(text: &str, json: bool) -> Outcome {
    if json {
        Outcome {
            code: 0,
            body: format!(
                "{{\"ok\":true,\"command\":\"help\",\"text\":{}}}",
                render::json_string(text)
            ),
        }
    } else {
        Outcome {
            code: 0,
            body: text.to_owned(),
        }
    }
}

/// JSON and successful text go to stdout. Text refusals go to stderr.
fn print_body(json: bool, outcome: &Outcome) {
    if outcome.body.is_empty() {
        return;
    }
    let body = outcome.body.trim_end_matches('\n');
    if json || outcome.code == 0 {
        println!("{body}");
    } else {
        eprintln!("{body}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn early_routing_keeps_snapshot_dot_and_help() {
        assert!(is_early("plugin"));
        assert!(is_early("git"));
        assert!(is_early("--internal-plugin-worker"));
        assert!(!is_early("."));
        assert!(!is_early("--help"));
        assert!(!is_early("pack"));
        assert!(!is_early("map"));
    }

    #[test]
    fn store_changes_are_classified_and_a_read_only_store_refuses_them_by_name() {
        let action = |words: &[&str]| {
            let args: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
            parse::parse_argv(&args).expect("parse").action
        };
        for words in [
            &["dctx", "plugin", "enable", "dctx/git", "--approve"][..],
            &["dctx", "plugin", "disable", "dctx/git"],
            &["dctx", "plugin", "remove", "dctx/git"],
            &["dctx", "plugin", "update"],
        ] {
            assert!(changes_store(&action(words)), "{words:?}");
        }
        for words in [
            &["dctx", "plugin", "update", "--check"][..],
            &["dctx", "plugin", "info", "dctx/git"],
            &["dctx", "plugin", "verify"],
            &["dctx", "plugin", "search", "git"],
            &["dctx", "git", "status"],
        ] {
            assert!(!changes_store(&action(words)), "{words:?}");
        }
        let text = ops_output::read_only_store("enable", false);
        assert_eq!(text.code, 1);
        assert!(text
            .body
            .starts_with("❌ Error: enable: the plugin store is read-only on "));
        assert!(
            text.body.contains("(ReadOnlyStore)\nhint: "),
            "{}",
            text.body
        );
        let json = ops_output::read_only_store("enable", true);
        assert!(
            json.body.contains("\"error\":\"ReadOnlyStore\""),
            "{}",
            json.body
        );
        assert!(ops_output::consent_step("dctx/git", "them")
            .contains("`dctx plugin enable dctx/git --approve`"));
    }
}
