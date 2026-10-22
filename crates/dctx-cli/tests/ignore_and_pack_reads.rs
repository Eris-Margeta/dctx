//! Ignored paths stay unread, and `pack --ext` opens only matching files.
//! Covers DCTX-COLD-SCAN C3 and E-02e G8b. A rule-text edit misses the reuse key.
//! Depends on the compiled `dctx-cli` binary and a private temporary directory.
//! Used by `cargo nextest run -p dctx-cli`.
//! `DCTX_INDEX_IGNORED=1` is the opt-in. `DCTX_READ_REPORT=1` lists pack reads.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Repository {
    root: PathBuf,
}

impl Repository {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("dctx-e02h-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let repo = root.join("repo");
        fs::create_dir_all(repo.join("src")).expect("src");
        fs::create_dir_all(repo.join("corpus")).expect("corpus");
        for folder in ["home", "config", "cache", "plugins", "map-cache", "local-app-data", "empty-path", "tmp"] {
            fs::create_dir_all(root.join(folder)).expect("isolated directory");
        }
        fs::create_dir_all(root.join("config").join("dctx")).expect("global rules");
        fs::write(root.join("config").join("dctx").join("ignore"), "# one\n").expect("global ignore");
        fs::write(repo.join(".gitignore"), "/corpus/\n*.secret.rs\n!keep.secret.rs\n").expect("gitignore");
        fs::write(repo.join("src").join(".gitignore"), "generated.rs\n").expect("nested");
        fs::write(repo.join("keep.rs"), "fn keep_marker() {}\n").expect("keep");
        fs::write(repo.join("keep.secret.rs"), "fn negated_marker() {}\n").expect("negated");
        fs::write(repo.join("drop.secret.rs"), "fn drop_marker() {}\n").expect("drop");
        fs::write(repo.join("corpus").join("ready.rs"), "fn corpus_marker() {}\n").expect("corpus");
        fs::write(repo.join("src").join("lib.rs"), "fn lib_marker() {}\n").expect("lib");
        fs::write(repo.join("src").join("generated.rs"), "fn generated_marker() {}\n").expect("generated");
        fs::write(repo.join("notes.md"), "notes_marker\n").expect("notes");
        fs::write(repo.join("logo.png"), b"\x89PNG\0png_marker").expect("png");
        Self { root }
    }

    fn repo(&self) -> PathBuf {
        self.root.join("repo")
    }

    fn output(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
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
        for (key, value) in extra {
            command.env(key, value);
        }
        command.output().expect("run compiled CLI")
    }

