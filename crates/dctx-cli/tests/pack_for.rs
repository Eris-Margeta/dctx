// SPDX-License-Identifier: Apache-2.0
//! Binary checks of `dctx pack --for "<text>" --budget N` (v1 card 9, agent task context).
//! The page never exceeds the budget (estimate ceil(2 * UTF-8 bytes / 7), the `map --budget` one), names the
//! symbols the card text names, keeps whole lines, is deterministic, and never prints a sensitive file.
//! Depends on the compiled dctx-cli and an isolated temporary place and cache. No network.
//! Used by `cargo test -p dctx-cli`.

#![forbid(unsafe_code)]

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

/// The card text every budget test packs for: two backticked or code-like names, one PascalCase type and a plain word.
const CARD: &str =
    "Fix `alpha_body` so BetaStore reads the TOKEN once; keep beta_helper unchanged.";

struct Place {
    root: PathBuf,
}

impl Place {
    fn new(label: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("dctx-packfor-{label}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("repo/src")).expect("repo");
        for folder in [
            "home",
            "config",
            "cache",
            "plugins",
            "map-cache",
            "local-app-data",
            "empty-path",
            "tmp",
        ] {
            fs::create_dir(root.join(folder)).expect("isolated directory");
        }
        let repo = root.join("repo");
        fs::write(repo.join("src/alpha.rs"), "pub fn alpha_body() -> u32 {\n    beta_helper() + 1\n}\n\npub fn beta_helper() -> u32 {\n    41\n}\n").expect("alpha");
        fs::write(repo.join("src/store.rs"), store_source()).expect("store");
        fs::write(repo.join(".env"), "TOKEN=CANARY_PACK_FOR_9\n").expect("env");
        Self { root }
    }

    fn repo(&self) -> PathBuf {
        self.root.join("repo")
    }

    fn output(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .current_dir(self.repo())
            .env_clear()
            .args(args)
            .env("DCTX_AUTO_CLEAN", "0")
            .env("NO_COLOR", "1");
        common::isolated_dirs(&mut command, &self.root);
        for (variable, folder) in [
            ("DCTX_PLUGIN_ROOT", "plugins"),
            ("DCTX_CACHE_DIR", "cache"),
            ("DCTX_MAP_CACHE", "map-cache"),
            ("PATH", "empty-path"),
        ] {
            command.env(variable, self.root.join(folder));
        }
        command.output().expect("run compiled CLI")
    }

    /// Runs a pack that must succeed with an empty stderr and returns its stdout.
    fn page(&self, args: &[&str]) -> String {
        let output = self.output(args);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "{args:?}: status {}; stdout: {stdout}; stderr: {stderr}",
            output.status
        );
        assert!(stderr.is_empty(), "{args:?}: {stderr}");
        stdout
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A struct with 200 methods that each call `alpha_body`: a large read and many callers, so a budget must cut.
fn store_source() -> String {
    let mut text =
        String::from("pub struct BetaStore {\n    pub slots: Vec<u32>,\n}\n\nimpl BetaStore {\n");
    for n in 0..200 {
        text.push_str(&format!("    pub fn slot_{n:03}(&self) -> u32 {{\n        crate::alpha::alpha_body() + {n}\n    }}\n"));
    }
    text.push_str("}\n");
    text
}

/// The `map --budget` estimate: ceil(2 * UTF-8 bytes / 7).
fn estimate(text: &str) -> u64 {
    (2 * u64::try_from(text.len()).expect("length")).div_ceil(7)
}

/// The `dropped=<n>` number of the end line.
fn dropped(page: &str) -> u64 {
    let end = page.lines().last().expect("end line");
    assert!(end.starts_with("# end body_tokens="), "{end}");
    end.rsplit("dropped=")
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("no dropped count in {end}"))
}

#[test]
fn budget_3000_fits_and_names_the_card_symbols() {
    let place = Place::new("fits");
    let page = place.page(&["pack", "--for", CARD, "--budget", "3000"]);
    assert!(
        estimate(&page) <= 3000,
        "{} estimated tokens",
        estimate(&page)
    );
    assert!(
        page.starts_with("# dctx pack --for budget=3000 terms="),
        "{page}"
    );
    for named in [
        "s:src/alpha.rs#alpha_body",
        "src/store.rs#BetaStore",
        "s:src/alpha.rs#beta_helper",
    ] {
        assert!(page.contains(named), "missing {named} in {page}");
    }
    assert!(page.contains("## find alpha_body\n"), "{page}");
    assert!(
        page.contains("## neighbors s:src/alpha.rs#alpha_body\n"),
        "{page}"
    );
    assert!(
        page.contains("## read s:src/alpha.rs#alpha_body\n"),
        "{page}"
    );
    assert!(
        page.contains("beta_helper() + 1"),
        "the read section carries the source: {page}"
    );
    assert!(
        !page.contains("CANARY_PACK_FOR_9"),
        "a sensitive file reached the page: {page}"
    );
    assert!(page.ends_with('\n'));
}

#[test]
fn the_default_budget_is_3000() {
    let place = Place::new("default");
    let page = place.page(&["pack", "--for", CARD]);
    assert!(page.starts_with("# dctx pack --for budget=3000 "), "{page}");
    assert!(estimate(&page) <= 3000);
}

#[test]
fn a_small_budget_cuts_whole_lines_and_counts_them() {
    let place = Place::new("small");
    let page = place.page(&["pack", ".", "--for", CARD, "--budget", "300"]);
    assert!(
        estimate(&page) <= 300,
        "{} estimated tokens: {page}",
        estimate(&page)
    );
    assert!(
        page.contains("s:src/alpha.rs#alpha_body"),
        "the first named symbol must survive a small budget: {page}"
    );
    assert!(
        dropped(&page) > 0,
        "a 300-token budget cannot hold 200 callers and a 200-method read: {page}"
    );
    for line in page
        .lines()
        .filter(|line| line.starts_with("s:") || line.starts_with("calls:"))
    {
        assert!(line.contains('\t'), "a record line was cut: {line:?}");
    }
    let large = place.page(&["pack", ".", "--for", CARD, "--budget", "100000"]);
    assert!(large.len() > page.len());
}

#[test]
fn two_runs_are_byte_identical() {
    let place = Place::new("same");
    let first = place.page(&["pack", "--for", CARD, "--budget", "1200"]);
    let second = place.page(&["pack", "--for", CARD, "--budget", "1200"]);
    assert_eq!(first, second);
}

#[test]
fn bad_inputs_follow_the_exit_code_contract() {
    let place = Place::new("bad");
    let missing = place.root.join("missing").display().to_string();
    for (want, args) in [
        (2, vec!["pack", "--budget", "3000"]),
        (2, vec!["pack", "--for"]),
        (2, vec!["pack", "--for", "alpha_body", "--budget", "0"]),
        (2, vec!["pack", "--for", "alpha_body", "--budget", "99"]),
        (2, vec!["pack", "--for", "alpha_body", "--budget", "many"]),
        (2, vec!["pack", "--for", "   "]),
        (2, vec!["pack", "--for", "alpha_body", "--ext", "rs"]),
        (1, vec!["pack", "--for", "zzzqqq_nowhere"]),
        (3, vec!["pack", missing.as_str(), "--for", "alpha_body"]),
    ] {
        let output = place.output(&args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(want), "{args:?}: {stderr}");
        assert!(stderr.contains("hint: "), "{args:?}: {stderr}");
        assert!(!stderr.contains("panicked"), "{args:?}: {stderr}");
    }
}

/// Adds a `main` entry point, a `Session` type and a `session` test fn to the place.
fn add_entry_point_and_session(place: &Place) {
    let repo = place.repo();
    fs::write(
        repo.join("src/main.rs"),
        "fn main() {\n    run();\n}\n\nfn run() {}\n",
    )
    .expect("main");
    fs::write(
        repo.join("src/session.rs"),
        "pub struct Session {\n    pub id: u32,\n}\n",
    )
    .expect("session");
    fs::create_dir_all(repo.join("tests")).expect("tests");
    fs::write(
        repo.join("tests/session.rs"),
        "#[test]\nfn session() {\n    assert_eq!(1, 1);\n}\n",
    )
    .expect("session test");
}

#[test]
fn bare_short_names_are_searched() {
    let place = Place::new("short");
    add_entry_point_and_session(&place);
    for text in ["main", "fn main", "run"] {
        let page = place.page(&["pack", "--for", text]);
        assert!(
            page.starts_with("# dctx pack --for budget=3000 terms=1\n"),
            "{text}: {page}"
        );
        assert!(page.contains("s:src/main.rs#"), "{text}: {page}");
    }
    let page = place.page(&["pack", "--for", "main"]);
    assert!(page.contains("## read s:src/main.rs#main\n"), "{page}");
}

#[test]
fn a_capitalized_name_finds_the_type_before_test_fns() {
    let place = Place::new("case");
    add_entry_point_and_session(&place);
    let page = place.page(&["pack", "--for", "Session"]);
    assert!(page.contains("## find Session\n"), "{page}");
    let first = page
        .split("## find Session\n")
        .nth(1)
        .and_then(|rest| rest.lines().next())
        .unwrap_or_default();
    assert!(
        first.starts_with("s:src/session.rs#Session\t"),
        "the type comes first: {page}"
    );
    assert!(
        page.contains("## read s:src/session.rs#Session\n"),
        "{page}"
    );
}

#[test]
fn prose_without_identifiers_warns_on_stderr_only() {
    let place = Place::new("prose");
    let text = "how does the helper work";
    let first = place.output(&["pack", "--for", text]);
    let second = place.output(&["pack", "--for", text]);
    let stderr = String::from_utf8_lossy(&first.stderr);
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(
        stderr.contains("warning: no identifiers in --for text"),
        "{stderr}"
    );
    assert!(stderr.contains("dctx pack --help"), "{stderr}");
    assert!(
        stdout.starts_with("# dctx pack --for budget=3000 terms=1\n"),
        "{stdout}"
    );
    assert!(
        !stdout.contains("warning"),
        "the page stays clean: {stdout}"
    );
    assert_eq!(first.stdout, second.stdout, "the page is deterministic");
    let json = place.output(&["pack", "--for", text, "--json"]);
    let document: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("one JSON document on stdout");
    assert_eq!(document["verb"], "context");
    assert!(String::from_utf8_lossy(&json.stderr).contains("warning: no identifiers"));
    // Names, even plain ones, are not prose.
    let named = place.output(&["pack", "--for", "BetaStore"]);
    assert!(
        named.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&named.stderr)
    );
}

#[test]
fn a_text_without_terms_says_why_and_suggests_backticks() {
    let place = Place::new("noterms");
    let output = place.output(&["pack", "--for", "how does it work"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("no search terms"), "{stderr}");
    assert!(stderr.contains("backticks"), "{stderr}");
    assert!(!stderr.contains("matched the index"), "{stderr}");
    let refused = place.output(&["pack", "--for", "alpha_body", "--ext", "rs"]);
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("--budget and --json only"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
}

#[test]
fn pack_help_names_for_and_budget_with_an_example() {
    let place = Place::new("help");
    let help = place.page(&["pack", "--help"]);
    assert!(help.contains("--for <text>"), "{help}");
    assert!(help.contains("--budget <tokens>"), "{help}");
    assert!(help.contains("Example: dctx pack --for "), "{help}");
}
