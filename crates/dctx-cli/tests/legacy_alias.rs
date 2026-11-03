// SPDX-License-Identifier: Apache-2.0
//! Binary compatibility checks for D3-LEGACY-REMOVAL §5 D3-04.
//! Each child receives a private HOME/XDG/cache and empty PATH; no Git is available.
//! Pinned renderer fixtures are immutable oracles; timestamps come from std `FileTimes`.
//! Clipboard probes supply only a fake clipboard executable and record its exact stdin.

use dctx_pack::{decode_legacy, LegacyBodies, LegacyLimits};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, UNIX_EPOCH},
};

struct Case {
    base: PathBuf,
    root: PathBuf,
}
impl Case {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "dctx-alias-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let root = base.join("Corpus");
        fs::create_dir_all(&root).unwrap();
        for dir in ["home", "config/dctx", "cache/dctx", "bin", "managed"] {
            fs::create_dir_all(base.join(dir)).unwrap();
        }
        Self { base, root }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", "")
            .env("HOME", self.base.join("home"))
            .env("XDG_CONFIG_HOME", self.base.join("config"))
            .env("XDG_CACHE_HOME", self.base.join("cache"))
            .env("APPDATA", self.base.join("config"))
            .env("LOCALAPPDATA", self.base.join("cache"))
            .env("DCTX_CACHE_DIR", self.base.join("managed"))
            .env("DCTX_AUTO_CLEAN", "0");
        command
    }
    fn run(&self, args: &[&str]) -> Output {
        let output = self.command().args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
    fn write(&self, path: &str, bytes: &[u8]) {
        let full = self.root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, bytes).unwrap();
    }
    fn config(&self, text: &str) {
        fs::write(self.base.join("config/dctx/config.toml"), text).unwrap();
    }
    fn text(&self) -> Vec<u8> {
        fs::read(self.root.join(".dctx/Corpus.llmcontext.txt")).unwrap()
    }
    fn binary(&self) -> PathBuf {
        let registry = fs::read_to_string(self.base.join("cache/dctx/projects.db")).unwrap();
        let id = registry.lines().last().unwrap().split('|').next().unwrap().trim();
        self.base.join("cache/dctx").join(format!("{id}.dctx"))
    }
    fn version(&self) -> String {
        decode_legacy(&fs::read(self.binary()).unwrap(), LegacyBodies::MetadataOnly, LegacyLimits::default())
            .unwrap()
            .header
            .version
            .to_string()
    }
    fn corpus(&self) {
        self.write(".git/config", b"sensitive");
        self.write("Cargo.lock", b"# lock\n");
        self.write("logo.png", &[0x89, b'P', b'N', b'G', 0, 0, 1]);
        self.write("secret.env", b"SECRET=1\n");
        self.write("src/main.rs", b"fn main() {}\n");
        fs::create_dir(self.root.join(".dctx")).unwrap();
        fs::write(self.base.join("config/dctx/ignore"), b"secret.env\n").unwrap();
        for (path, millis) in [
            (".git/config", 1700),
            (".git", 1600),
            ("Cargo.lock", 1500),
            ("logo.png", 1400),
            ("secret.env", 1300),
            ("src/main.rs", 1200),
            ("src", 1100),
            ("", 1000),
        ] {
            stamp(&self.root.join(path), millis);
        }
    }
}
impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}
fn stamp(path: &Path, millis: u64) {
    fs::File::open(path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_millis(millis)))
        .unwrap();
}

#[test]
fn alias_snapshot_and_tree_are_the_pinned_bytes() {
    // Every mode is pinned: no flag and `--tree-stamps=off` give the MOD-free text, `--tree-stamps=on` the 1.0.0 bytes.
    for (flag, tree, expected) in [
        ("", false, include_bytes!("../../dctx-template/tests/fixtures/legacy-snapshot.txt").as_slice()),
        ("", true, include_bytes!("../../dctx-template/tests/fixtures/legacy-tree.txt").as_slice()),
        ("--tree-stamps=off", false, include_bytes!("../../dctx-template/tests/fixtures/legacy-snapshot.txt").as_slice()),
        ("--tree-stamps=off", true, include_bytes!("../../dctx-template/tests/fixtures/legacy-tree.txt").as_slice()),
        ("--tree-stamps=on", false, include_bytes!("../../dctx-template/tests/fixtures/legacy-snapshot-stamped.txt").as_slice()),
        ("--tree-stamps=on", true, include_bytes!("../../dctx-template/tests/fixtures/legacy-tree-stamped.txt").as_slice()),
    ] {
        let case = Case::new();
        case.corpus();
        let mut args = vec![".", "--debug-ignored", "--non-empty", "--binary"];
        if !flag.is_empty() {
            args.push(flag);
        }
        if tree {
            args.push("--tree");
        }
        case.run(&args);
        assert_eq!(case.text(), expected, "{flag} tree={tree}");
        assert_eq!(case.version(), "V1");
        assert!(!case.root.join(".gitignore").exists());
        assert_eq!(fs::read(case.root.join(".git/config")).unwrap(), b"sensitive");
        assert!(!case.root.join(".git/index").exists());
    }
}

