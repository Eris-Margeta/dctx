//! Plugin management commands and the same-binary worker switch.
//! Implements PLUGINS.md §5.1 and §7.3 row PL-CLI: every management command goes through
//! `dctx-plugin-distribution`, and the bundled git catalog comes from the dctx facade.
//! `list` and `help` do not lock the registry, fetch, or start a worker. Used by `main`.

mod bundled;
mod disk;
mod fetch;
mod ops;
mod parse;
mod render;
mod worker;

use dctx::PluginHost;
use dctx_plugin_distribution::{Clock, InstallPolicy, Network, Store, TrustStore};

pub use worker::enter as enter_worker;

use ops::Outcome;
use parse::Action;

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
pub fn enter(args: &[String]) -> u8 {
    let root = disk::default_root();
    enter_at(args, &root)
}

/// Same as [`enter`] with an explicit plugin root. A missing root is left missing for read-only commands.
pub fn enter_at(args: &[String], root: &std::path::Path) -> u8 {
    let request = match parse::parse_argv(args) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("plugin: {error}");
            return 2;
        }
    };
    if matches!(request.action, Action::Help) {
        let body = render::help_text();
        print_body(request.json, &help_outcome(&body, request.json));
        return 0;
    }
    let store = disk::FsStore::new(root);
    let clock = disk::SystemClock;
    let trust = TrustStore::new();
    let policy = InstallPolicy::new(disk::host_target());
    let network = disk::NoTls;
    let host = PluginHost::platform();
    let mut index = IndexCache::default();
    let mut env = Env::product(&store, &clock, &trust, &policy, &mut index, &network, &host);
    let outcome = ops::execute(&request, &mut env);
    print_body(request.json, &outcome);
    outcome.code
}

fn help_outcome(text: &str, json: bool) -> Outcome {
    if json {
        Outcome { code: 0, body: format!("{{\"ok\":true,\"command\":\"help\",\"text\":{}}}", render::json_string(text)) }
    } else {
        Outcome { code: 0, body: text.to_owned() }
    }
}

fn print_body(json: bool, outcome: &Outcome) {
    if outcome.body.is_empty() {
        return;
    }
    if json || outcome.body.ends_with('\n') {
        println!("{}", outcome.body.trim_end_matches('\n'));
    } else {
        println!("{}", outcome.body);
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
}
