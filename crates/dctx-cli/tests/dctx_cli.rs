//! Cache command persistence through the library surface.
//! Implements ARCHITECTURE-PROPOSAL §4 `cache config` and corrupt-ledger refusal.
//! Depends on `dctx-cli` [`run_in`]. The binary's `dctx .` path is not invoked.

use std::path::PathBuf;

use dctx_cli::run_in;

#[test]
fn config_persists_across_two_invocations() {
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
}

#[test]
fn a_corrupt_ledger_is_reported_and_left_in_place() {
    let root = scratch("corrupt");
    std::fs::write(root.join("ledger.v1"), b"not-a-ledger").expect("write");
    let err = run_in(&args(&["cache", "status"]), Some(&root)).expect_err("status");
    assert!(err.contains("corrupt ledger"), "{err}");
    assert_eq!(std::fs::read(root.join("ledger.v1")).expect("kept"), b"not-a-ledger");
}

#[test]
fn help_names_config_auto_and_the_ttl_defaults() {
    let page = run_in(&args(&["cache", "--help"]), None).expect("help");
    assert!(page.contains("dctx cache config"));
    assert!(page.contains("dctx cache auto"));
    assert!(page.contains("--no-auto-clean"));
    assert!(page.contains("7 days"));
    assert!(page.contains("cache cap:"));
}

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("dctx-cli-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("dir");
    path
}