#[test]
fn second_run_diffs_unchanged_stays_put_and_resets_retain_exports() {
    let case = Case::new();
    case.config("text_output_mode_generates_diff_files = true\nmax_diff_files = 1\n");
    case.write("kept.rs", b"fn kept() {}\n");
    case.write("change.rs", b"before\n");
    case.write(".gitignore", b"do-not-edit\n");
    case.run(&["."]);
    let registry = fs::read(case.base.join("cache/dctx/projects.db")).unwrap();
    case.run(&["."]);
    assert_eq!(case.version(), "V1");
    case.write("change.rs", b"after: a different size\n");
    case.run(&["."]);
    assert_eq!(case.version(), "V1.1");
    let diff = case.root.join(".dctx/Corpus.llmcontext-V1.1-diff.txt");
    let bytes = fs::read(&diff).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.starts_with("[DCTX_LLM_CONTEXT_DIFF_V1.1]\nVersion Change: V1 -> V1.1\n"));
    assert!(text.contains("[MODIFIED] change.rs"));
    assert!(text.contains("after: a different size"));
    assert!(!text.contains("fn kept()"));
    assert!(text.contains("<UPDATED_DIRECTORY_TREE>\n[D]  (ID:ROOT)\n"), "default diff tree has no timestamp: {text}");
    assert!(!text.contains("MOD:"), "default diff carries no MOD: {text}");
    assert!(!String::from_utf8(case.text()).unwrap().contains("MOD:"), "default snapshot carries no MOD");
    case.write("change.rs", b"next version\n");
    case.run(&["."]);
    assert_eq!(case.version(), "V2");
    assert_eq!(fs::read(&diff).unwrap(), bytes);
    case.run(&[".", "--reset"]);
    assert_eq!(case.version(), "V1");
    assert_eq!(fs::read(&diff).unwrap(), bytes);
    fs::write(case.binary(), b"corrupt").unwrap();
    case.run(&["."]);
    assert_eq!(case.version(), "V1");
    assert_eq!(fs::read(&diff).unwrap(), bytes);
    assert_eq!(fs::read(case.base.join("cache/dctx/projects.db")).unwrap(), registry);
    assert_eq!(fs::read(case.root.join(".gitignore")).unwrap(), b"do-not-edit\n");
}

#[test]
fn a_bare_touch_is_no_change_and_a_same_size_edit_is_one() {
    let case = Case::new();
    case.write("kept.rs", b"fn kept() {}\n");
    case.write("edit.rs", b"before\n");
    stamp(&case.root.join("edit.rs"), 5_000);
    case.run(&["."]);
    assert_eq!(case.version(), "V1");
    stamp(&case.root.join("edit.rs"), 6_000);
    let touched = case.run(&["."]);
    assert!(String::from_utf8_lossy(&touched.stdout).contains("No changes detected"));
    assert_eq!(case.version(), "V1");
    case.write("edit.rs", b"behind\n");
    stamp(&case.root.join("edit.rs"), 7_000);
    let edited = case.run(&["."]);
    assert!(String::from_utf8_lossy(&edited.stdout).contains("~1"));
    assert_eq!(case.version(), "V1.1");
}

