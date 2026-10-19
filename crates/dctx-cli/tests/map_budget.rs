//! Binary checks that `map read` of a normal module fits its default budget.
//! Covers SPEC-MAP-MODE §4 and contracts/API-PORTS-v1.md read token caps.
//! Depends on the compiled dctx-cli and a temporary source file. No network.
//! Used by `cargo nextest -p dctx-cli`. A tiny `--budget` must name the needed size.
//! The default is the facade hard cap. A page that cannot fit is refused exactly.

#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Repository {
    root: PathBuf,
}

impl Repository {
    fn new(label: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("dctx-e02j-{label}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("repo")).expect("repo");
        for folder in ["home", "config", "cache", "plugins", "map-cache", "local-app-data", "empty-path", "tmp"] {
            fs::create_dir(root.join(folder)).expect("isolated directory");
        }
        Self { root }
    }

    fn repo(&self) -> PathBuf {
        self.root.join("repo")
    }

    fn output(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command.current_dir(self.repo()).env_clear().args(args).env("DCTX_AUTO_CLEAN", "0").env("NO_COLOR", "1");
        for (variable, folder) in [
            ("HOME", "home"),
            ("USERPROFILE", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("LOCALAPPDATA", "local-app-data"),
            ("DCTX_PLUGIN_ROOT", "plugins"),
            ("DCTX_CACHE_DIR", "cache"),
            ("DCTX_MAP_CACHE", "map-cache"),
            ("PATH", "empty-path"),
            ("TMPDIR", "tmp"),
            ("TMP", "tmp"),
            ("TEMP", "tmp"),
        ] {
            command.env(variable, self.root.join(folder));
        }
        command.output().expect("run compiled CLI")
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// About 5 KB of source, under the 120-line read default, with a head and a tail marker.
fn module_source() -> String {
    let mut body = String::from("fn BudgetBeacon() {\n    let kept = 1;\n}\n");
    let pad = "// pad line for the read budget ................\n";
    while body.len() < 5 * 1024 {
        body.push_str(pad);
    }
    body.push_str("// BUDGET_TAIL\n");
    body
}

fn needed_bytes(stderr: &str) -> Option<u64> {
    let marker = "needs at least ";
    let start = stderr.find(marker)? + marker.len();
    let rest = &stderr[start..];
    let end = rest.find(" bytes")?;
    rest[..end].parse().ok()
}

#[test]
fn read_of_a_five_kilobyte_module_returns_its_source() {
    let repo = Repository::new("module");
    let source = module_source();
    assert!(source.len() >= 5 * 1024, "fixture is {} bytes", source.len());
    assert!(source.lines().count() <= 120, "fixture has {} lines", source.lines().count());
    fs::write(repo.repo().join("wide.rs"), &source).expect("source");
    let output = repo.output(&["map", "read", "m:wide.rs", "--limit", "1", "--json"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "status {}; stdout: {stdout}; stderr: {stderr}",
        output.status
    );
    assert!(stderr.is_empty(), "{stderr}");
    assert!(stdout.contains("fn BudgetBeacon()"), "{stdout}");
    assert!(stdout.contains("BUDGET_TAIL"), "{stdout}");
    assert!(stdout.contains("let kept = 1;"), "{stdout}");
}

#[test]
fn tiny_budget_names_the_needed_size() {
    let repo = Repository::new("tiny");
    let source = module_source();
    fs::write(repo.repo().join("wide.rs"), &source).expect("source");
    // 400 tokens admit 1,400 bytes: the envelope can fit, the module page cannot.
    let output = repo.output(&["map", "read", "m:wide.rs", "--limit", "1", "--json", "--budget", "400"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "tiny budget succeeded: {stdout}");
    assert!(stdout.is_empty(), "stdout on refusal: {stdout}");
    let needed = needed_bytes(&stderr).unwrap_or_else(|| panic!("no needed size in {stderr}"));
    let ceiling = 400u64 * 7 / 2;
    assert!(needed > ceiling, "needed {needed} is not above the {ceiling}-byte ceiling: {stderr}");
    assert!(needed > source.len() as u64, "needed {needed} does not cover the module: {stderr}");
}