    fn run(&self, args: &[&str], extra: &[(&str, &str)]) -> (String, String) {
        let output = self.output(args, extra);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(output.status.success(), "{args:?}: status {}; stdout: {stdout}; stderr: {stderr}", output.status);
        (stdout, stderr)
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn count_line(stderr: &str, label: &str) -> u64 {
    stderr
        .lines()
        .find_map(|line| line.strip_prefix(label)?.trim().parse().ok())
        .unwrap_or_else(|| panic!("missing {label} in {stderr}"))
}

fn payload(stdout: &str) -> &str {
    stdout.rfind("pack bytes=").map_or_else(|| panic!("status line in {stdout}"), |pos| &stdout[..pos])
}

fn file_paths(page: &str) -> BTreeSet<String> {
    let start = page.find("<DIRECTORY_TREE>").unwrap_or_else(|| panic!("tree in {page}"));
    let rest = &page[start..];
    let end = rest.find("</DIRECTORY_TREE>").unwrap_or_else(|| panic!("tree end in {page}"));
    let mut paths = BTreeSet::new();
    for line in rest[..end].lines() {
        let Some(rest) = line.trim_start().strip_prefix("[F] ") else {
            continue;
        };
        let head = rest.split(" (ID:").next().unwrap_or(rest);
        let path = head.strip_suffix(" [IGNORED]").unwrap_or(head).trim();
        if !path.is_empty() {
            paths.insert(path.to_string());
        }
    }
    paths
}

fn read_paths(stderr: &str) -> BTreeSet<String> {
    stderr.lines().filter_map(|line| line.strip_prefix("read ").map(str::to_string)).collect()
}

#[test]
fn ignored_files_stay_unread_until_opt_in_and_a_rule_edit_rebuilds() {
    let repo = Repository::new();
    let place = repo.repo().to_string_lossy().into_owned();
    let args = ["map", "find", "marker", "--json", "--place", place.as_str()];

    let (cold, cold_err) = repo.run(&args, &[("DCTX_REUSE_REPORT", "1")]);
    assert!(cold_err.contains("generation built"), "{cold_err}");
    assert!(!cold_err.contains("generation reused"), "{cold_err}");
    // .gitignore, keep.rs, keep.secret.rs, notes.md, logo.png, src/.gitignore, src/lib.rs.
    let cold_reads = count_line(&cold_err, "content_reads");
    assert_eq!(cold_reads, 7, "{cold_err}");
    assert!(cold.contains("keep_marker"), "{cold}");
    assert!(cold.contains("negated_marker"), "{cold}");
    assert!(cold.contains("lib_marker"), "{cold}");
    assert!(!cold.contains("drop_marker"), "{cold}");
    assert!(!cold.contains("corpus_marker"), "{cold}");
    assert!(!cold.contains("generated_marker"), "{cold}");

    let (_warm, warm_err) = repo.run(&args, &[("DCTX_REUSE_REPORT", "1")]);
    assert!(warm_err.contains("generation reused"), "{warm_err}");
    assert!(count_line(&warm_err, "content_reads") < cold_reads, "{warm_err}");

    fs::write(repo.root.join("config").join("dctx").join("ignore"), "# two\n").expect("edit rule");
    let (edited, edited_err) = repo.run(&args, &[("DCTX_REUSE_REPORT", "1")]);
    assert!(edited_err.contains("generation built"), "{edited_err}");
    assert!(!edited_err.contains("generation reused"), "{edited_err}");
    assert_eq!(count_line(&edited_err, "content_reads"), cold_reads, "{edited_err}");
    assert!(!edited.contains("corpus_marker"), "{edited}");

    let (opt_in, opt_err) = repo.run(&args, &[("DCTX_REUSE_REPORT", "1"), ("DCTX_INDEX_IGNORED", "1")]);
    assert!(opt_err.contains("generation built"), "{opt_err}");
    // The three ignored rust files join the seven included files.
    assert_eq!(count_line(&opt_err, "content_reads"), cold_reads + 3, "{opt_err}");
    assert!(opt_in.contains("corpus_marker"), "{opt_in}");
    assert!(opt_in.contains("drop_marker"), "{opt_in}");
    assert!(opt_in.contains("generated_marker"), "{opt_in}");
    assert!(opt_in.contains("keep_marker"), "{opt_in}");
}

#[test]
#[allow(clippy::case_sensitive_file_extension_comparisons, reason = "The export fixture selects literal lowercase .txt paths; folding case would change the fixture policy.")]
fn pack_ext_opens_only_rust_files_and_matches_the_filtered_page() {
    let repo = Repository::new();
    let place = repo.repo().to_string_lossy().into_owned();
    let (open, open_err) = repo.run(&["pack", place.as_str()], &[]);
    assert!(open_err.is_empty(), "{open_err}");
    let open_page = payload(&open);
    assert!(open_page.contains("notes_marker"), "{open_page}");
    assert!(open_page.contains("keep_marker"), "{open_page}");
    assert!(!open_page.contains("logo.png"), "{open_page}");
    assert!(!open_page.contains("drop_marker"), "{open_page}");
    assert!(!open_page.contains("corpus_marker"), "{open_page}");
    assert!(!open_page.contains("generated_marker"), "{open_page}");
    let open_paths = file_paths(open_page);
    assert!(!open_paths.iter().any(|path| path.contains("drop.secret") || path.contains("generated.rs") || path.starts_with("corpus")), "{open_paths:?}");
    let rust_only: BTreeSet<String> = open_paths.iter().filter(|path| path.ends_with(".rs")).cloned().collect();

    let (filtered, filtered_err) = repo.run(&["pack", place.as_str(), "--ext", "rs"], &[("DCTX_READ_REPORT", "1")]);
    let filtered_page = payload(&filtered);
    assert_eq!(file_paths(filtered_page), rust_only, "{filtered_page}");
    assert_eq!(read_paths(&filtered_err), rust_only, "{filtered_err}");
    assert!(filtered_page.contains("lib_marker"), "{filtered_page}");
    assert!(!filtered_page.contains("notes_marker"), "{filtered_page}");
    assert!(!filtered_page.contains("png_marker"), "{filtered_page}");
    assert!(!filtered_page.contains("drop_marker"), "{filtered_page}");
    assert!(rust_only.contains("keep.rs"), "{rust_only:?}");
    assert!(rust_only.contains("src/lib.rs"), "{rust_only:?}");
}