#[test]
fn tree_stamps_on_keeps_mod_in_the_snapshot_and_the_diff_tree() {
    let case = Case::new();
    case.config("text_output_mode_generates_diff_files = true\nmax_diff_files = 1\n");
    case.write("kept.rs", b"fn kept() {}\n");
    case.write("change.rs", b"before\n");
    case.run(&[".", "--tree-stamps=on"]);
    assert!(String::from_utf8(case.text()).unwrap().contains("(ID:ROOT, MOD:"));
    case.write("change.rs", b"after: a different size\n");
    case.run(&[".", "--tree-stamps=on"]);
    let diff = fs::read_to_string(case.root.join(".dctx/Corpus.llmcontext-V1.1-diff.txt")).unwrap();
    assert!(diff.contains("<UPDATED_DIRECTORY_TREE>\n[D]  (ID:ROOT, MOD:"), "{diff}");
    assert!(diff.contains("change.rs (ID:F001, MOD:") || diff.contains("(ID:F002, MOD:"), "{diff}");
}

#[test]
fn registry_keeps_existing_mapping_and_known_fnv_vectors() {
    use dctx_cli::compat::{path_id, project_id};
    assert_eq!(path_id(""), "cbf29ce484222325");
    assert_eq!(path_id("a"), "af63dc4c8601ec8c");
    assert_eq!(path_id("test"), "f9e6e6ef197c2b25");
    assert_ne!(path_id("/Path/To/Project"), path_id("/path/to/project"));
    let case = Case::new();
    let cache = case.base.join("cache/dctx");
    let root = fs::canonicalize(&case.root).unwrap();
    assert_eq!(project_id(&cache, &root).unwrap(), path_id(&root.to_string_lossy()));
    let registry = format!("# old mapping\nignored line\nfirst | {}\nhistoric | {}\n", root.display(), root.display());
    fs::write(cache.join("projects.db"), &registry).unwrap();
    case.write("kept.rs", b"kept");
    case.run(&["."]);
    assert!(cache.join("historic.dctx").exists());
    assert_eq!(fs::read_to_string(cache.join("projects.db")).unwrap(), registry);
}

#[test]
fn filters_do_not_change_state_and_local_omit_is_not_cached() {
    // ADR-014: an omit rule denies the read. The omitted source must not reach the cache or the export.
    // `notes.txt` is the included-file read control; `--ext rs` only hides it from the text.
    let case = Case::new();
    case.write("kept.rs", b"pub fn kept() {}\n");
    case.write("notes.txt", b"notes\n");
    case.write(".dctxomitcontent", b"kept.rs\n");
    case.run(&[".", "--ext", "rs"]);
    let output = String::from_utf8(case.text()).unwrap();
    assert!(output.contains("kept.rs"));
    assert!(output.contains("[Content omitted by pattern rules]"));
    assert!(!output.contains("pub fn kept()"));
    assert!(!output.contains("notes.txt"));
    let bytes = fs::read(case.binary()).unwrap();
    assert!(!contains(&bytes, b"pub fn kept() {}\n"));
    assert!(contains(&bytes, b"notes\n"));
    let state = decode_legacy(&bytes, LegacyBodies::Include, LegacyLimits::default()).unwrap();
    let notes = state.root.children.iter().find(|node| node.relative_path == "notes.txt").unwrap();
    assert_eq!(notes.file_content.as_deref(), Some(b"notes\n".as_slice()));
    let kept = state.root.children.iter().find(|node| node.relative_path == "kept.rs").unwrap();
    assert!(!contains(kept.file_content.as_deref().unwrap_or_default(), b"pub fn kept()"));
    let failure = case.command().args([".", "--empty", "--non-empty"]).output().unwrap();
    assert!(!failure.status.success());
    assert_eq!(fs::read(case.binary()).unwrap(), bytes);
}

#[test]
fn configured_template_binary_only_and_rule_read_errors_keep_legacy_behavior() {
    let case = Case::new();
    case.corpus();
    // A custom template keeps `{{{node.mod}}}`: the stamped text compiled as a user template renders the 1.0.0 bytes.
    let template = dctx_template::TreeStamps::On.template();
    let compiled = template.compile("custom").unwrap();
    let directory = case.base.join("cache/dctx/templates");
    fs::create_dir_all(&directory).unwrap();
    compiled.serialize(&mut fs::File::create(directory.join("custom.bin")).unwrap()).unwrap();
    case.config("default_template = 'custom'\n");
    case.run(&[".", "--debug-ignored", "--non-empty", "--binary"]);
    assert_eq!(case.text(), include_bytes!("../../dctx-template/tests/fixtures/legacy-snapshot-stamped.txt"));
    let saved = case.text();
    case.config("output_mode = 'binary'\n");
    case.run(&[".", "--reset"]);
    assert_eq!(case.text(), saved);
    assert_eq!(case.version(), "V1");
    fs::create_dir(case.root.join(".dctxomitcontent")).unwrap();
    case.config("");
    case.run(&["."]);
    let binary = fs::read(case.binary()).unwrap();
    fs::create_dir(case.root.join(".dctxignore")).unwrap();
    let denied = case.command().arg(".").output().unwrap();
    assert!(!denied.status.success());
    assert_eq!(fs::read(case.binary()).unwrap(), binary);
}

