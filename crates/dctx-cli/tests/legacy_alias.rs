//! Binary compatibility checks for D3-LEGACY-REMOVAL §5 D3-04.
//! Each child receives a private HOME/XDG/cache and empty PATH; no Git is available.
//! Pinned renderer fixtures are immutable oracles; timestamps come from std FileTimes.
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
    for (tree, expected) in [
        (false, include_bytes!("../../dctx-template/tests/fixtures/legacy-snapshot.txt").as_slice()),
        (true, include_bytes!("../../dctx-template/tests/fixtures/legacy-tree.txt").as_slice()),
    ] {
        let case = Case::new();
        case.corpus();
        let mut args = vec![".", "--debug-ignored", "--non-empty", "--binary"];
        if tree {
            args.push("--tree");
        }
        case.run(&args);
        assert_eq!(case.text(), expected);
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
fn filters_do_not_change_state_and_local_omit_stays_presentation_only() {
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
    let state = decode_legacy(&bytes, LegacyBodies::Include, LegacyLimits::default()).unwrap();
    assert!(state.root.children.iter().any(|node| node.relative_path == "notes.txt"));
    assert_eq!(
        state.root.children.iter().find(|node| node.relative_path == "kept.rs").unwrap().file_content.as_deref(),
        Some(b"pub fn kept() {}\n".as_slice())
    );
    let failure = case.command().args([".", "--empty", "--non-empty"]).output().unwrap();
    assert!(!failure.status.success());
    assert_eq!(fs::read(case.binary()).unwrap(), bytes);
}

#[test]
fn configured_template_binary_only_and_rule_read_errors_keep_legacy_behavior() {
    let case = Case::new();
    case.corpus();
    let template = dctx_template::Template::parse(include_str!("../../dctx-template/templates/default.dctx_template"));
    let compiled = template.compile("custom").unwrap();
    let directory = case.base.join("cache/dctx/templates");
    fs::create_dir_all(&directory).unwrap();
    compiled.serialize(&mut fs::File::create(directory.join("custom.bin")).unwrap()).unwrap();
    case.config("default_template = 'custom'\n");
    case.run(&[".", "--debug-ignored", "--non-empty", "--binary"]);
    assert_eq!(case.text(), include_bytes!("../../dctx-template/tests/fixtures/legacy-snapshot.txt"));
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