#[cfg(unix)]
#[test]
fn clipboard_only_spawns_the_requested_tool_and_writes_no_project_text() {
    use std::os::unix::fs::PermissionsExt;
    let case = Case::new();
    case.write("kept.rs", b"fn kept() {}\n");
    case.write("change.rs", b"before\n");
    let tool = case.base.join("bin").join(if cfg!(target_os = "macos") { "pbcopy" } else { "wl-copy" });
    fs::write(&tool, b"#!/bin/sh\nprintf 'copy\\n' >> \"$CALLS\"\nexec /bin/cat > \"$CAPTURE\"\n").unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o700)).unwrap();
    let capture = case.base.join("clipboard");
    let calls = case.base.join("calls");
    let run = |args: &[&str]| {
        let output = case
            .command()
            .env("PATH", case.base.join("bin"))
            .env("CAPTURE", &capture)
            .env("CALLS", &calls)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Copied to clipboard"), "{output:?}");
    };
    run(&[".", "-c", "-d"]);
    assert!(fs::read_to_string(&capture).unwrap().starts_with("[DCTX_LLM_CONTEXT_SNAPSHOT_V1]"));
    case.write("change.rs", b"changed and longer\n");
    run(&[".", "-c", "-d"]);
    assert!(fs::read_to_string(&capture).unwrap().starts_with("[DCTX_LLM_CONTEXT_DIFF_V1.1]"));
    run(&[".", "-c", "-d"]);
    assert!(fs::read_to_string(&capture).unwrap().starts_with("[DCTX_LLM_CONTEXT_SNAPSHOT_V1.1]"));
    assert_eq!(fs::read_to_string(calls).unwrap(), "copy\ncopy\ncopy\n");
    assert!(!case.root.join(".dctx").exists());
    let failure = case.run(&[".", "-c"]);
    assert!(String::from_utf8_lossy(&failure.stderr).contains("Clipboard error"));
    assert!(!case.root.join(".dctx").exists());
}

#[test]
fn tui_loads_legacy_version_and_reset_uses_the_alias_host() {
    let case = Case::new();
    case.write("kept.rs", b"kept");
    case.write("change.rs", b"before");
    case.run(&["."]);
    case.write("change.rs", b"after, longer");
    case.run(&["."]);
    let mut child = case.command().stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"5\n3\n2\n\n0\n0\n0\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Last Snapshot: V1.1"));
    assert_eq!(case.version(), "V1");
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

fn report(output: &Output) -> String {
    format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

#[test]
fn legacy_writes_stay_inside_the_cache_and_the_project_export() {
    let case = Case::new();
    let sentinel = case.base.join("outside-sentinel");
    fs::write(&sentinel, b"SENTINEL-OUTSIDE").unwrap();
    case.write("a.txt", b"included\n");
    case.run(&["."]);
    assert_eq!(fs::read(&sentinel).unwrap(), b"SENTINEL-OUTSIDE");
    assert!(contains(&case.text(), b"included\n"));
    let binary = case.binary();
    assert!(binary.starts_with(case.base.join("cache/dctx")));
    assert!(binary.is_file());
    assert!(contains(&fs::read(&binary).unwrap(), b"included\n"));

    let escaped = case.base.join("cache/review-victim.dctx");
    fs::write(&escaped, b"SENTINEL-TRAVERSAL").unwrap();
    let root = fs::canonicalize(&case.root).unwrap();
    let registry = case.base.join("cache/dctx/projects.db");
    let saved_registry = fs::read(&registry).unwrap();
    fs::write(&registry, format!("../review-victim | {}\n", root.display())).unwrap();
    let traversed = case.command().args(["."]).output().unwrap();
    assert!(!traversed.status.success(), "{}", report(&traversed));
    assert_eq!(fs::read(&escaped).unwrap(), b"SENTINEL-TRAVERSAL");
    assert_eq!(fs::read(&sentinel).unwrap(), b"SENTINEL-OUTSIDE");

    let absolute = case.base.join("abs-victim.dctx");
    fs::write(&absolute, b"SENTINEL-ABSOLUTE").unwrap();
    let id = case.base.join("abs-victim");
    fs::write(&registry, format!("{} | {}\n", id.display(), root.display())).unwrap();
    let absolute_run = case.command().args(["."]).output().unwrap();
    assert!(!absolute_run.status.success(), "{}", report(&absolute_run));
    assert_eq!(fs::read(&absolute).unwrap(), b"SENTINEL-ABSOLUTE");
    fs::write(&registry, saved_registry).unwrap();
    case.run(&["."]);
    assert!(contains(&case.text(), b"included\n"));
    assert_eq!(fs::read(&sentinel).unwrap(), b"SENTINEL-OUTSIDE");
    assert_eq!(fs::read(&escaped).unwrap(), b"SENTINEL-TRAVERSAL");
    assert_eq!(fs::read(&absolute).unwrap(), b"SENTINEL-ABSOLUTE");
}

#[cfg(unix)]
#[test]
fn export_and_cache_links_do_not_overwrite_outside_sentinels() {
    use std::os::unix::fs::symlink;
    let case = Case::new();
    case.write("a.txt", b"included\n");
    let outside = case.base.join("linked-export");
    fs::create_dir(&outside).unwrap();
    let sentinel = outside.join("Corpus.llmcontext.txt");
    fs::write(&sentinel, b"SENTINEL-DIR").unwrap();
    symlink(&outside, case.root.join(".dctx")).unwrap();
    let denied = case.command().args(["."]).output().unwrap();
    assert!(!denied.status.success(), "{}", report(&denied));
    assert_eq!(fs::read(&sentinel).unwrap(), b"SENTINEL-DIR");
    fs::remove_file(case.root.join(".dctx")).unwrap();

    fs::create_dir(case.root.join(".dctx")).unwrap();
    let file_sentinel = case.base.join("sentinel-export.txt");
    fs::write(&file_sentinel, b"SENTINEL-FILE").unwrap();
    symlink(&file_sentinel, case.root.join(".dctx/Corpus.llmcontext.txt")).unwrap();
    case.run(&["."]);
    assert_eq!(fs::read(&file_sentinel).unwrap(), b"SENTINEL-FILE");
    let export = fs::read(case.root.join(".dctx/Corpus.llmcontext.txt")).unwrap();
    assert!(contains(&export, b"included\n"));
    assert!(!fs::symlink_metadata(case.root.join(".dctx/Corpus.llmcontext.txt")).unwrap().file_type().is_symlink());

    let cache_sentinel = case.base.join("sentinel-cache.bin");
    fs::write(&cache_sentinel, b"SENTINEL-CACHE").unwrap();
    let binary = case.binary();
    fs::remove_file(&binary).unwrap();
    symlink(&cache_sentinel, &binary).unwrap();
    case.write("a.txt", b"included again\n");
    case.run(&["."]);
    assert_eq!(fs::read(&cache_sentinel).unwrap(), b"SENTINEL-CACHE");
    assert!(!fs::symlink_metadata(&binary).unwrap().file_type().is_symlink());
    assert!(contains(&fs::read(&binary).unwrap(), b"included again\n"));
    assert_eq!(fs::read(&file_sentinel).unwrap(), b"SENTINEL-FILE");
}

#[test]
fn template_failure_keeps_the_previous_export_and_state() {
    let case = Case::new();
    case.write("a.txt", b"alpha\n");
    case.run(&["."]);
    let text = case.text();
    let binary = fs::read(case.binary()).unwrap();
    assert!(contains(&text, b"alpha\n"));
    assert_ne!(text, [] as [u8; 0]);

    case.write("a.txt", b"beta-changed\n");
    case.config("default_template = 'missing'\n");
    let missing = case.command().args(["."]).output().unwrap();
    assert!(!missing.status.success(), "{}", report(&missing));
    assert!(report(&missing).contains("Generation error"), "{}", report(&missing));
    assert_eq!(case.text(), text);
    assert_eq!(fs::read(case.binary()).unwrap(), binary);

    let directory = case.base.join("cache/dctx/templates");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("broken.bin"), b"not-a-template").unwrap();
    case.config("default_template = 'broken'\n");
    let malformed = case.command().args(["."]).output().unwrap();
    assert!(!malformed.status.success(), "{}", report(&malformed));
    assert!(report(&malformed).contains("Generation error"), "{}", report(&malformed));
    assert_eq!(case.text(), text);
    assert_eq!(fs::read(case.binary()).unwrap(), binary);

    let template = dctx_template::Template::parse(include_str!("../../dctx-template/templates/default.dctx_template"));
    let compiled = template.compile("custom").unwrap();
    compiled.serialize(&mut fs::File::create(directory.join("custom.bin")).unwrap()).unwrap();
    case.config("default_template = 'custom'\n");
    case.run(&["."]);
    let updated = case.text();
    assert_ne!(updated, text);
    assert!(contains(&updated, b"beta-changed\n"));
    assert_ne!(fs::read(case.binary()).unwrap(), binary);
}

#[test]
fn injected_file_limit_withholds_oversize_content_and_reads_the_control() {
    let case = Case::new();
    case.write("ok.txt", b"hello");
    case.write("large.txt", b"UNIQUE_OVERSIZE_SENTINEL");
    let output = case.command().env("DCTX_ALIAS_MAX_FILE_BYTES", "8").args(["."]).output().unwrap();
    assert!(output.status.success(), "{}", report(&output));
    let text = case.text();
    assert!(contains(&text, b"hello"));
    assert!(!contains(&text, b"UNIQUE_OVERSIZE_SENTINEL"));
    let cache = fs::read(case.binary()).unwrap();
    assert!(contains(&cache, b"hello"));
    assert!(!contains(&cache, b"UNIQUE_OVERSIZE_SENTINEL"));
}

#[test]
fn depth_and_entry_bounds_refuse_without_replacing_a_successful_export() {
    let depth = Case::new();
    depth.write("a.txt", b"top-level");
    let ok = depth.command().env("DCTX_ALIAS_MAX_DEPTH", "2").args(["."]).output().unwrap();
    assert!(ok.status.success(), "{}", report(&ok));
    let saved = depth.text();
    assert!(contains(&saved, b"top-level"));
    depth.write("d/e/f.txt", b"TOO_DEEP_SENTINEL");
    let denied = depth.command().env("DCTX_ALIAS_MAX_DEPTH", "2").args(["."]).output().unwrap();
    assert!(!denied.status.success(), "{}", report(&denied));
    assert_eq!(depth.text(), saved);
    assert!(!contains(&depth.text(), b"TOO_DEEP_SENTINEL"));
    assert!(!contains(&fs::read(depth.binary()).unwrap(), b"TOO_DEEP_SENTINEL"));

    let entries = Case::new();
    entries.write("a.txt", b"one-file");
    let first = entries.command().env("DCTX_ALIAS_MAX_ENTRIES", "2").args(["."]).output().unwrap();
    assert!(first.status.success(), "{}", report(&first));
    let kept = entries.text();
    assert!(contains(&kept, b"one-file"));
    entries.write("b.txt", b"ENTRY_SENTINEL");
    let too_many = entries.command().env("DCTX_ALIAS_MAX_ENTRIES", "2").args(["."]).output().unwrap();
    assert!(!too_many.status.success(), "{}", report(&too_many));
    assert_eq!(entries.text(), kept);
    assert!(!contains(&fs::read(entries.binary()).unwrap(), b"ENTRY_SENTINEL"));
}

#[test]
fn oversize_tree_snapshot_refuses_save_and_keeps_the_previous_export() {
    let case = Case::new();
    case.write("ok.txt", b"small-control\n");
    case.run(&["."]);
    let text = case.text();
    let binary = fs::read(case.binary()).unwrap();
    assert!(contains(&text, b"small-control\n"));
    let large = fs::File::create(case.root.join("large.txt")).unwrap();
    large.set_len(64 * 1024 * 1024 + 1).unwrap();
    let denied = case.command().args([".", "--tree"]).output().unwrap();
    assert!(!denied.status.success(), "{}", report(&denied));
    assert!(report(&denied).contains("Failed to save binary state"), "{}", report(&denied));
    assert_eq!(case.text(), text);
    assert_eq!(fs::read(case.binary()).unwrap(), binary);
    assert!(!contains(&case.text(), b"large.txt"));
}
